use async_trait::async_trait;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use chrono::Utc;
use p256::ecdsa::{Signature, SigningKey, signature::Signer};
use p256::pkcs8::DecodePrivateKey;
use secrecy::{ExposeSecret, SecretString};
use serde::Serialize;
use std::collections::HashMap;
use thiserror::Error;

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct CredentialMetadata {
    pub team_id: String,
    pub key_id: String,
    pub service_id: String,
}

#[derive(Debug, Error)]
pub enum AuthError {
    #[error("WeatherKit credentials are incomplete")]
    Incomplete,
    #[error("the private key is not a valid ES256 PKCS#8 .p8 key")]
    InvalidKey,
    #[error("GNOME Keyring is unavailable: {0}")]
    Keyring(String),
    #[error("failed to construct developer token: {0}")]
    Token(String),
}

#[async_trait]
pub trait TokenProvider: Send + Sync {
    async fn token(&self) -> Result<String, AuthError>;
}

#[async_trait]
pub trait SecretStore: Send + Sync {
    async fn save_private_key(&self, key: &SecretString) -> Result<(), AuthError>;
    async fn load_private_key(&self) -> Result<Option<SecretString>, AuthError>;
    async fn clear_private_key(&self) -> Result<(), AuthError>;
}

/// Talks directly to GNOME Secret Service over the user's session D-Bus.
#[derive(Debug, Clone, Default)]
pub struct GnomeSecretStore;

#[async_trait]
impl SecretStore for GnomeSecretStore {
    async fn save_private_key(&self, key: &SecretString) -> Result<(), AuthError> {
        use secret_service::{EncryptionType, SecretService};
        let service = SecretService::connect(EncryptionType::Dh)
            .await
            .map_err(|e| AuthError::Keyring(e.to_string()))?;
        let collection = service
            .get_default_collection()
            .await
            .map_err(|e| AuthError::Keyring(e.to_string()))?;
        if collection
            .is_locked()
            .await
            .map_err(|e| AuthError::Keyring(e.to_string()))?
        {
            collection
                .unlock()
                .await
                .map_err(|e| AuthError::Keyring(e.to_string()))?;
        }
        collection
            .create_item(
                "Weatherglass WeatherKit private key",
                attributes(),
                key.expose_secret().as_bytes(),
                true,
                "text/plain",
            )
            .await
            .map_err(|e| AuthError::Keyring(e.to_string()))?;
        Ok(())
    }

    async fn load_private_key(&self) -> Result<Option<SecretString>, AuthError> {
        if let Ok(value) = std::env::var("WEATHERGLASS_WEATHERKIT_PRIVATE_KEY") {
            return Ok(Some(SecretString::from(value)));
        }
        use secret_service::{EncryptionType, SecretService};
        let service = SecretService::connect(EncryptionType::Dh)
            .await
            .map_err(|e| AuthError::Keyring(e.to_string()))?;
        let found = service
            .search_items(attributes())
            .await
            .map_err(|e| AuthError::Keyring(e.to_string()))?;
        let item = if let Some(x) = found.unlocked.first() {
            x
        } else if let Some(x) = found.locked.first() {
            x.unlock()
                .await
                .map_err(|e| AuthError::Keyring(e.to_string()))?;
            x
        } else {
            return Ok(None);
        };
        let bytes = item
            .get_secret()
            .await
            .map_err(|e| AuthError::Keyring(e.to_string()))?;
        let value = String::from_utf8(bytes).map_err(|e| AuthError::Keyring(e.to_string()))?;
        Ok(Some(SecretString::from(value)))
    }

    async fn clear_private_key(&self) -> Result<(), AuthError> {
        use secret_service::{EncryptionType, SecretService};
        let service = SecretService::connect(EncryptionType::Dh)
            .await
            .map_err(|e| AuthError::Keyring(e.to_string()))?;
        let found = service
            .search_items(attributes())
            .await
            .map_err(|e| AuthError::Keyring(e.to_string()))?;
        for item in found.unlocked.iter().chain(found.locked.iter()) {
            item.delete()
                .await
                .map_err(|e| AuthError::Keyring(e.to_string()))?;
        }
        Ok(())
    }
}

fn attributes() -> HashMap<&'static str, &'static str> {
    HashMap::from([
        ("application", "io.github.weatherglass.Weatherglass"),
        ("credential", "weatherkit-private-key"),
    ])
}

pub struct LocalJwtProvider<S> {
    pub metadata: CredentialMetadata,
    pub secrets: S,
}

#[derive(Serialize)]
struct Header<'a> {
    alg: &'static str,
    kid: &'a str,
    id: String,
}
#[derive(Serialize)]
struct Claims<'a> {
    iss: &'a str,
    iat: i64,
    exp: i64,
    sub: &'a str,
}

impl<S: SecretStore> LocalJwtProvider<S> {
    pub async fn create_token(&self, now: i64) -> Result<String, AuthError> {
        let m = &self.metadata;
        if m.team_id.trim().is_empty()
            || m.key_id.trim().is_empty()
            || m.service_id.trim().is_empty()
        {
            return Err(AuthError::Incomplete);
        }
        let secret = self
            .secrets
            .load_private_key()
            .await?
            .ok_or(AuthError::Incomplete)?;
        sign_token(m, secret.expose_secret(), now)
    }
}

#[async_trait]
impl<S: SecretStore> TokenProvider for LocalJwtProvider<S> {
    async fn token(&self) -> Result<String, AuthError> {
        self.create_token(Utc::now().timestamp()).await
    }
}

pub fn sign_token(metadata: &CredentialMetadata, pem: &str, now: i64) -> Result<String, AuthError> {
    let header = Header {
        alg: "ES256",
        kid: &metadata.key_id,
        id: format!("{}.{}", metadata.team_id, metadata.service_id),
    };
    let claims = Claims {
        iss: &metadata.team_id,
        iat: now,
        exp: now + 900,
        sub: &metadata.service_id,
    };
    fn encode<T: Serialize>(value: &T) -> Result<String, AuthError> {
        serde_json::to_vec(value)
            .map(|b| URL_SAFE_NO_PAD.encode(b))
            .map_err(|e| AuthError::Token(e.to_string()))
    }
    let signing_input = format!("{}.{}", encode(&header)?, encode(&claims)?);
    let key = SigningKey::from_pkcs8_pem(pem).map_err(|_| AuthError::InvalidKey)?;
    let signature: Signature = key.sign(signing_input.as_bytes());
    Ok(format!(
        "{}.{}",
        signing_input,
        URL_SAFE_NO_PAD.encode(signature.to_bytes())
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use p256::pkcs8::{EncodePrivateKey, LineEnding};
    #[test]
    fn jwt_has_only_documented_claims_and_no_secret() {
        let key = SigningKey::random(&mut p256::elliptic_curve::rand_core::OsRng);
        let pem = key.to_pkcs8_pem(LineEnding::LF).unwrap().to_string();
        let meta = CredentialMetadata {
            team_id: "TEAM123456".into(),
            key_id: "KEY1234567".into(),
            service_id: "io.test.weather".into(),
        };
        let token = sign_token(&meta, &pem, 1_700_000_000).unwrap();
        let parts: Vec<_> = token.split('.').collect();
        assert_eq!(parts.len(), 3);
        let payload: serde_json::Value =
            serde_json::from_slice(&URL_SAFE_NO_PAD.decode(parts[1]).unwrap()).unwrap();
        assert_eq!(payload.as_object().unwrap().len(), 4);
        assert_eq!(payload["exp"], 1_700_000_900i64);
        assert!(!token.contains("PRIVATE"));
    }
}
