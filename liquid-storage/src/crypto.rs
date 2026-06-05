use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use rand_core::{OsRng, RngCore};
use ring::aead::{AES_256_GCM, Aad, LessSafeKey, Nonce, UnboundKey};
use sha2::{Digest, Sha256};

use crate::error::StorageError;

const NONCE_BYTES: usize = 12;

#[derive(Debug, Clone)]
pub(crate) struct PasswordCipher {
    key: [u8; 32],
}

impl PasswordCipher {
    pub(crate) fn new(secret: &str) -> Self {
        let digest = Sha256::digest(secret.as_bytes());
        let mut key = [0u8; 32];
        key.copy_from_slice(&digest);
        Self { key }
    }

    pub(crate) fn encrypt(&self, plaintext: &str) -> Result<String, StorageError> {
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

    pub(crate) fn decrypt(&self, ciphertext: &str) -> Result<String, StorageError> {
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
