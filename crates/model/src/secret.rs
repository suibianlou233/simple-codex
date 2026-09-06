use std::fmt;

use crate::ModelError;

/// A model credential whose debug output never exposes the underlying value.
#[derive(Clone)]
pub struct ApiKey(String);

impl ApiKey {
    pub fn new(value: impl Into<String>) -> Result<Self, ModelError> {
        let value = value.into();
        if value.trim().is_empty() {
            return Err(ModelError::MissingCredential);
        }
        Ok(Self(value))
    }

    pub(crate) fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for ApiKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("ApiKey([REDACTED])")
    }
}

#[cfg(test)]
mod tests {
    use super::ApiKey;

    #[test]
    fn debug_never_contains_secret() {
        let key = ApiKey::new("test-secret-value").expect("test key should be accepted");
        let debug = format!("{key:?}");
        assert!(!debug.contains("test-secret-value"));
        assert!(debug.contains("REDACTED"));
    }
}
