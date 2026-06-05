use crate::error::StorageError;

pub(crate) fn normalize_email(email: &str) -> Result<String, StorageError> {
    let email = required_string("email", email)?.to_ascii_lowercase();

    if !email.contains('@') {
        return Err(StorageError::Validation(
            "email must include an @ sign".to_owned(),
        ));
    }

    Ok(email)
}

pub(crate) fn required_string(field: &str, value: &str) -> Result<String, StorageError> {
    let value = value.trim();

    if value.is_empty() {
        return Err(StorageError::Validation(format!("{field} is required")));
    }

    Ok(value.to_owned())
}

pub(crate) fn optional_string(
    field: &str,
    value: Option<String>,
) -> Result<Option<String>, StorageError> {
    value
        .map(|value| required_string(field, &value))
        .transpose()
}

pub(crate) fn validate_password(password: &str) -> Result<(), StorageError> {
    if password.len() < 8 {
        return Err(StorageError::Validation(
            "password must be at least 8 characters".to_owned(),
        ));
    }

    Ok(())
}

pub(crate) fn validate_port(port: i32) -> Result<(), StorageError> {
    if !(1..=65_535).contains(&port) {
        return Err(StorageError::Validation(
            "port must be between 1 and 65535".to_owned(),
        ));
    }

    Ok(())
}
