use argon2::{
    Argon2, PasswordHash, PasswordHasher, PasswordVerifier,
    password_hash::{
        SaltString,
        rand_core::{OsRng, RngCore},
    },
};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use liquid_core::{
    AuthResponse, CurrentUserResponse, LoginRequest, PublicUser, RegisterRequest,
    UpdateCurrentUserRequest, UpdatePasswordRequest,
};
use sha2::{Digest, Sha256};

use crate::{
    error::{StorageError, map_database_error},
    store::Storage,
    validation::{normalize_email, required_string, validate_password},
};

const TOKEN_BYTES: usize = 32;

pub(crate) async fn register_user(
    storage: &Storage,
    request: RegisterRequest,
) -> Result<AuthResponse, StorageError> {
    let email = normalize_email(&request.email)?;
    let display_name = required_string("display_name", &request.display_name)?;
    validate_password(&request.password)?;
    let password_hash = hash_password(&request.password)?;
    let token = generate_token();
    let token_hash = hash_token(&token);

    let mut transaction = storage.pool.begin().await?;
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
    .bind(storage.token_ttl_seconds)
    .execute(&mut *transaction)
    .await?;

    transaction.commit().await?;

    Ok(auth_response(
        token,
        storage.token_ttl_seconds,
        public_user(user),
    ))
}

pub(crate) async fn login_user(
    storage: &Storage,
    request: LoginRequest,
) -> Result<AuthResponse, StorageError> {
    let email = normalize_email(&request.email)?;
    let user = sqlx::query_as::<_, (String, String, String, String)>(
        r#"
        select id::text, email, display_name, password_hash
        from users
        where lower(email) = lower($1)
        "#,
    )
    .bind(email)
    .fetch_optional(&storage.pool)
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
    .bind(storage.token_ttl_seconds)
    .execute(&storage.pool)
    .await?;

    Ok(auth_response(
        token,
        storage.token_ttl_seconds,
        PublicUser {
            id,
            email,
            display_name,
        },
    ))
}

pub(crate) async fn authenticate_token(
    storage: &Storage,
    token: &str,
) -> Result<Option<PublicUser>, StorageError> {
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
    .fetch_optional(&storage.pool)
    .await?;

    Ok(user.map(public_user))
}

pub(crate) async fn update_current_user(
    storage: &Storage,
    owner_user_id: &str,
    request: UpdateCurrentUserRequest,
) -> Result<PublicUser, StorageError> {
    let display_name = required_string("display_name", &request.display_name)?;
    let row = sqlx::query_as::<_, (String, String, String)>(
        r#"
        update users
        set display_name = $2,
            updated_at = now()
        where id = $1::uuid
        returning id::text, email, display_name
        "#,
    )
    .bind(owner_user_id)
    .bind(display_name)
    .fetch_optional(&storage.pool)
    .await?;

    row.map(public_user).ok_or(StorageError::NotFound)
}

pub(crate) async fn update_password(
    storage: &Storage,
    owner_user_id: &str,
    request: UpdatePasswordRequest,
) -> Result<(), StorageError> {
    validate_password(&request.new_password)?;
    let row = sqlx::query_as::<_, (String,)>(
        r#"
        select password_hash
        from users
        where id = $1::uuid
        "#,
    )
    .bind(owner_user_id)
    .fetch_optional(&storage.pool)
    .await?;

    let Some((password_hash,)) = row else {
        return Err(StorageError::NotFound);
    };

    if !verify_password(&password_hash, &request.current_password) {
        return Err(StorageError::InvalidCredentials);
    }

    let new_password_hash = hash_password(&request.new_password)?;
    sqlx::query(
        r#"
        update users
        set password_hash = $2,
            updated_at = now()
        where id = $1::uuid
        "#,
    )
    .bind(owner_user_id)
    .bind(new_password_hash)
    .execute(&storage.pool)
    .await?;

    Ok(())
}

pub(crate) async fn revoke_token(storage: &Storage, token: &str) -> Result<(), StorageError> {
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
    .execute(&storage.pool)
    .await?;

    Ok(())
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
}
