use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use jsonwebtoken::jwk::JwkSet;
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use tokio::sync::{OnceCell, RwLock};
use zeroize::Zeroizing;

use crate::application::port::TaskBackend;
use crate::application::port::auth::{AuthError, AuthProvider, AuthResult};
use crate::domain::duration::parse_duration;
use crate::domain::user::{CreateUserParams, hash_api_key};
use crate::infra::config::SessionConfig;

fn constant_time_key_eq(a: &str, b: &str) -> bool {
    let hash_a = Sha256::digest(a.as_bytes());
    let hash_b = Sha256::digest(b.as_bytes());
    hash_a.ct_eq(&hash_b).into()
}

pub struct ApiKeyProvider {
    backend: Arc<dyn TaskBackend>,
    master_api_key: Option<Zeroizing<String>>,
    session_config: SessionConfig,
}

impl ApiKeyProvider {
    pub fn new(
        backend: Arc<dyn TaskBackend>,
        master_api_key: Option<String>,
        session_config: SessionConfig,
    ) -> Self {
        Self {
            backend,
            master_api_key: master_api_key.map(Zeroizing::new),
            session_config,
        }
    }
}

#[async_trait]
impl AuthProvider for ApiKeyProvider {
    async fn authenticate(&self, token: &str) -> std::result::Result<AuthResult, AuthError> {
        if let Some(ref master_key) = self.master_api_key
            && constant_time_key_eq(token, master_key)
        {
            return Ok(AuthResult {
                user: crate::domain::user::User::new(
                    0,
                    "master".to_string(),
                    "master".to_string(),
                    None,
                    None,
                    String::new(),
                ),
                is_master: true,
            });
        }

        let key_hash = hash_api_key(token);
        let auth_result = self
            .backend
            .get_user_by_api_key(&key_hash)
            .await
            .map_err(|_| AuthError::InvalidToken)?;

        // Check absolute TTL
        if let Some(ref ttl_str) = self.session_config.ttl
            && let Ok(ttl) = parse_duration(ttl_str)
            && let Ok(created) = chrono::DateTime::parse_from_rfc3339(&auth_result.key_created_at)
        {
            let elapsed = chrono::Utc::now().signed_duration_since(created);
            if elapsed > chrono::Duration::from_std(ttl).unwrap_or(chrono::Duration::MAX) {
                tracing::debug!(key_created_at = %auth_result.key_created_at, "API key expired (TTL)");
                return Err(AuthError::InvalidToken);
            }
        }

        // Check inactive TTL
        if let Some(ref inactive_ttl_str) = self.session_config.inactive_ttl
            && let Ok(inactive_ttl) = parse_duration(inactive_ttl_str)
            && let Some(ref last_used) = auth_result.key_last_used_at
            && let Ok(last) = chrono::DateTime::parse_from_rfc3339(last_used)
        {
            let elapsed = chrono::Utc::now().signed_duration_since(last);
            if elapsed > chrono::Duration::from_std(inactive_ttl).unwrap_or(chrono::Duration::MAX) {
                tracing::debug!(last_used_at = %last_used, "API key expired (inactive TTL)");
                return Err(AuthError::InvalidToken);
            }
        }

        Ok(AuthResult {
            user: auth_result.user,
            is_master: false,
        })
    }
}

// --- OIDC JWT Authentication ---

const JWKS_CACHE_TTL: Duration = Duration::from_secs(300); // 5 minutes
const JWKS_FORCE_REFRESH_COOLDOWN: Duration = Duration::from_secs(30);

#[derive(serde::Deserialize)]
struct OidcDiscoveryDocument {
    jwks_uri: String,
}

struct JwksCache {
    keys: JwkSet,
    fetched_at: Instant,
}

pub struct JwtAuthProvider {
    http_client: reqwest::Client,
    issuer_url: String,
    client_id: String,
    username_claim: Option<String>,
    required_claims: HashMap<String, String>,
    groups_claim: String,
    master_group: Option<String>,
    jwks_uri: OnceCell<String>,
    jwks_cache: RwLock<Option<JwksCache>>,
    last_force_refresh: RwLock<Option<Instant>>,
    backend: Arc<dyn TaskBackend>,
}

impl JwtAuthProvider {
    pub fn new(
        issuer_url: String,
        client_id: String,
        username_claim: Option<String>,
        required_claims: HashMap<String, String>,
        groups_claim: String,
        master_group: Option<String>,
        backend: Arc<dyn TaskBackend>,
    ) -> Self {
        Self {
            http_client: reqwest::Client::new(),
            issuer_url,
            client_id,
            username_claim,
            required_claims,
            groups_claim,
            master_group,
            jwks_uri: OnceCell::new(),
            jwks_cache: RwLock::new(None),
            last_force_refresh: RwLock::new(None),
            backend,
        }
    }

    async fn discover_jwks_uri(&self) -> Result<String, AuthError> {
        let discovery_url = format!(
            "{}/.well-known/openid-configuration",
            self.issuer_url.trim_end_matches('/')
        );
        let doc: OidcDiscoveryDocument = self
            .http_client
            .get(&discovery_url)
            .send()
            .await
            .map_err(|e| {
                tracing::warn!(url = %discovery_url, error = %e, "OIDC discovery request failed");
                AuthError::InvalidToken
            })?
            .json()
            .await
            .map_err(|e| {
                tracing::warn!(url = %discovery_url, error = %e, "OIDC discovery response parse failed");
                AuthError::InvalidToken
            })?;
        Ok(doc.jwks_uri)
    }

    async fn get_jwks_uri(&self) -> Result<&str, AuthError> {
        self.jwks_uri
            .get_or_try_init(|| self.discover_jwks_uri())
            .await
            .map(|s| s.as_str())
    }

    async fn fetch_jwks(&self) -> Result<JwkSet, AuthError> {
        let jwks_uri = self.get_jwks_uri().await?;
        self.http_client
            .get(jwks_uri)
            .send()
            .await
            .map_err(|e| {
                tracing::warn!(url = %jwks_uri, error = %e, "JWKS fetch failed");
                AuthError::InvalidToken
            })?
            .json()
            .await
            .map_err(|e| {
                tracing::warn!(url = %jwks_uri, error = %e, "JWKS parse failed");
                AuthError::InvalidToken
            })
    }

    async fn get_jwks(&self) -> Result<JwkSet, AuthError> {
        // Try read lock first (fast path)
        {
            let cache = self.jwks_cache.read().await;
            if let Some(ref c) = *cache
                && c.fetched_at.elapsed() < JWKS_CACHE_TTL
            {
                return Ok(c.keys.clone());
            }
        }
        // Cache miss or expired — fetch and update
        self.refresh_jwks().await
    }

    async fn refresh_jwks(&self) -> Result<JwkSet, AuthError> {
        let keys = self.fetch_jwks().await?;
        let mut cache = self.jwks_cache.write().await;
        *cache = Some(JwksCache {
            keys: keys.clone(),
            fetched_at: Instant::now(),
        });
        Ok(keys)
    }

    async fn force_refresh_jwks(&self) -> Result<JwkSet, AuthError> {
        // Rate-limit forced refreshes
        {
            let last = self.last_force_refresh.read().await;
            if let Some(t) = *last
                && t.elapsed() < JWKS_FORCE_REFRESH_COOLDOWN
            {
                tracing::debug!("JWKS force refresh rate-limited");
                // Return current cache if available
                let cache = self.jwks_cache.read().await;
                if let Some(ref c) = *cache {
                    return Ok(c.keys.clone());
                }
                return Err(AuthError::InvalidToken);
            }
        }
        let keys = self.refresh_jwks().await?;
        let mut last = self.last_force_refresh.write().await;
        *last = Some(Instant::now());
        Ok(keys)
    }

    async fn verify_jwt(
        &self,
        token: &str,
    ) -> Result<jsonwebtoken::TokenData<serde_json::Value>, AuthError> {
        let header = jsonwebtoken::decode_header(token).map_err(|_| AuthError::InvalidToken)?;

        let algorithm = header.alg;
        let kid = header.kid.as_deref();

        // Try with cached JWKS first
        let jwks = self.get_jwks().await?;
        match self.try_verify_with_jwks(token, &jwks, kid, algorithm) {
            Ok(data) => Ok(data),
            Err(_) if kid.is_some() => {
                // Key not found or verification failed — force refresh JWKS (key rotation)
                tracing::debug!(kid = ?kid, "JWT verification failed, refreshing JWKS");
                let jwks = self.force_refresh_jwks().await?;
                self.try_verify_with_jwks(token, &jwks, kid, algorithm)
            }
            Err(e) => Err(e),
        }
    }

    fn try_verify_with_jwks(
        &self,
        token: &str,
        jwks: &JwkSet,
        kid: Option<&str>,
        algorithm: jsonwebtoken::Algorithm,
    ) -> Result<jsonwebtoken::TokenData<serde_json::Value>, AuthError> {
        let jwk = if let Some(kid) = kid {
            jwks.find(kid).ok_or(AuthError::InvalidToken)?
        } else {
            // No kid — use first key
            jwks.keys.first().ok_or(AuthError::InvalidToken)?
        };

        let decoding_key = jsonwebtoken::DecodingKey::from_jwk(jwk).map_err(|e| {
            tracing::warn!(error = %e, "failed to create decoding key from JWK");
            AuthError::InvalidToken
        })?;

        let mut validation = jsonwebtoken::Validation::new(algorithm);
        validation.set_issuer(&[&self.issuer_url]);
        validation.set_audience(&[&self.client_id]);

        jsonwebtoken::decode::<serde_json::Value>(token, &decoding_key, &validation).map_err(|e| {
            tracing::debug!(error = %e, "JWT validation failed");
            AuthError::InvalidToken
        })
    }
}

#[async_trait]
impl AuthProvider for JwtAuthProvider {
    async fn authenticate(&self, token: &str) -> std::result::Result<AuthResult, AuthError> {
        let token_data = self.verify_jwt(token).await?;

        // Validate required claims (all conditions must be satisfied)
        for (claim_name, expected_value) in &self.required_claims {
            match token_data.claims.get(claim_name.as_str()) {
                None => {
                    tracing::debug!(claim = %claim_name, "required claim missing from JWT");
                    return Err(AuthError::InvalidToken);
                }
                Some(serde_json::Value::String(s)) => {
                    if s != expected_value {
                        tracing::debug!(claim = %claim_name, expected = %expected_value, actual = %s, "required claim value mismatch");
                        return Err(AuthError::InvalidToken);
                    }
                }
                Some(serde_json::Value::Array(arr)) => {
                    if !arr
                        .iter()
                        .any(|v| v.as_str() == Some(expected_value.as_str()))
                    {
                        tracing::debug!(claim = %claim_name, expected = %expected_value, "required claim value not found in array");
                        return Err(AuthError::InvalidToken);
                    }
                }
                Some(_) => {
                    tracing::debug!(claim = %claim_name, "required claim has unsupported type");
                    return Err(AuthError::InvalidToken);
                }
            }
        }

        // Extract sub claim (required for user identity)
        let sub = token_data
            .claims
            .get("sub")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                tracing::debug!("sub claim missing from JWT");
                AuthError::InvalidToken
            })?;

        let username = if let Some(ref claim_name) = self.username_claim {
            token_data
                .claims
                .get(claim_name.as_str())
                .and_then(|v| v.as_str())
                .ok_or_else(|| {
                    tracing::debug!(claim = %claim_name, "configured username_claim not found in token");
                    AuthError::InvalidToken
                })?
        } else {
            token_data
                .claims
                .get("preferred_username")
                .and_then(|v| v.as_str())
                .or_else(|| token_data.claims.get("email").and_then(|v| v.as_str()))
                .unwrap_or(sub)
        };

        let is_master = self.master_group.as_deref().is_some_and(|group| {
            token_data
                .claims
                .get(self.groups_claim.as_str())
                .and_then(|v| v.as_array())
                .is_some_and(|arr| arr.iter().any(|v| v.as_str() == Some(group)))
        });

        // Try to find existing user by sub; auto-create if not found (standard OIDC provisioning)
        let user = match self.backend.get_user_by_sub(sub).await {
            Ok(user) => user,
            Err(_) => {
                let display_name = token_data
                    .claims
                    .get("name")
                    .and_then(|v| v.as_str())
                    .map(String::from);
                let email = token_data
                    .claims
                    .get("email")
                    .and_then(|v| v.as_str())
                    .map(String::from);
                tracing::info!(sub = %sub, username = %username, "auto-provisioning user from OIDC claims");
                self.backend
                    .create_user(&CreateUserParams {
                        username: username.to_string(),
                        sub: Some(sub.to_string()),
                        display_name,
                        email,
                    })
                    .await
                    .map_err(|e| {
                        tracing::warn!(error = %e, "failed to auto-provision OIDC user");
                        AuthError::InvalidToken
                    })?
            }
        };

        Ok(AuthResult { user, is_master })
    }
}

// --- Trusted Headers Auth ---

pub struct TrustedHeadersAuthResult {
    pub user: crate::domain::user::User,
    pub groups: Vec<String>,
    pub scopes: Vec<String>,
    pub is_master: bool,
}

pub struct TrustedHeadersAuthProvider {
    backend: Arc<dyn TaskBackend>,
    subject_header: String,
    name_header: Option<String>,
    display_name_header: Option<String>,
    email_header: Option<String>,
    groups_header: Option<String>,
    scope_header: Option<String>,
    master_group: Option<String>,
}

impl TrustedHeadersAuthProvider {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        backend: Arc<dyn TaskBackend>,
        subject_header: String,
        name_header: Option<String>,
        display_name_header: Option<String>,
        email_header: Option<String>,
        groups_header: Option<String>,
        scope_header: Option<String>,
        master_group: Option<String>,
    ) -> Self {
        Self {
            backend,
            subject_header,
            name_header,
            display_name_header,
            email_header,
            groups_header,
            scope_header,
            master_group,
        }
    }

    pub async fn authenticate_from_headers(
        &self,
        headers: &axum::http::HeaderMap,
    ) -> std::result::Result<TrustedHeadersAuthResult, AuthError> {
        let sub = headers
            .get(&self.subject_header)
            .and_then(|v: &axum::http::HeaderValue| v.to_str().ok())
            .ok_or(AuthError::MissingToken)?;

        let name_value = self
            .name_header
            .as_ref()
            .and_then(|h| headers.get(h.as_str()))
            .and_then(|v: &axum::http::HeaderValue| v.to_str().ok())
            .map(String::from);

        let display_name_value = self
            .display_name_header
            .as_ref()
            .and_then(|h| headers.get(h.as_str()))
            .and_then(|v: &axum::http::HeaderValue| v.to_str().ok())
            .map(String::from);

        let email = self
            .email_header
            .as_ref()
            .and_then(|h| headers.get(h.as_str()))
            .and_then(|v: &axum::http::HeaderValue| v.to_str().ok())
            .map(String::from);

        // Username fallback: name_header -> email -> sub
        let username = name_value
            .as_deref()
            .or(email.as_deref())
            .unwrap_or(sub)
            .to_string();

        // Display name fallback: display_name_header -> name_header -> email
        let display_name = display_name_value
            .or_else(|| name_value.clone())
            .or_else(|| email.clone());

        let groups: Vec<String> = self
            .groups_header
            .as_ref()
            .and_then(|h| headers.get(h.as_str()))
            .and_then(|v: &axum::http::HeaderValue| v.to_str().ok())
            .map(|s| {
                s.split(',')
                    .map(|g| g.trim().to_string())
                    .filter(|g| !g.is_empty())
                    .collect()
            })
            .unwrap_or_default();

        let scopes: Vec<String> = self
            .scope_header
            .as_ref()
            .and_then(|h| headers.get(h.as_str()))
            .and_then(|v: &axum::http::HeaderValue| v.to_str().ok())
            .map(|s| {
                s.split(',')
                    .map(|g| g.trim().to_string())
                    .filter(|g| !g.is_empty())
                    .collect()
            })
            .unwrap_or_default();

        let user = match self.backend.get_user_by_sub(sub).await {
            Ok(user) => user,
            Err(_) => {
                tracing::info!(sub = %sub, username = %username, "auto-provisioning user from trusted headers");
                self.backend
                    .create_user(&CreateUserParams {
                        username,
                        sub: Some(sub.to_string()),
                        display_name,
                        email,
                    })
                    .await
                    .map_err(|e| {
                        tracing::warn!(error = %e, "failed to auto-provision trusted-headers user");
                        AuthError::InvalidToken
                    })?
            }
        };

        let is_master = self
            .master_group
            .as_deref()
            .is_some_and(|mg| groups.iter().any(|g| g == mg));

        Ok(TrustedHeadersAuthResult {
            user,
            groups,
            scopes,
            is_master,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::user::{ApiKeyRepository, CreateUserParams, NewApiKey, UserRepository};
    use crate::infra::sqlite::SqliteBackend;

    async fn setup_backend_with_api_key() -> (Arc<SqliteBackend>, String) {
        let backend = SqliteBackend::new_in_memory().unwrap();
        let user = backend
            .create_user(&CreateUserParams {
                username: "testuser".to_string(),
                sub: None,
                display_name: None,
                email: None,
            })
            .await
            .unwrap();
        let new_key = NewApiKey::generate();
        let raw_key = new_key.raw_key.clone();
        backend
            .create_api_key(user.id(), "test-key", None, &new_key)
            .await
            .unwrap();
        (Arc::new(backend), raw_key)
    }

    #[tokio::test]
    async fn master_key_match() {
        let backend = Arc::new(SqliteBackend::new_in_memory().unwrap());
        let provider = ApiKeyProvider::new(
            backend,
            Some("master-secret".to_string()),
            SessionConfig::default(),
        );

        let result = provider.authenticate("master-secret").await.unwrap();

        assert_eq!(result.user.id(), 0);
        assert_eq!(result.user.username(), "master");
        assert!(result.is_master);
    }

    #[tokio::test]
    async fn master_key_mismatch_valid_user_key() {
        let (backend, raw_key) = setup_backend_with_api_key().await;
        let provider = ApiKeyProvider::new(
            backend,
            Some("master-secret".to_string()),
            SessionConfig::default(),
        );

        let result = provider.authenticate(&raw_key).await.unwrap();

        assert_eq!(result.user.username(), "testuser");
        assert!(!result.is_master);
    }

    #[tokio::test]
    async fn master_key_mismatch_invalid() {
        let backend = Arc::new(SqliteBackend::new_in_memory().unwrap());
        let provider = ApiKeyProvider::new(
            backend,
            Some("master-secret".to_string()),
            SessionConfig::default(),
        );

        let result = provider.authenticate("wrong-key").await;

        assert!(matches!(result, Err(AuthError::InvalidToken)));
    }

    #[tokio::test]
    async fn no_master_key_valid_user_key() {
        let (backend, raw_key) = setup_backend_with_api_key().await;
        let provider = ApiKeyProvider::new(backend, None, SessionConfig::default());

        let result = provider.authenticate(&raw_key).await.unwrap();

        assert_eq!(result.user.username(), "testuser");
        assert!(!result.is_master);
    }

    #[tokio::test]
    async fn no_master_key_invalid() {
        let backend = Arc::new(SqliteBackend::new_in_memory().unwrap());
        let provider = ApiKeyProvider::new(backend, None, SessionConfig::default());

        let result = provider.authenticate("garbage").await;

        assert!(matches!(result, Err(AuthError::InvalidToken)));
    }

    // --- TrustedHeadersAuthProvider tests ---

    #[tokio::test]
    async fn trusted_headers_valid_subject() {
        let backend = Arc::new(SqliteBackend::new_in_memory().unwrap());
        let provider = TrustedHeadersAuthProvider::new(
            backend,
            "x-senko-user-sub".to_string(),
            None,
            None,
            None,
            None,
            None,
            None,
        );

        let mut headers = axum::http::HeaderMap::new();
        headers.insert("x-senko-user-sub", "alice".parse().unwrap());

        let result = provider.authenticate_from_headers(&headers).await.unwrap();
        assert_eq!(result.user.username(), "alice");
        assert!(result.groups.is_empty());
        assert!(result.scopes.is_empty());
        assert!(!result.is_master);
    }

    #[tokio::test]
    async fn trusted_headers_missing_subject() {
        let backend = Arc::new(SqliteBackend::new_in_memory().unwrap());
        let provider = TrustedHeadersAuthProvider::new(
            backend,
            "x-senko-user-sub".to_string(),
            None,
            None,
            None,
            None,
            None,
            None,
        );

        let headers = axum::http::HeaderMap::new();
        let result = provider.authenticate_from_headers(&headers).await;
        assert!(matches!(result, Err(AuthError::MissingToken)));
    }

    #[tokio::test]
    async fn trusted_headers_with_name_and_email() {
        let backend = Arc::new(SqliteBackend::new_in_memory().unwrap());
        let provider = TrustedHeadersAuthProvider::new(
            backend,
            "x-senko-user-sub".to_string(),
            Some("x-senko-user-name".to_string()),
            None,
            Some("x-senko-user-email".to_string()),
            None,
            None,
            None,
        );

        let mut headers = axum::http::HeaderMap::new();
        headers.insert("x-senko-user-sub", "bob".parse().unwrap());
        headers.insert("x-senko-user-name", "Bob Smith".parse().unwrap());
        headers.insert("x-senko-user-email", "bob@example.com".parse().unwrap());

        let result = provider.authenticate_from_headers(&headers).await.unwrap();
        assert_eq!(result.user.username(), "Bob Smith");
        assert_eq!(result.user.sub(), "bob");
    }

    #[tokio::test]
    async fn trusted_headers_existing_user_returned() {
        let backend: Arc<dyn TaskBackend> = Arc::new(SqliteBackend::new_in_memory().unwrap());
        backend
            .create_user(&CreateUserParams {
                username: "existing".to_string(),
                sub: None,
                display_name: Some("Existing User".to_string()),
                email: None,
            })
            .await
            .unwrap();

        let provider = TrustedHeadersAuthProvider::new(
            backend,
            "x-senko-user-sub".to_string(),
            None,
            None,
            None,
            None,
            None,
            None,
        );

        let mut headers = axum::http::HeaderMap::new();
        headers.insert("x-senko-user-sub", "existing".parse().unwrap());

        let result = provider.authenticate_from_headers(&headers).await.unwrap();
        assert_eq!(result.user.username(), "existing");
    }

    #[tokio::test]
    async fn trusted_headers_with_groups_and_scope() {
        let backend = Arc::new(SqliteBackend::new_in_memory().unwrap());
        let provider = TrustedHeadersAuthProvider::new(
            backend,
            "x-senko-user-sub".to_string(),
            None,
            None,
            None,
            Some("x-senko-user-groups".to_string()),
            Some("x-senko-user-scope".to_string()),
            None,
        );

        let mut headers = axum::http::HeaderMap::new();
        headers.insert("x-senko-user-sub", "carol".parse().unwrap());
        headers.insert("x-senko-user-groups", "admin, dev, ops".parse().unwrap());
        headers.insert("x-senko-user-scope", "read,write".parse().unwrap());

        let result = provider.authenticate_from_headers(&headers).await.unwrap();
        assert_eq!(result.user.username(), "carol");
        assert_eq!(result.groups, vec!["admin", "dev", "ops"]);
        assert_eq!(result.scopes, vec!["read", "write"]);
        assert!(!result.is_master);
    }

    #[tokio::test]
    async fn trusted_headers_master_group_sets_is_master() {
        let backend = Arc::new(SqliteBackend::new_in_memory().unwrap());
        let provider = TrustedHeadersAuthProvider::new(
            backend,
            "x-senko-user-sub".to_string(),
            None,
            None,
            None,
            Some("x-senko-user-groups".to_string()),
            None,
            Some("admin".to_string()),
        );

        let mut headers = axum::http::HeaderMap::new();
        headers.insert("x-senko-user-sub", "erin".parse().unwrap());
        headers.insert("x-senko-user-groups", "admin, dev".parse().unwrap());

        let result = provider.authenticate_from_headers(&headers).await.unwrap();
        assert!(result.is_master);
    }

    #[tokio::test]
    async fn trusted_headers_master_group_absent_stays_false() {
        let backend = Arc::new(SqliteBackend::new_in_memory().unwrap());
        let provider = TrustedHeadersAuthProvider::new(
            backend,
            "x-senko-user-sub".to_string(),
            None,
            None,
            None,
            Some("x-senko-user-groups".to_string()),
            None,
            Some("admin".to_string()),
        );

        let mut headers = axum::http::HeaderMap::new();
        headers.insert("x-senko-user-sub", "frank".parse().unwrap());
        headers.insert("x-senko-user-groups", "dev, ops".parse().unwrap());

        let result = provider.authenticate_from_headers(&headers).await.unwrap();
        assert!(!result.is_master);
    }

    #[tokio::test]
    async fn trusted_headers_empty_groups_and_scope() {
        let backend = Arc::new(SqliteBackend::new_in_memory().unwrap());
        let provider = TrustedHeadersAuthProvider::new(
            backend,
            "x-senko-user-sub".to_string(),
            None,
            None,
            None,
            Some("x-senko-user-groups".to_string()),
            Some("x-senko-user-scope".to_string()),
            None,
        );

        let mut headers = axum::http::HeaderMap::new();
        headers.insert("x-senko-user-sub", "dave".parse().unwrap());

        let result = provider.authenticate_from_headers(&headers).await.unwrap();
        assert_eq!(result.user.username(), "dave");
        assert!(result.groups.is_empty());
        assert!(result.scopes.is_empty());
    }

    // --- JwtAuthProvider tests ---

    // Pre-generated 2048-bit RSA test key pair (for testing only, never use in production)
    const TEST_RSA_PRIVATE_KEY_PEM: &str = "-----BEGIN PRIVATE KEY-----\n\
        MIIEvQIBADANBgkqhkiG9w0BAQEFAASCBKcwggSjAgEAAoIBAQDUiMV/eBBUNjyt\n\
        D6pa3sdXK+z1CzP9X/hvipatxNUEksaHoNsuuLd3iQYwLQlpywdjqVeuBMnbs0qs\n\
        Uzc9HfdHKGcspayLKI+itsKPKATmySFbi1nCNToQUHSfw0gkqS1aD/1qiANccodq\n\
        qWC5U4P3PyawS3JOfNH02EDwQYER2ZZLZsJk4velJvxLL6Lr5QX3WtlJxHgZSbid\n\
        2tDR21q8B1dCtFNfheB4SehXv+V2a1ldIwlungQBV6WpgRNTdlu5IGKh/IR/9BvK\n\
        3d958O65fQolT+4J0rSumLFWOW9Y+2/vPUxmdgzyyRTg68ZIGxARTlsFwG3LFF6d\n\
        3N1dwMS7AgMBAAECggEACY9tcuJvuZoG+LHvs865oM41BoDOgeNDRaEyTfbArkf7\n\
        7jXqJhvhBNuBD8G23q9nUbBYZVeJzPwvq7jCj1k9ulGy8msxa8ETVPprngzqy4bY\n\
        nUmTbA0A46L63AToxd1mUNrPR29+1zW/qaic1TlQglqw3tVF+wnaV+0yXpxTtf3C\n\
        nFd9lf9r2/upnHOR9if5VQ5voRevF9K+077DDYm7CtdkFg9rvw4kkKvHNz18XNyE\n\
        YSvl5AU4MB5yEoncp+Ghwqj4aG9o+P1XMejwR2hhIxnOnuq33RCGFirQH4BgnjVw\n\
        6pjDc8zElfhxfWUPVNoNKqSI0IerFFHyREoLYJb3wQKBgQD8Ow2xrDaV7VLJwRcC\n\
        Ez8eY4gqlNrfoshzCMG+T2KSO6EM88XcAicpCSn3uxWTxU9KTPV2B+4nEMEe+/+Q\n\
        TOrStbtktfeg7eZQvM6q3qzQdEVwgGXXbFX3CJ8Ysm2HrUpCODj5/dwmPNDkw62Z\n\
        z776NjfoDb+PrYWMthHyL41+OQKBgQDXtdp2BQD5kEWkzx3592T+adAxAWPZHRNH\n\
        GlgMOzI4TMekfo/UPZjB9Sx0WCz1cGzidIoNrUbxqjGaZDhKQWWH8iMTVfpYmBr6\n\
        A0eiVyCkC/7U+pg3+uRTeL2nBxio7B8EuU1viZSoS+/l99rEcfy8LZw6QfKo+KY3\n\
        I4KyriuakwKBgAJ8eogT0H3t1vESLC3jDq44APGaggXOTveDUJWVpr0WRWIhTQP8\n\
        KXKoGnfMqkvImB19YLYHIfvUmHK7vSso9u+Yxv4ZJRW7ApgtJERe6YksfDq9qUNU\n\
        WAyVUywlJhs+RAsfDsC4FeFynASFQULQ32sL+cUZzZeW+EgIy2h9u4FRAoGAdYYU\n\
        whwzzcR2zTYyxM+u7JXF4g050z5uFF0b/334/IeIdeymfCIbKgFj+PdZd1eLW03X\n\
        MWBouJ3bbJyRtpMuuASKa6x6Ou6UNAa5bo89r2MBshPd/xHoeDneSjQpkU8kDzTO\n\
        Jai1n4PP7mE9ha383qGS7oKjrL/b/0qPmL4f75UCgYEA65iJ3jKWAGu6KSsfp/Ni\n\
        Hqo4+L8XTsRKjX+FAwVhU5Rox8HVCGKk/uc+gAlAnNFmRQ4jVnNKY3svCxb0rfkP\n\
        THa2UtBwsprZSRxuH03X/Q4hhGpAoK4k3po5unRHyWeoDiLAfe+q40WEElA+1VMX\n\
        Rgu+XI6p9qsqbKuqo8oflCg=\n\
        -----END PRIVATE KEY-----";

    // Corresponding JWK public key components (base64url-encoded)
    const TEST_JWK_N: &str = "1IjFf3gQVDY8rQ-qWt7HVyvs9Qsz_V_4b4qWrcTVBJLGh6DbLri3d4kGMC0JacsHY6lXrgTJ27NKrFM3PR33RyhnLKWsiyiPorbCjygE5skhW4tZwjU6EFB0n8NIJKktWg_9aogDXHKHaqlguVOD9z8msEtyTnzR9NhA8EGBEdmWS2bCZOL3pSb8Sy-i6-UF91rZScR4GUm4ndrQ0dtavAdXQrRTX4XgeEnoV7_ldmtZXSMJbp4EAVelqYETU3ZbuSBiofyEf_Qbyt3fefDuuX0KJU_uCdK0rpixVjlvWPtv7z1MZnYM8skU4OvGSBsQEU5bBcBtyxRendzdXcDEuw";
    const TEST_JWK_E: &str = "AQAB";

    fn make_test_jwk() -> jsonwebtoken::jwk::Jwk {
        use jsonwebtoken::jwk::{CommonParameters, Jwk, KeyAlgorithm, RSAKeyParameters};
        Jwk {
            common: CommonParameters {
                public_key_use: Some(jsonwebtoken::jwk::PublicKeyUse::Signature),
                key_operations: None,
                key_algorithm: Some(KeyAlgorithm::RS256),
                key_id: Some("test-kid".to_string()),
                x509_url: None,
                x509_chain: None,
                x509_sha1_fingerprint: None,
                x509_sha256_fingerprint: None,
            },
            algorithm: jsonwebtoken::jwk::AlgorithmParameters::RSA(RSAKeyParameters {
                key_type: jsonwebtoken::jwk::RSAKeyType::RSA,
                n: TEST_JWK_N.to_string(),
                e: TEST_JWK_E.to_string(),
            }),
        }
    }

    fn make_test_encoding_key() -> jsonwebtoken::EncodingKey {
        jsonwebtoken::EncodingKey::from_rsa_pem(TEST_RSA_PRIVATE_KEY_PEM.as_bytes()).unwrap()
    }

    fn make_jwt(encoding_key: &jsonwebtoken::EncodingKey, claims: &serde_json::Value) -> String {
        let mut header = jsonwebtoken::Header::new(jsonwebtoken::Algorithm::RS256);
        header.kid = Some("test-kid".to_string());
        jsonwebtoken::encode(&header, claims, encoding_key).unwrap()
    }

    #[tokio::test]
    async fn jwt_provider_verify_valid_token() {
        let encoding_key = make_test_encoding_key();
        let jwk = make_test_jwk();
        let backend = Arc::new(SqliteBackend::new_in_memory().unwrap());
        let user = backend
            .create_user(&CreateUserParams {
                username: "jwt-user".to_string(),
                sub: None,
                display_name: None,
                email: None,
            })
            .await
            .unwrap();
        let _ = user;

        let provider = JwtAuthProvider::new(
            "https://issuer.example.com".to_string(),
            "test-client-id".to_string(),
            None,
            HashMap::new(),
            "groups".to_string(),
            None,
            backend,
        );

        // Pre-populate JWKS cache
        {
            let mut cache = provider.jwks_cache.write().await;
            *cache = Some(JwksCache {
                keys: JwkSet { keys: vec![jwk] },
                fetched_at: Instant::now(),
            });
        }

        let claims = serde_json::json!({
            "sub": "jwt-user",
            "iss": "https://issuer.example.com",
            "aud": "test-client-id",
            "exp": (chrono::Utc::now().timestamp() + 3600),
            "iat": chrono::Utc::now().timestamp(),
        });
        let token = make_jwt(&encoding_key, &claims);

        let user = provider.authenticate(&token).await.unwrap().user;
        assert_eq!(user.username(), "jwt-user");
    }

    #[tokio::test]
    async fn jwt_provider_rejects_wrong_issuer() {
        let encoding_key = make_test_encoding_key();
        let jwk = make_test_jwk();
        let backend = Arc::new(SqliteBackend::new_in_memory().unwrap());

        let provider = JwtAuthProvider::new(
            "https://issuer.example.com".to_string(),
            "test-client-id".to_string(),
            None,
            HashMap::new(),
            "groups".to_string(),
            None,
            backend,
        );

        {
            let mut cache = provider.jwks_cache.write().await;
            *cache = Some(JwksCache {
                keys: JwkSet { keys: vec![jwk] },
                fetched_at: Instant::now(),
            });
        }

        let claims = serde_json::json!({
            "sub": "jwt-user",
            "iss": "https://wrong-issuer.example.com",
            "aud": "test-client-id",
            "exp": (chrono::Utc::now().timestamp() + 3600),
        });
        let token = make_jwt(&encoding_key, &claims);

        let result = provider.authenticate(&token).await;
        assert!(matches!(result, Err(AuthError::InvalidToken)));
    }

    #[tokio::test]
    async fn jwt_provider_rejects_wrong_audience() {
        let encoding_key = make_test_encoding_key();
        let jwk = make_test_jwk();
        let backend = Arc::new(SqliteBackend::new_in_memory().unwrap());

        let provider = JwtAuthProvider::new(
            "https://issuer.example.com".to_string(),
            "test-client-id".to_string(),
            None,
            HashMap::new(),
            "groups".to_string(),
            None,
            backend,
        );

        {
            let mut cache = provider.jwks_cache.write().await;
            *cache = Some(JwksCache {
                keys: JwkSet { keys: vec![jwk] },
                fetched_at: Instant::now(),
            });
        }

        let claims = serde_json::json!({
            "sub": "jwt-user",
            "iss": "https://issuer.example.com",
            "aud": "wrong-client-id",
            "exp": (chrono::Utc::now().timestamp() + 3600),
        });
        let token = make_jwt(&encoding_key, &claims);

        let result = provider.authenticate(&token).await;
        assert!(matches!(result, Err(AuthError::InvalidToken)));
    }

    #[tokio::test]
    async fn jwt_provider_rejects_expired_token() {
        let encoding_key = make_test_encoding_key();
        let jwk = make_test_jwk();
        let backend = Arc::new(SqliteBackend::new_in_memory().unwrap());

        let provider = JwtAuthProvider::new(
            "https://issuer.example.com".to_string(),
            "test-client-id".to_string(),
            None,
            HashMap::new(),
            "groups".to_string(),
            None,
            backend,
        );

        {
            let mut cache = provider.jwks_cache.write().await;
            *cache = Some(JwksCache {
                keys: JwkSet { keys: vec![jwk] },
                fetched_at: Instant::now(),
            });
        }

        let claims = serde_json::json!({
            "sub": "jwt-user",
            "iss": "https://issuer.example.com",
            "aud": "test-client-id",
            "exp": (chrono::Utc::now().timestamp() - 3600),
        });
        let token = make_jwt(&encoding_key, &claims);

        let result = provider.authenticate(&token).await;
        assert!(matches!(result, Err(AuthError::InvalidToken)));
    }

    #[tokio::test]
    async fn jwt_provider_rejects_missing_sub() {
        let encoding_key = make_test_encoding_key();
        let jwk = make_test_jwk();
        let backend = Arc::new(SqliteBackend::new_in_memory().unwrap());

        let provider = JwtAuthProvider::new(
            "https://issuer.example.com".to_string(),
            "test-client-id".to_string(),
            None,
            HashMap::new(),
            "groups".to_string(),
            None,
            backend,
        );

        {
            let mut cache = provider.jwks_cache.write().await;
            *cache = Some(JwksCache {
                keys: JwkSet { keys: vec![jwk] },
                fetched_at: Instant::now(),
            });
        }

        let claims = serde_json::json!({
            "iss": "https://issuer.example.com",
            "aud": "test-client-id",
            "exp": (chrono::Utc::now().timestamp() + 3600),
        });
        let token = make_jwt(&encoding_key, &claims);

        let result = provider.authenticate(&token).await;
        assert!(matches!(result, Err(AuthError::InvalidToken)));
    }

    #[tokio::test]
    async fn jwt_provider_auto_provisions_unknown_user() {
        let encoding_key = make_test_encoding_key();
        let jwk = make_test_jwk();
        let backend = Arc::new(SqliteBackend::new_in_memory().unwrap());

        let provider = JwtAuthProvider::new(
            "https://issuer.example.com".to_string(),
            "test-client-id".to_string(),
            None,
            HashMap::new(),
            "groups".to_string(),
            None,
            backend,
        );

        {
            let mut cache = provider.jwks_cache.write().await;
            *cache = Some(JwksCache {
                keys: JwkSet { keys: vec![jwk] },
                fetched_at: Instant::now(),
            });
        }

        let claims = serde_json::json!({
            "sub": "new-oidc-user",
            "name": "New User",
            "email": "new@example.com",
            "iss": "https://issuer.example.com",
            "aud": "test-client-id",
            "exp": (chrono::Utc::now().timestamp() + 3600),
        });
        let token = make_jwt(&encoding_key, &claims);

        let user = provider.authenticate(&token).await.unwrap().user;
        // Fallback order: preferred_username → email → sub
        assert_eq!(user.username(), "new@example.com");
        assert_eq!(user.display_name(), Some("New User"));
        assert_eq!(user.email(), Some("new@example.com"));
    }

    #[tokio::test]
    async fn jwt_provider_validates_required_string_claim() {
        let encoding_key = make_test_encoding_key();
        let jwk = make_test_jwk();
        let backend = Arc::new(SqliteBackend::new_in_memory().unwrap());
        backend
            .create_user(&CreateUserParams {
                username: "jwt-user".to_string(),
                sub: None,
                display_name: None,
                email: None,
            })
            .await
            .unwrap();

        let mut required = HashMap::new();
        required.insert("custom:tenant".to_string(), "acme-corp".to_string());

        let provider = JwtAuthProvider::new(
            "https://issuer.example.com".to_string(),
            "test-client-id".to_string(),
            None,
            required,
            "groups".to_string(),
            None,
            backend,
        );

        {
            let mut cache = provider.jwks_cache.write().await;
            *cache = Some(JwksCache {
                keys: JwkSet { keys: vec![jwk] },
                fetched_at: Instant::now(),
            });
        }

        let claims = serde_json::json!({
            "sub": "jwt-user",
            "iss": "https://issuer.example.com",
            "aud": "test-client-id",
            "exp": (chrono::Utc::now().timestamp() + 3600),
            "custom:tenant": "acme-corp",
        });
        let token = make_jwt(&encoding_key, &claims);

        let user = provider.authenticate(&token).await.unwrap().user;
        assert_eq!(user.username(), "jwt-user");
    }

    #[tokio::test]
    async fn jwt_provider_validates_required_array_claim() {
        let encoding_key = make_test_encoding_key();
        let jwk = make_test_jwk();
        let backend = Arc::new(SqliteBackend::new_in_memory().unwrap());
        backend
            .create_user(&CreateUserParams {
                username: "jwt-user".to_string(),
                sub: None,
                display_name: None,
                email: None,
            })
            .await
            .unwrap();

        let mut required = HashMap::new();
        required.insert("cognito:groups".to_string(), "senko".to_string());

        let provider = JwtAuthProvider::new(
            "https://issuer.example.com".to_string(),
            "test-client-id".to_string(),
            None,
            required,
            "groups".to_string(),
            None,
            backend,
        );

        {
            let mut cache = provider.jwks_cache.write().await;
            *cache = Some(JwksCache {
                keys: JwkSet { keys: vec![jwk] },
                fetched_at: Instant::now(),
            });
        }

        let claims = serde_json::json!({
            "sub": "jwt-user",
            "iss": "https://issuer.example.com",
            "aud": "test-client-id",
            "exp": (chrono::Utc::now().timestamp() + 3600),
            "cognito:groups": ["admin", "senko", "users"],
        });
        let token = make_jwt(&encoding_key, &claims);

        let user = provider.authenticate(&token).await.unwrap().user;
        assert_eq!(user.username(), "jwt-user");
    }

    #[tokio::test]
    async fn jwt_provider_rejects_missing_required_claim() {
        let encoding_key = make_test_encoding_key();
        let jwk = make_test_jwk();
        let backend = Arc::new(SqliteBackend::new_in_memory().unwrap());

        let mut required = HashMap::new();
        required.insert("custom:tenant".to_string(), "acme-corp".to_string());

        let provider = JwtAuthProvider::new(
            "https://issuer.example.com".to_string(),
            "test-client-id".to_string(),
            None,
            required,
            "groups".to_string(),
            None,
            backend,
        );

        {
            let mut cache = provider.jwks_cache.write().await;
            *cache = Some(JwksCache {
                keys: JwkSet { keys: vec![jwk] },
                fetched_at: Instant::now(),
            });
        }

        // JWT without the required "custom:tenant" claim
        let claims = serde_json::json!({
            "sub": "jwt-user",
            "iss": "https://issuer.example.com",
            "aud": "test-client-id",
            "exp": (chrono::Utc::now().timestamp() + 3600),
        });
        let token = make_jwt(&encoding_key, &claims);

        let result = provider.authenticate(&token).await;
        assert!(matches!(result, Err(AuthError::InvalidToken)));
    }

    #[tokio::test]
    async fn jwt_provider_rejects_wrong_required_claim() {
        let encoding_key = make_test_encoding_key();
        let jwk = make_test_jwk();
        let backend = Arc::new(SqliteBackend::new_in_memory().unwrap());

        let mut required = HashMap::new();
        required.insert("custom:tenant".to_string(), "acme-corp".to_string());

        let provider = JwtAuthProvider::new(
            "https://issuer.example.com".to_string(),
            "test-client-id".to_string(),
            None,
            required,
            "groups".to_string(),
            None,
            backend,
        );

        {
            let mut cache = provider.jwks_cache.write().await;
            *cache = Some(JwksCache {
                keys: JwkSet { keys: vec![jwk] },
                fetched_at: Instant::now(),
            });
        }

        let claims = serde_json::json!({
            "sub": "jwt-user",
            "iss": "https://issuer.example.com",
            "aud": "test-client-id",
            "exp": (chrono::Utc::now().timestamp() + 3600),
            "custom:tenant": "other-corp",
        });
        let token = make_jwt(&encoding_key, &claims);

        let result = provider.authenticate(&token).await;
        assert!(matches!(result, Err(AuthError::InvalidToken)));
    }

    #[tokio::test]
    async fn jwt_provider_rejects_array_claim_without_expected_value() {
        let encoding_key = make_test_encoding_key();
        let jwk = make_test_jwk();
        let backend = Arc::new(SqliteBackend::new_in_memory().unwrap());

        let mut required = HashMap::new();
        required.insert("cognito:groups".to_string(), "senko".to_string());

        let provider = JwtAuthProvider::new(
            "https://issuer.example.com".to_string(),
            "test-client-id".to_string(),
            None,
            required,
            "groups".to_string(),
            None,
            backend,
        );

        {
            let mut cache = provider.jwks_cache.write().await;
            *cache = Some(JwksCache {
                keys: JwkSet { keys: vec![jwk] },
                fetched_at: Instant::now(),
            });
        }

        let claims = serde_json::json!({
            "sub": "jwt-user",
            "iss": "https://issuer.example.com",
            "aud": "test-client-id",
            "exp": (chrono::Utc::now().timestamp() + 3600),
            "cognito:groups": ["admin", "users"],
        });
        let token = make_jwt(&encoding_key, &claims);

        let result = provider.authenticate(&token).await;
        assert!(matches!(result, Err(AuthError::InvalidToken)));
    }

    #[tokio::test]
    async fn jwt_provider_uses_preferred_username_fallback() {
        let encoding_key = make_test_encoding_key();
        let jwk = make_test_jwk();
        let backend = Arc::new(SqliteBackend::new_in_memory().unwrap());

        let provider = JwtAuthProvider::new(
            "https://issuer.example.com".to_string(),
            "test-client-id".to_string(),
            None,
            HashMap::new(),
            "groups".to_string(),
            None,
            backend,
        );

        {
            let mut cache = provider.jwks_cache.write().await;
            *cache = Some(JwksCache {
                keys: JwkSet { keys: vec![jwk] },
                fetched_at: Instant::now(),
            });
        }

        let claims = serde_json::json!({
            "sub": "sub-value",
            "preferred_username": "pref-user",
            "email": "user@example.com",
            "iss": "https://issuer.example.com",
            "aud": "test-client-id",
            "exp": (chrono::Utc::now().timestamp() + 3600),
        });
        let token = make_jwt(&encoding_key, &claims);

        let user = provider.authenticate(&token).await.unwrap().user;
        assert_eq!(user.username(), "pref-user");
    }

    #[tokio::test]
    async fn jwt_provider_uses_email_fallback() {
        let encoding_key = make_test_encoding_key();
        let jwk = make_test_jwk();
        let backend = Arc::new(SqliteBackend::new_in_memory().unwrap());

        let provider = JwtAuthProvider::new(
            "https://issuer.example.com".to_string(),
            "test-client-id".to_string(),
            None,
            HashMap::new(),
            "groups".to_string(),
            None,
            backend,
        );

        {
            let mut cache = provider.jwks_cache.write().await;
            *cache = Some(JwksCache {
                keys: JwkSet { keys: vec![jwk] },
                fetched_at: Instant::now(),
            });
        }

        let claims = serde_json::json!({
            "sub": "sub-value",
            "email": "user@example.com",
            "iss": "https://issuer.example.com",
            "aud": "test-client-id",
            "exp": (chrono::Utc::now().timestamp() + 3600),
        });
        let token = make_jwt(&encoding_key, &claims);

        let user = provider.authenticate(&token).await.unwrap().user;
        assert_eq!(user.username(), "user@example.com");
    }

    #[tokio::test]
    async fn jwt_provider_uses_sub_fallback() {
        let encoding_key = make_test_encoding_key();
        let jwk = make_test_jwk();
        let backend = Arc::new(SqliteBackend::new_in_memory().unwrap());

        let provider = JwtAuthProvider::new(
            "https://issuer.example.com".to_string(),
            "test-client-id".to_string(),
            None,
            HashMap::new(),
            "groups".to_string(),
            None,
            backend,
        );

        {
            let mut cache = provider.jwks_cache.write().await;
            *cache = Some(JwksCache {
                keys: JwkSet { keys: vec![jwk] },
                fetched_at: Instant::now(),
            });
        }

        let claims = serde_json::json!({
            "sub": "sub-value",
            "iss": "https://issuer.example.com",
            "aud": "test-client-id",
            "exp": (chrono::Utc::now().timestamp() + 3600),
        });
        let token = make_jwt(&encoding_key, &claims);

        let user = provider.authenticate(&token).await.unwrap().user;
        assert_eq!(user.username(), "sub-value");
    }

    #[tokio::test]
    async fn jwt_provider_uses_configured_username_claim() {
        let encoding_key = make_test_encoding_key();
        let jwk = make_test_jwk();
        let backend = Arc::new(SqliteBackend::new_in_memory().unwrap());

        let provider = JwtAuthProvider::new(
            "https://issuer.example.com".to_string(),
            "test-client-id".to_string(),
            Some("custom_name".to_string()),
            HashMap::new(),
            "groups".to_string(),
            None,
            backend,
        );

        {
            let mut cache = provider.jwks_cache.write().await;
            *cache = Some(JwksCache {
                keys: JwkSet { keys: vec![jwk] },
                fetched_at: Instant::now(),
            });
        }

        let claims = serde_json::json!({
            "sub": "sub-value",
            "preferred_username": "pref-user",
            "custom_name": "custom-user",
            "iss": "https://issuer.example.com",
            "aud": "test-client-id",
            "exp": (chrono::Utc::now().timestamp() + 3600),
        });
        let token = make_jwt(&encoding_key, &claims);

        let user = provider.authenticate(&token).await.unwrap().user;
        assert_eq!(user.username(), "custom-user");
    }

    #[tokio::test]
    async fn jwt_provider_rejects_missing_configured_username_claim() {
        let encoding_key = make_test_encoding_key();
        let jwk = make_test_jwk();
        let backend = Arc::new(SqliteBackend::new_in_memory().unwrap());

        let provider = JwtAuthProvider::new(
            "https://issuer.example.com".to_string(),
            "test-client-id".to_string(),
            Some("custom_name".to_string()),
            HashMap::new(),
            "groups".to_string(),
            None,
            backend,
        );

        {
            let mut cache = provider.jwks_cache.write().await;
            *cache = Some(JwksCache {
                keys: JwkSet { keys: vec![jwk] },
                fetched_at: Instant::now(),
            });
        }

        let claims = serde_json::json!({
            "sub": "sub-value",
            "preferred_username": "pref-user",
            "iss": "https://issuer.example.com",
            "aud": "test-client-id",
            "exp": (chrono::Utc::now().timestamp() + 3600),
        });
        let token = make_jwt(&encoding_key, &claims);

        let result = provider.authenticate(&token).await;
        assert!(matches!(result, Err(AuthError::InvalidToken)));
    }

    #[tokio::test]
    async fn jwt_provider_master_group_sets_is_master() {
        let encoding_key = make_test_encoding_key();
        let jwk = make_test_jwk();
        let backend = Arc::new(SqliteBackend::new_in_memory().unwrap());

        let provider = JwtAuthProvider::new(
            "https://issuer.example.com".to_string(),
            "test-client-id".to_string(),
            None,
            HashMap::new(),
            "groups".to_string(),
            Some("senko-admin".to_string()),
            backend,
        );

        {
            let mut cache = provider.jwks_cache.write().await;
            *cache = Some(JwksCache {
                keys: JwkSet { keys: vec![jwk] },
                fetched_at: Instant::now(),
            });
        }

        let claims = serde_json::json!({
            "sub": "jwt-admin",
            "iss": "https://issuer.example.com",
            "aud": "test-client-id",
            "exp": (chrono::Utc::now().timestamp() + 3600),
            "groups": ["senko-admin", "engineering"],
        });
        let token = make_jwt(&encoding_key, &claims);

        let result = provider.authenticate(&token).await.unwrap();
        assert!(result.is_master);
    }

    #[tokio::test]
    async fn jwt_provider_master_group_absent_stays_false() {
        let encoding_key = make_test_encoding_key();
        let jwk = make_test_jwk();
        let backend = Arc::new(SqliteBackend::new_in_memory().unwrap());

        let provider = JwtAuthProvider::new(
            "https://issuer.example.com".to_string(),
            "test-client-id".to_string(),
            None,
            HashMap::new(),
            "groups".to_string(),
            Some("senko-admin".to_string()),
            backend,
        );

        {
            let mut cache = provider.jwks_cache.write().await;
            *cache = Some(JwksCache {
                keys: JwkSet { keys: vec![jwk] },
                fetched_at: Instant::now(),
            });
        }

        let claims = serde_json::json!({
            "sub": "jwt-user",
            "iss": "https://issuer.example.com",
            "aud": "test-client-id",
            "exp": (chrono::Utc::now().timestamp() + 3600),
            "groups": ["engineering"],
        });
        let token = make_jwt(&encoding_key, &claims);

        let result = provider.authenticate(&token).await.unwrap();
        assert!(!result.is_master);
    }

    #[tokio::test]
    async fn jwt_provider_custom_groups_claim_respected() {
        let encoding_key = make_test_encoding_key();
        let jwk = make_test_jwk();
        let backend = Arc::new(SqliteBackend::new_in_memory().unwrap());

        let provider = JwtAuthProvider::new(
            "https://issuer.example.com".to_string(),
            "test-client-id".to_string(),
            None,
            HashMap::new(),
            "cognito:groups".to_string(),
            Some("admin".to_string()),
            backend,
        );

        {
            let mut cache = provider.jwks_cache.write().await;
            *cache = Some(JwksCache {
                keys: JwkSet { keys: vec![jwk] },
                fetched_at: Instant::now(),
            });
        }

        let claims = serde_json::json!({
            "sub": "cognito-admin",
            "iss": "https://issuer.example.com",
            "aud": "test-client-id",
            "exp": (chrono::Utc::now().timestamp() + 3600),
            "cognito:groups": ["admin"],
        });
        let token = make_jwt(&encoding_key, &claims);

        let result = provider.authenticate(&token).await.unwrap();
        assert!(result.is_master);
    }

    #[tokio::test]
    async fn jwt_provider_master_group_unset_is_never_master() {
        let encoding_key = make_test_encoding_key();
        let jwk = make_test_jwk();
        let backend = Arc::new(SqliteBackend::new_in_memory().unwrap());

        let provider = JwtAuthProvider::new(
            "https://issuer.example.com".to_string(),
            "test-client-id".to_string(),
            None,
            HashMap::new(),
            "groups".to_string(),
            None,
            backend,
        );

        {
            let mut cache = provider.jwks_cache.write().await;
            *cache = Some(JwksCache {
                keys: JwkSet { keys: vec![jwk] },
                fetched_at: Instant::now(),
            });
        }

        let claims = serde_json::json!({
            "sub": "jwt-user",
            "iss": "https://issuer.example.com",
            "aud": "test-client-id",
            "exp": (chrono::Utc::now().timestamp() + 3600),
            "groups": ["admin"],
        });
        let token = make_jwt(&encoding_key, &claims);

        let result = provider.authenticate(&token).await.unwrap();
        assert!(!result.is_master);
    }
}
