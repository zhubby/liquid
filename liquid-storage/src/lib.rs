use std::{error::Error, fmt};

use argon2::{
    Argon2, PasswordHash, PasswordHasher, PasswordVerifier,
    password_hash::{SaltString, rand_core::OsRng},
};
use async_trait::async_trait;
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use liquid_core::{
    AuditedDatabase, AuditedDatabaseEngine, AuditedDatabaseSslMode, AuthResponse,
    CreateAuditedDatabaseRequest, CurrentUserResponse, LoginRequest, PublicUser, RegisterRequest,
    UpdateAuditedDatabaseRequest,
};
use rand_core::RngCore;
use ring::aead::{AES_256_GCM, Aad, LessSafeKey, Nonce, UnboundKey};
use sha2::{Digest, Sha256};
use sqlx::{PgPool, postgres::PgPoolOptions};

const DEFAULT_TOKEN_TTL_SECONDS: i64 = 60 * 60 * 24 * 7;
const DEFAULT_ENCRYPTION_KEY: &str = "liquid-development-encryption-key-change-me";
const TOKEN_BYTES: usize = 32;
const NONCE_BYTES: usize = 12;

#[derive(Debug, Clone)]
pub struct Storage {
    pool: PgPool,
    token_ttl_seconds: i64,
    cipher: PasswordCipher,
}

#[derive(Debug, Clone)]
pub struct StorageOptions {
    pub database_url: String,
    pub max_connections: u32,
    pub token_ttl_seconds: i64,
    pub encryption_key: String,
}

impl StorageOptions {
    pub fn new(database_url: impl Into<String>) -> Self {
        Self {
            database_url: database_url.into(),
            max_connections: 5,
            token_ttl_seconds: DEFAULT_TOKEN_TTL_SECONDS,
            encryption_key: DEFAULT_ENCRYPTION_KEY.to_owned(),
        }
    }

    pub fn with_max_connections(mut self, max_connections: u32) -> Self {
        self.max_connections = max_connections;
        self
    }

    pub fn with_token_ttl_seconds(mut self, token_ttl_seconds: i64) -> Self {
        self.token_ttl_seconds = token_ttl_seconds;
        self
    }

    pub fn with_encryption_key(mut self, encryption_key: impl Into<String>) -> Self {
        self.encryption_key = encryption_key.into();
        self
    }
}

#[async_trait]
pub trait LiquidStore: Send + Sync {
    async fn register_user(&self, request: RegisterRequest) -> Result<AuthResponse, StorageError>;
    async fn login_user(&self, request: LoginRequest) -> Result<AuthResponse, StorageError>;
    async fn authenticate_token(&self, token: &str) -> Result<Option<PublicUser>, StorageError>;
    async fn revoke_token(&self, token: &str) -> Result<(), StorageError>;
    async fn list_audited_databases(
        &self,
        owner_user_id: &str,
    ) -> Result<Vec<AuditedDatabase>, StorageError>;
    async fn create_audited_database(
        &self,
        owner_user_id: &str,
        request: CreateAuditedDatabaseRequest,
    ) -> Result<AuditedDatabase, StorageError>;
    async fn update_audited_database(
        &self,
        owner_user_id: &str,
        id: &str,
        request: UpdateAuditedDatabaseRequest,
    ) -> Result<AuditedDatabase, StorageError>;
    async fn delete_audited_database(
        &self,
        owner_user_id: &str,
        id: &str,
    ) -> Result<(), StorageError>;
}

impl Storage {
    pub async fn connect(database_url: &str) -> Result<Self, sqlx::Error> {
        Self::connect_with_options(StorageOptions::new(database_url)).await
    }

    pub async fn connect_with_options(options: StorageOptions) -> Result<Self, sqlx::Error> {
        let pool = PgPoolOptions::new()
            .max_connections(options.max_connections)
            .connect(&options.database_url)
            .await?;

        Ok(Self {
            pool,
            token_ttl_seconds: options.token_ttl_seconds,
            cipher: PasswordCipher::new(&options.encryption_key),
        })
    }

    pub async fn migrate(&self) -> Result<(), sqlx::migrate::MigrateError> {
        sqlx::migrate!("./migrations").run(&self.pool).await
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    pub fn decrypt_audited_database_password(
        &self,
        encrypted_password: &str,
    ) -> Result<String, StorageError> {
        self.cipher.decrypt(encrypted_password)
    }
}

#[async_trait]
impl LiquidStore for Storage {
    async fn register_user(&self, request: RegisterRequest) -> Result<AuthResponse, StorageError> {
        let email = normalize_email(&request.email)?;
        let display_name = required_string("display_name", &request.display_name)?;
        validate_password(&request.password)?;
        let password_hash = hash_password(&request.password)?;
        let token = generate_token();
        let token_hash = hash_token(&token);

        let mut transaction = self.pool.begin().await?;
        let user = sqlx::query_as::<_, (String, String, String)>(
            r#"
            insert into users (email, display_name, password_hash)
            values ($1, $2, $3)
            returning id::text, email, display_name
            "#,
        )
        .bind(email)
        .bind(display_name)
        .bind(password_hash)
        .fetch_one(&mut *transaction)
        .await
        .map_err(map_database_error)?;

        sqlx::query(
            r#"
            insert into auth_tokens (user_id, token_hash, expires_at)
            values ($1::uuid, $2, now() + ($3::bigint * interval '1 second'))
            "#,
        )
        .bind(&user.0)
        .bind(token_hash)
        .bind(self.token_ttl_seconds)
        .execute(&mut *transaction)
        .await?;

        transaction.commit().await?;

        Ok(auth_response(
            token,
            self.token_ttl_seconds,
            public_user(user),
        ))
    }

    async fn login_user(&self, request: LoginRequest) -> Result<AuthResponse, StorageError> {
        let email = normalize_email(&request.email)?;
        let user = sqlx::query_as::<_, (String, String, String, String)>(
            r#"
            select id::text, email, display_name, password_hash
            from users
            where lower(email) = lower($1)
            "#,
        )
        .bind(email)
        .fetch_optional(&self.pool)
        .await?;

        let Some((id, email, display_name, password_hash)) = user else {
            return Err(StorageError::InvalidCredentials);
        };

        if !verify_password(&password_hash, &request.password) {
            return Err(StorageError::InvalidCredentials);
        }

        let token = generate_token();
        let token_hash = hash_token(&token);
        sqlx::query(
            r#"
            insert into auth_tokens (user_id, token_hash, expires_at)
            values ($1::uuid, $2, now() + ($3::bigint * interval '1 second'))
            "#,
        )
        .bind(&id)
        .bind(token_hash)
        .bind(self.token_ttl_seconds)
        .execute(&self.pool)
        .await?;

        Ok(auth_response(
            token,
            self.token_ttl_seconds,
            PublicUser {
                id,
                email,
                display_name,
            },
        ))
    }

    async fn authenticate_token(&self, token: &str) -> Result<Option<PublicUser>, StorageError> {
        if token.trim().is_empty() {
            return Ok(None);
        }

        let token_hash = hash_token(token);
        let user = sqlx::query_as::<_, (String, String, String)>(
            r#"
            select users.id::text, users.email, users.display_name
            from auth_tokens
            join users on users.id = auth_tokens.user_id
            where auth_tokens.token_hash = $1
              and auth_tokens.revoked_at is null
              and auth_tokens.expires_at > now()
            "#,
        )
        .bind(token_hash)
        .fetch_optional(&self.pool)
        .await?;

        Ok(user.map(public_user))
    }

    async fn revoke_token(&self, token: &str) -> Result<(), StorageError> {
        let token_hash = hash_token(token);
        sqlx::query(
            r#"
            update auth_tokens
            set revoked_at = now()
            where token_hash = $1
              and revoked_at is null
            "#,
        )
        .bind(token_hash)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    async fn list_audited_databases(
        &self,
        owner_user_id: &str,
    ) -> Result<Vec<AuditedDatabase>, StorageError> {
        let rows = sqlx::query_as::<_, AuditedDatabaseRow>(
            r#"
            select id::text, name, engine, host, port, database_name, username, ssl_mode,
                   encrypted_password <> '' as has_password
            from audited_databases
            where owner_user_id = $1::uuid
            order by lower(name)
            "#,
        )
        .bind(owner_user_id)
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter().map(AuditedDatabase::try_from).collect()
    }

    async fn create_audited_database(
        &self,
        owner_user_id: &str,
        request: CreateAuditedDatabaseRequest,
    ) -> Result<AuditedDatabase, StorageError> {
        let record = ValidatedAuditedDatabase::from_create(request)?;
        let encrypted_password = self.cipher.encrypt(&record.password)?;

        let row = sqlx::query_as::<_, AuditedDatabaseRow>(
            r#"
            insert into audited_databases (
                owner_user_id, name, engine, host, port, database_name, username,
                encrypted_password, ssl_mode
            )
            values ($1::uuid, $2, $3, $4, $5, $6, $7, $8, $9)
            returning id::text, name, engine, host, port, database_name, username, ssl_mode,
                      encrypted_password <> '' as has_password
            "#,
        )
        .bind(owner_user_id)
        .bind(record.name)
        .bind(record.engine.as_str())
        .bind(record.host)
        .bind(record.port)
        .bind(record.database)
        .bind(record.username)
        .bind(encrypted_password)
        .bind(record.ssl_mode.as_str())
        .fetch_one(&self.pool)
        .await
        .map_err(map_database_error)?;

        AuditedDatabase::try_from(row)
    }

    async fn update_audited_database(
        &self,
        owner_user_id: &str,
        id: &str,
        request: UpdateAuditedDatabaseRequest,
    ) -> Result<AuditedDatabase, StorageError> {
        let update = ValidatedAuditedDatabaseUpdate::from_update(request, &self.cipher)?;
        let row = sqlx::query_as::<_, AuditedDatabaseRow>(
            r#"
            update audited_databases
            set name = coalesce($3::text, name),
                host = coalesce($4::text, host),
                port = coalesce($5::integer, port),
                database_name = coalesce($6::text, database_name),
                username = coalesce($7::text, username),
                encrypted_password = coalesce($8::text, encrypted_password),
                ssl_mode = coalesce($9::text, ssl_mode),
                updated_at = now()
            where id = $1::uuid
              and owner_user_id = $2::uuid
            returning id::text, name, engine, host, port, database_name, username, ssl_mode,
                      encrypted_password <> '' as has_password
            "#,
        )
        .bind(id)
        .bind(owner_user_id)
        .bind(update.name)
        .bind(update.host)
        .bind(update.port)
        .bind(update.database)
        .bind(update.username)
        .bind(update.encrypted_password)
        .bind(update.ssl_mode.map(|mode| mode.as_str().to_owned()))
        .fetch_optional(&self.pool)
        .await
        .map_err(map_database_error)?;

        let Some(row) = row else {
            return Err(StorageError::NotFound);
        };

        AuditedDatabase::try_from(row)
    }

    async fn delete_audited_database(
        &self,
        owner_user_id: &str,
        id: &str,
    ) -> Result<(), StorageError> {
        let result = sqlx::query(
            r#"
            delete from audited_databases
            where id = $1::uuid
              and owner_user_id = $2::uuid
            "#,
        )
        .bind(id)
        .bind(owner_user_id)
        .execute(&self.pool)
        .await?;

        if result.rows_affected() == 0 {
            return Err(StorageError::NotFound);
        }

        Ok(())
    }
}

#[derive(Debug)]
pub enum StorageError {
    DuplicateEmail,
    DuplicateAuditedDatabaseName,
    InvalidCredentials,
    NotFound,
    Validation(String),
    Database(sqlx::Error),
    Crypto(String),
}

impl fmt::Display for StorageError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateEmail => write!(formatter, "email already registered"),
            Self::DuplicateAuditedDatabaseName => {
                write!(formatter, "audited database name already exists")
            }
            Self::InvalidCredentials => write!(formatter, "invalid email or password"),
            Self::NotFound => write!(formatter, "record not found"),
            Self::Validation(message) => write!(formatter, "{message}"),
            Self::Database(error) => write!(formatter, "{error}"),
            Self::Crypto(message) => write!(formatter, "{message}"),
        }
    }
}

impl Error for StorageError {}

impl From<sqlx::Error> for StorageError {
    fn from(error: sqlx::Error) -> Self {
        map_database_error(error)
    }
}

#[derive(Debug, Clone)]
struct PasswordCipher {
    key: [u8; 32],
}

impl PasswordCipher {
    fn new(secret: &str) -> Self {
        let digest = Sha256::digest(secret.as_bytes());
        let mut key = [0u8; 32];
        key.copy_from_slice(&digest);
        Self { key }
    }

    fn encrypt(&self, plaintext: &str) -> Result<String, StorageError> {
        let mut nonce_bytes = [0u8; NONCE_BYTES];
        OsRng.fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::assume_unique_for_key(nonce_bytes);
        let key = self.less_safe_key()?;
        let mut in_out = plaintext.as_bytes().to_vec();

        key.seal_in_place_append_tag(nonce, Aad::empty(), &mut in_out)
            .map_err(|_| {
                StorageError::Crypto("failed to encrypt audited database password".into())
            })?;

        let mut combined = nonce_bytes.to_vec();
        combined.extend(in_out);
        Ok(URL_SAFE_NO_PAD.encode(combined))
    }

    fn decrypt(&self, ciphertext: &str) -> Result<String, StorageError> {
        let mut combined = URL_SAFE_NO_PAD.decode(ciphertext).map_err(|_| {
            StorageError::Crypto("invalid encrypted audited database password".into())
        })?;

        if combined.len() <= NONCE_BYTES {
            return Err(StorageError::Crypto(
                "invalid encrypted audited database password".into(),
            ));
        }

        let mut nonce_bytes = [0u8; NONCE_BYTES];
        nonce_bytes.copy_from_slice(&combined[..NONCE_BYTES]);
        let nonce = Nonce::assume_unique_for_key(nonce_bytes);
        let mut encrypted = combined.split_off(NONCE_BYTES);
        let key = self.less_safe_key()?;
        let plaintext = key
            .open_in_place(nonce, Aad::empty(), &mut encrypted)
            .map_err(|_| {
                StorageError::Crypto("failed to decrypt audited database password".into())
            })?;

        String::from_utf8(plaintext.to_vec())
            .map_err(|_| StorageError::Crypto("decrypted password is not utf-8".into()))
    }

    fn less_safe_key(&self) -> Result<LessSafeKey, StorageError> {
        let unbound = UnboundKey::new(&AES_256_GCM, &self.key)
            .map_err(|_| StorageError::Crypto("invalid encryption key".into()))?;
        Ok(LessSafeKey::new(unbound))
    }
}

#[derive(Debug)]
struct AuditedDatabaseRow {
    id: String,
    name: String,
    engine: String,
    host: String,
    port: i32,
    database_name: String,
    username: String,
    ssl_mode: String,
    has_password: bool,
}

impl<'r> sqlx::FromRow<'r, sqlx::postgres::PgRow> for AuditedDatabaseRow {
    fn from_row(row: &'r sqlx::postgres::PgRow) -> Result<Self, sqlx::Error> {
        use sqlx::Row;

        Ok(Self {
            id: row.try_get("id")?,
            name: row.try_get("name")?,
            engine: row.try_get("engine")?,
            host: row.try_get("host")?,
            port: row.try_get("port")?,
            database_name: row.try_get("database_name")?,
            username: row.try_get("username")?,
            ssl_mode: row.try_get("ssl_mode")?,
            has_password: row.try_get("has_password")?,
        })
    }
}

impl TryFrom<AuditedDatabaseRow> for AuditedDatabase {
    type Error = StorageError;

    fn try_from(row: AuditedDatabaseRow) -> Result<Self, Self::Error> {
        Ok(Self {
            id: row.id,
            name: row.name,
            engine: parse_engine(&row.engine)?,
            host: row.host,
            port: row.port,
            database: row.database_name,
            username: row.username,
            ssl_mode: parse_ssl_mode(&row.ssl_mode)?,
            has_password: row.has_password,
        })
    }
}

#[derive(Debug)]
struct ValidatedAuditedDatabase {
    name: String,
    engine: AuditedDatabaseEngine,
    host: String,
    port: i32,
    database: String,
    username: String,
    password: String,
    ssl_mode: AuditedDatabaseSslMode,
}

impl ValidatedAuditedDatabase {
    fn from_create(request: CreateAuditedDatabaseRequest) -> Result<Self, StorageError> {
        validate_port(request.port)?;

        Ok(Self {
            name: required_string("name", &request.name)?,
            engine: request.engine,
            host: required_string("host", &request.host)?,
            port: request.port,
            database: required_string("database", &request.database)?,
            username: required_string("username", &request.username)?,
            password: required_string("password", &request.password)?,
            ssl_mode: request.ssl_mode,
        })
    }
}

#[derive(Debug)]
struct ValidatedAuditedDatabaseUpdate {
    name: Option<String>,
    host: Option<String>,
    port: Option<i32>,
    database: Option<String>,
    username: Option<String>,
    encrypted_password: Option<String>,
    ssl_mode: Option<AuditedDatabaseSslMode>,
}

impl ValidatedAuditedDatabaseUpdate {
    fn from_update(
        request: UpdateAuditedDatabaseRequest,
        cipher: &PasswordCipher,
    ) -> Result<Self, StorageError> {
        if let Some(port) = request.port {
            validate_port(port)?;
        }

        let encrypted_password = match request.password {
            Some(password) => Some(cipher.encrypt(&required_string("password", &password)?)?),
            None => None,
        };

        Ok(Self {
            name: optional_string("name", request.name)?,
            host: optional_string("host", request.host)?,
            port: request.port,
            database: optional_string("database", request.database)?,
            username: optional_string("username", request.username)?,
            encrypted_password,
            ssl_mode: request.ssl_mode,
        })
    }
}

fn public_user(row: (String, String, String)) -> PublicUser {
    PublicUser {
        id: row.0,
        email: row.1,
        display_name: row.2,
    }
}

fn auth_response(token: String, expires_in_seconds: i64, user: PublicUser) -> AuthResponse {
    AuthResponse {
        token,
        token_type: "Bearer".to_owned(),
        expires_in_seconds,
        user,
    }
}

fn normalize_email(email: &str) -> Result<String, StorageError> {
    let email = required_string("email", email)?.to_ascii_lowercase();

    if !email.contains('@') {
        return Err(StorageError::Validation(
            "email must include an @ sign".to_owned(),
        ));
    }

    Ok(email)
}

fn required_string(field: &str, value: &str) -> Result<String, StorageError> {
    let value = value.trim();

    if value.is_empty() {
        return Err(StorageError::Validation(format!("{field} is required")));
    }

    Ok(value.to_owned())
}

fn optional_string(field: &str, value: Option<String>) -> Result<Option<String>, StorageError> {
    value
        .map(|value| required_string(field, &value))
        .transpose()
}

fn validate_password(password: &str) -> Result<(), StorageError> {
    if password.len() < 8 {
        return Err(StorageError::Validation(
            "password must be at least 8 characters".to_owned(),
        ));
    }

    Ok(())
}

fn validate_port(port: i32) -> Result<(), StorageError> {
    if !(1..=65_535).contains(&port) {
        return Err(StorageError::Validation(
            "port must be between 1 and 65535".to_owned(),
        ));
    }

    Ok(())
}

fn hash_password(password: &str) -> Result<String, StorageError> {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(|error| StorageError::Crypto(format!("failed to hash password: {error}")))
}

fn verify_password(password_hash: &str, password: &str) -> bool {
    let Ok(parsed_hash) = PasswordHash::new(password_hash) else {
        return false;
    };

    Argon2::default()
        .verify_password(password.as_bytes(), &parsed_hash)
        .is_ok()
}

fn generate_token() -> String {
    let mut bytes = [0u8; TOKEN_BYTES];
    OsRng.fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

fn hash_token(token: &str) -> String {
    hex::encode(Sha256::digest(token.as_bytes()))
}

fn parse_engine(value: &str) -> Result<AuditedDatabaseEngine, StorageError> {
    match value {
        "postgres" => Ok(AuditedDatabaseEngine::Postgres),
        other => Err(StorageError::Validation(format!(
            "unsupported audited database engine: {other}"
        ))),
    }
}

fn parse_ssl_mode(value: &str) -> Result<AuditedDatabaseSslMode, StorageError> {
    match value {
        "disable" => Ok(AuditedDatabaseSslMode::Disable),
        "prefer" => Ok(AuditedDatabaseSslMode::Prefer),
        "require" => Ok(AuditedDatabaseSslMode::Require),
        other => Err(StorageError::Validation(format!(
            "unsupported audited database ssl mode: {other}"
        ))),
    }
}

fn map_database_error(error: sqlx::Error) -> StorageError {
    let sqlx::Error::Database(database_error) = &error else {
        return StorageError::Database(error);
    };

    if database_error.code().as_deref() == Some("23505") {
        return match database_error.constraint() {
            Some("users_email_unique_idx") => StorageError::DuplicateEmail,
            Some("audited_databases_owner_name_unique_idx") => {
                StorageError::DuplicateAuditedDatabaseName
            }
            _ => StorageError::Database(error),
        };
    }

    StorageError::Database(error)
}

pub fn current_user_response(user: PublicUser) -> CurrentUserResponse {
    CurrentUserResponse { user }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn password_hash_round_trip_verifies_only_original_password() {
        let password_hash = hash_password("correct horse battery staple").unwrap();

        assert!(verify_password(
            &password_hash,
            "correct horse battery staple"
        ));
        assert!(!verify_password(&password_hash, "wrong password"));
    }

    #[test]
    fn token_hash_is_stable_without_storing_raw_token() {
        let token = generate_token();
        let token_hash = hash_token(&token);

        assert_eq!(token_hash, hash_token(&token));
        assert_ne!(token_hash, token);
        assert_eq!(token_hash.len(), 64);
    }

    #[test]
    fn audited_database_password_encryption_round_trips() {
        let cipher = PasswordCipher::new("test-secret");
        let encrypted = cipher.encrypt("postgres-password").unwrap();

        assert_ne!(encrypted, "postgres-password");
        assert_eq!(cipher.decrypt(&encrypted).unwrap(), "postgres-password");
    }

    #[test]
    fn audited_database_password_encryption_rejects_wrong_key() {
        let cipher = PasswordCipher::new("test-secret");
        let wrong_cipher = PasswordCipher::new("different-secret");
        let encrypted = cipher.encrypt("postgres-password").unwrap();

        assert!(wrong_cipher.decrypt(&encrypted).is_err());
    }

    #[test]
    fn create_audited_database_validation_rejects_bad_port() {
        let request = CreateAuditedDatabaseRequest {
            name: "Warehouse".to_owned(),
            engine: AuditedDatabaseEngine::Postgres,
            host: "localhost".to_owned(),
            port: 70_000,
            database: "warehouse".to_owned(),
            username: "readonly".to_owned(),
            password: "secret".to_owned(),
            ssl_mode: AuditedDatabaseSslMode::Prefer,
        };

        let error = ValidatedAuditedDatabase::from_create(request).unwrap_err();

        assert!(error.to_string().contains("port must be between"));
    }
}
