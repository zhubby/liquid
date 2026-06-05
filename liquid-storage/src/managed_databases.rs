use liquid_core::{
    CreateManagedDatabaseRequest, ManagedDatabase, ManagedDatabaseConnectionSpec,
    ManagedDatabaseEngine, ManagedDatabasePoolKey, ManagedDatabaseSnapshot, ManagedDatabaseSslMode,
    UpdateManagedDatabaseRequest,
};

use crate::{
    crypto::PasswordCipher,
    error::{StorageError, map_database_error},
    store::Storage,
    validation::{optional_string, required_string, validate_port},
};

pub(crate) async fn list_managed_databases(
    storage: &Storage,
    owner_user_id: &str,
) -> Result<Vec<ManagedDatabase>, StorageError> {
    let rows = sqlx::query_as::<_, ManagedDatabaseRow>(
        r#"
        select id::text, name, engine, host, port, database_name, username, ssl_mode,
               encrypted_password <> '' as has_password
        from managed_databases
        where owner_user_id = $1::uuid
        order by lower(name)
        "#,
    )
    .bind(owner_user_id)
    .fetch_all(&storage.pool)
    .await?;

    rows.into_iter().map(ManagedDatabase::try_from).collect()
}

pub(crate) async fn get_current_managed_database(
    storage: &Storage,
    owner_user_id: &str,
) -> Result<Option<ManagedDatabase>, StorageError> {
    let row = sqlx::query_as::<_, ManagedDatabaseRow>(
        r#"
        select managed_databases.id::text, managed_databases.name, managed_databases.engine,
               managed_databases.host, managed_databases.port,
               managed_databases.database_name, managed_databases.username,
               managed_databases.ssl_mode,
               managed_databases.encrypted_password <> '' as has_password
        from user_managed_database_preferences
        join managed_databases
          on managed_databases.id = user_managed_database_preferences.current_managed_database_id
         and managed_databases.owner_user_id = user_managed_database_preferences.owner_user_id
        where user_managed_database_preferences.owner_user_id = $1::uuid
        "#,
    )
    .bind(owner_user_id)
    .fetch_optional(&storage.pool)
    .await
    .map_err(map_database_error)?;

    row.map(ManagedDatabase::try_from).transpose()
}

pub(crate) async fn set_current_managed_database(
    storage: &Storage,
    owner_user_id: &str,
    managed_database_id: &str,
) -> Result<ManagedDatabase, StorageError> {
    let mut transaction = storage.pool.begin().await?;
    let row = sqlx::query_as::<_, ManagedDatabaseRow>(
        r#"
        select id::text, name, engine, host, port, database_name, username, ssl_mode,
               encrypted_password <> '' as has_password
        from managed_databases
        where id = $1::uuid
          and owner_user_id = $2::uuid
        "#,
    )
    .bind(managed_database_id)
    .bind(owner_user_id)
    .fetch_optional(&mut *transaction)
    .await
    .map_err(map_database_error)?;

    let Some(row) = row else {
        return Err(StorageError::NotFound);
    };

    sqlx::query(
        r#"
        insert into user_managed_database_preferences (
            owner_user_id, current_managed_database_id, updated_at
        )
        values ($1::uuid, $2::uuid, now())
        on conflict (owner_user_id) do update
        set current_managed_database_id = excluded.current_managed_database_id,
            updated_at = now()
        "#,
    )
    .bind(owner_user_id)
    .bind(managed_database_id)
    .execute(&mut *transaction)
    .await
    .map_err(map_database_error)?;

    transaction.commit().await?;

    ManagedDatabase::try_from(row)
}

pub(crate) async fn clear_current_managed_database(
    storage: &Storage,
    owner_user_id: &str,
) -> Result<(), StorageError> {
    sqlx::query(
        r#"
        delete from user_managed_database_preferences
        where owner_user_id = $1::uuid
        "#,
    )
    .bind(owner_user_id)
    .execute(&storage.pool)
    .await
    .map_err(map_database_error)?;

    Ok(())
}

pub(crate) async fn create_managed_database(
    storage: &Storage,
    owner_user_id: &str,
    request: CreateManagedDatabaseRequest,
) -> Result<ManagedDatabase, StorageError> {
    let record = ValidatedManagedDatabase::from_create(request)?;
    let encrypted_password = storage.cipher.encrypt(&record.password)?;

    let row = sqlx::query_as::<_, ManagedDatabaseRow>(
        r#"
        insert into managed_databases (
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
    .fetch_one(&storage.pool)
    .await
    .map_err(map_database_error)?;

    ManagedDatabase::try_from(row)
}

pub(crate) async fn update_managed_database(
    storage: &Storage,
    owner_user_id: &str,
    id: &str,
    request: UpdateManagedDatabaseRequest,
) -> Result<ManagedDatabase, StorageError> {
    let update = ValidatedManagedDatabaseUpdate::from_update(request, &storage.cipher)?;
    let row = sqlx::query_as::<_, ManagedDatabaseRow>(
        r#"
        update managed_databases
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
    .fetch_optional(&storage.pool)
    .await
    .map_err(map_database_error)?;

    let Some(row) = row else {
        return Err(StorageError::NotFound);
    };

    ManagedDatabase::try_from(row)
}

pub(crate) async fn delete_managed_database(
    storage: &Storage,
    owner_user_id: &str,
    id: &str,
) -> Result<(), StorageError> {
    let result = sqlx::query(
        r#"
        delete from managed_databases
        where id = $1::uuid
          and owner_user_id = $2::uuid
        "#,
    )
    .bind(id)
    .bind(owner_user_id)
    .execute(&storage.pool)
    .await?;

    if result.rows_affected() == 0 {
        return Err(StorageError::NotFound);
    }

    Ok(())
}

pub(crate) async fn load_managed_database_connection(
    storage: &Storage,
    key: &ManagedDatabasePoolKey,
) -> Result<ManagedDatabaseConnectionSpec, StorageError> {
    let row = sqlx::query_as::<_, ManagedDatabaseConnectionRow>(
        r#"
        select engine, host, port, database_name, username, encrypted_password, ssl_mode
        from managed_databases
        where id = $1::uuid
          and owner_user_id = $2::uuid
        "#,
    )
    .bind(&key.database_id)
    .bind(&key.owner_user_id)
    .fetch_optional(&storage.pool)
    .await
    .map_err(map_database_error)?;

    let Some(row) = row else {
        return Err(StorageError::NotFound);
    };

    row.into_connection_spec(storage)
}

#[derive(Debug)]
struct ManagedDatabaseRow {
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

#[derive(Debug)]
struct ManagedDatabaseConnectionRow {
    engine: String,
    host: String,
    port: i32,
    database_name: String,
    username: String,
    encrypted_password: String,
    ssl_mode: String,
}

impl<'r> sqlx::FromRow<'r, sqlx::postgres::PgRow> for ManagedDatabaseRow {
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

impl TryFrom<ManagedDatabaseRow> for ManagedDatabase {
    type Error = StorageError;

    fn try_from(row: ManagedDatabaseRow) -> Result<Self, Self::Error> {
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

impl<'r> sqlx::FromRow<'r, sqlx::postgres::PgRow> for ManagedDatabaseConnectionRow {
    fn from_row(row: &'r sqlx::postgres::PgRow) -> Result<Self, sqlx::Error> {
        use sqlx::Row;

        Ok(Self {
            engine: row.try_get("engine")?,
            host: row.try_get("host")?,
            port: row.try_get("port")?,
            database_name: row.try_get("database_name")?,
            username: row.try_get("username")?,
            encrypted_password: row.try_get("encrypted_password")?,
            ssl_mode: row.try_get("ssl_mode")?,
        })
    }
}

impl ManagedDatabaseConnectionRow {
    fn into_connection_spec(
        self,
        storage: &Storage,
    ) -> Result<ManagedDatabaseConnectionSpec, StorageError> {
        let port = u16::try_from(self.port).map_err(|_| {
            StorageError::Validation("managed database port must be between 1 and 65535".to_owned())
        })?;
        let password = storage.decrypt_managed_database_password(&self.encrypted_password)?;

        Ok(ManagedDatabaseConnectionSpec {
            engine: parse_engine(&self.engine)?,
            host: self.host,
            port,
            database: self.database_name,
            username: self.username,
            password,
            ssl_mode: parse_ssl_mode(&self.ssl_mode)?,
        })
    }
}

pub(crate) async fn load_managed_database_snapshot(
    storage: &Storage,
    owner_user_id: &str,
    id: &str,
) -> Result<ManagedDatabaseSnapshot, StorageError> {
    let row = sqlx::query_as::<_, ManagedDatabaseRow>(
        r#"
        select id::text, name, engine, host, port, database_name, username, ssl_mode,
               encrypted_password <> '' as has_password
        from managed_databases
        where id = $1::uuid
          and owner_user_id = $2::uuid
        "#,
    )
    .bind(id)
    .bind(owner_user_id)
    .fetch_optional(&storage.pool)
    .await
    .map_err(map_database_error)?;

    let Some(row) = row else {
        return Err(StorageError::NotFound);
    };

    Ok(ManagedDatabaseSnapshot {
        id: row.id,
        name: row.name,
        engine: parse_engine(&row.engine)?,
        host: row.host,
        port: row.port,
        database: row.database_name,
        username: row.username,
        ssl_mode: parse_ssl_mode(&row.ssl_mode)?,
    })
}

#[derive(Debug)]
struct ValidatedManagedDatabase {
    name: String,
    engine: ManagedDatabaseEngine,
    host: String,
    port: i32,
    database: String,
    username: String,
    password: String,
    ssl_mode: ManagedDatabaseSslMode,
}

impl ValidatedManagedDatabase {
    fn from_create(request: CreateManagedDatabaseRequest) -> Result<Self, StorageError> {
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
struct ValidatedManagedDatabaseUpdate {
    name: Option<String>,
    host: Option<String>,
    port: Option<i32>,
    database: Option<String>,
    username: Option<String>,
    encrypted_password: Option<String>,
    ssl_mode: Option<ManagedDatabaseSslMode>,
}

impl ValidatedManagedDatabaseUpdate {
    fn from_update(
        request: UpdateManagedDatabaseRequest,
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

pub(crate) fn parse_engine(value: &str) -> Result<ManagedDatabaseEngine, StorageError> {
    match value {
        "postgres" => Ok(ManagedDatabaseEngine::Postgres),
        other => Err(StorageError::Validation(format!(
            "unsupported managed database engine: {other}"
        ))),
    }
}

pub(crate) fn parse_ssl_mode(value: &str) -> Result<ManagedDatabaseSslMode, StorageError> {
    match value {
        "disable" => Ok(ManagedDatabaseSslMode::Disable),
        "prefer" => Ok(ManagedDatabaseSslMode::Prefer),
        "require" => Ok(ManagedDatabaseSslMode::Require),
        other => Err(StorageError::Validation(format!(
            "unsupported managed database ssl mode: {other}"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use liquid_core::{
        CreateManagedDatabaseRequest, ManagedDatabaseEngine, ManagedDatabaseSslMode,
    };

    use super::*;

    #[test]
    fn create_managed_database_validation_rejects_bad_port() {
        let request = CreateManagedDatabaseRequest {
            name: "Warehouse".to_owned(),
            engine: ManagedDatabaseEngine::Postgres,
            host: "localhost".to_owned(),
            port: 70_000,
            database: "warehouse".to_owned(),
            username: "readonly".to_owned(),
            password: "secret".to_owned(),
            ssl_mode: ManagedDatabaseSslMode::Prefer,
        };

        let error = ValidatedManagedDatabase::from_create(request).unwrap_err();

        assert!(error.to_string().contains("port must be between"));
    }
}
