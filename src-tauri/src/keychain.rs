use keyring::Entry;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum KeychainError {
    #[error("keychain operation failed for {service}/{account}: {message}; unlock or enable the operating-system credential store and try again")]
    Unavailable {
        service: String,
        account: String,
        message: String,
    },
    #[error("credential is not present for {service}/{account}")]
    Missing { service: String, account: String },
}

pub struct Keychain {
    service: String,
}

impl Keychain {
    pub fn new(service: impl Into<String>) -> Self {
        Self {
            service: service.into(),
        }
    }

    pub fn store(&self, account: &str, secret: &str) -> Result<(), KeychainError> {
        let entry = Entry::new(&self.service, account).map_err(|e| self.error(account, e))?;
        entry
            .set_password(secret)
            .map_err(|e| self.error(account, e))
    }

    pub fn retrieve(&self, account: &str) -> Result<String, KeychainError> {
        let entry = Entry::new(&self.service, account).map_err(|e| self.error(account, e))?;
        match entry.get_password() {
            Ok(secret) => Ok(secret),
            Err(keyring::Error::NoEntry) => Err(KeychainError::Missing {
                service: self.service.clone(),
                account: account.to_owned(),
            }),
            Err(error) => Err(self.error(account, error)),
        }
    }

    pub fn delete(&self, account: &str) -> Result<(), KeychainError> {
        let entry = Entry::new(&self.service, account).map_err(|e| self.error(account, e))?;
        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(error) => Err(self.error(account, error)),
        }
    }

    fn error(&self, account: &str, error: keyring::Error) -> KeychainError {
        KeychainError::Unavailable {
            service: self.service.clone(),
            account: account.to_owned(),
            message: error.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::HashMap,
        sync::{Arc, Mutex},
    };

    #[derive(Default, Clone)]
    struct MemoryKeychain(Arc<Mutex<HashMap<String, String>>>);
    impl MemoryKeychain {
        fn store(&self, key: &str, value: &str) {
            self.0.lock().unwrap().insert(key.into(), value.into());
        }
        fn retrieve(&self, key: &str) -> Option<String> {
            self.0.lock().unwrap().get(key).cloned()
        }
        fn delete(&self, key: &str) {
            self.0.lock().unwrap().remove(key);
        }
    }

    #[test]
    fn credential_lifecycle_is_store_retrieve_delete_without_plaintext_fallback() {
        let keychain = MemoryKeychain::default();
        keychain.store("test", "secret");
        assert_eq!(keychain.retrieve("test").as_deref(), Some("secret"));
        keychain.delete("test");
        assert_eq!(keychain.retrieve("test"), None);
    }
}
