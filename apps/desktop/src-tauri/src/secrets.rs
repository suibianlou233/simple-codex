use thiserror::Error;

const KEYRING_SERVICE: &str = "dev.localagent.desktop";

#[derive(Debug, Error)]
pub enum SecretStoreError {
    #[error("系统凭据库不可用")]
    Unavailable,
    #[error("未找到模型凭据")]
    NotFound,
}

pub trait SecretStore: Send + Sync {
    fn set(&self, reference: &str, secret: &str) -> Result<(), SecretStoreError>;
    fn get(&self, reference: &str) -> Result<String, SecretStoreError>;
}

pub struct SystemSecretStore;

impl SecretStore for SystemSecretStore {
    fn set(&self, reference: &str, secret: &str) -> Result<(), SecretStoreError> {
        entry(reference)?.set_password(secret).map_err(map_error)
    }

    fn get(&self, reference: &str) -> Result<String, SecretStoreError> {
        entry(reference)?.get_password().map_err(map_error)
    }
}

fn entry(reference: &str) -> Result<keyring::Entry, SecretStoreError> {
    keyring::Entry::new(KEYRING_SERVICE, reference).map_err(map_error)
}

fn map_error(error: keyring::Error) -> SecretStoreError {
    match error {
        keyring::Error::NoEntry => SecretStoreError::NotFound,
        _ => SecretStoreError::Unavailable,
    }
}

#[cfg(test)]
pub struct MemorySecretStore {
    entries: std::sync::RwLock<std::collections::HashMap<String, String>>,
}

#[cfg(test)]
impl MemorySecretStore {
    pub fn new() -> Self {
        Self {
            entries: std::sync::RwLock::new(std::collections::HashMap::new()),
        }
    }
}

#[cfg(test)]
impl SecretStore for MemorySecretStore {
    fn set(&self, reference: &str, secret: &str) -> Result<(), SecretStoreError> {
        self.entries
            .write()
            .map_err(|_| SecretStoreError::Unavailable)?
            .insert(reference.to_owned(), secret.to_owned());
        Ok(())
    }

    fn get(&self, reference: &str) -> Result<String, SecretStoreError> {
        self.entries
            .read()
            .map_err(|_| SecretStoreError::Unavailable)?
            .get(reference)
            .cloned()
            .ok_or(SecretStoreError::NotFound)
    }
}
