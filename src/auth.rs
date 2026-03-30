use std::sync::Arc;

use async_trait::async_trait;
use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Serialize;

use crate::domain::repository::TaskBackend;
use crate::domain::user::{hash_api_key, ProjectMember, Role, User};

// --- AuthProvider trait (extensible for OIDC in the future) ---

#[async_trait]
pub trait AuthProvider: Send + Sync {
    async fn authenticate(&self, token: &str) -> std::result::Result<User, AuthError>;
}

// --- API Key provider ---

pub struct ApiKeyProvider {
    backend: Arc<dyn TaskBackend>,
}

impl ApiKeyProvider {
    pub fn new(backend: Arc<dyn TaskBackend>) -> Self {
        Self { backend }
    }
}

#[async_trait]
impl AuthProvider for ApiKeyProvider {
    async fn authenticate(&self, token: &str) -> std::result::Result<User, AuthError> {
        let key_hash = hash_api_key(token);
        self.backend
            .get_user_by_api_key(&key_hash)
            .await
            .map_err(|_| AuthError::InvalidToken)
    }
}

// --- Auth errors ---

#[derive(Debug)]
pub enum AuthError {
    MissingToken,
    InvalidToken,
    Forbidden(String),
}

impl std::fmt::Display for AuthError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AuthError::MissingToken => write!(f, "missing authorization header"),
            AuthError::InvalidToken => write!(f, "invalid api key"),
            AuthError::Forbidden(msg) => write!(f, "{msg}"),
        }
    }
}

impl std::error::Error for AuthError {}

#[derive(Serialize)]
struct ErrorBody {
    error: String,
}

impl IntoResponse for AuthError {
    fn into_response(self) -> Response {
        let (status, message) = match &self {
            AuthError::MissingToken => (StatusCode::UNAUTHORIZED, "missing authorization header"),
            AuthError::InvalidToken => (StatusCode::UNAUTHORIZED, "invalid api key"),
            AuthError::Forbidden(msg) => (StatusCode::FORBIDDEN, msg.as_str()),
        };
        (
            status,
            Json(ErrorBody {
                error: message.to_string(),
            }),
        )
            .into_response()
    }
}

// --- AppState trait for auth extraction ---

pub trait HasAuth {
    fn auth_provider(&self) -> Option<&dyn AuthProvider>;
}

// --- AuthUser extractor ---

#[derive(Debug, Clone)]
pub struct AuthUser {
    pub user: User,
}

impl<S> FromRequestParts<S> for AuthUser
where
    S: HasAuth + Send + Sync,
{
    type Rejection = AuthError;

    fn from_request_parts(
        parts: &mut Parts,
        state: &S,
    ) -> impl std::future::Future<Output = std::result::Result<Self, Self::Rejection>> + Send {
        async move {
            let provider = match state.auth_provider() {
                Some(p) => p,
                None => {
                    return Err(AuthError::MissingToken);
                }
            };

            let auth_header = parts
                .headers
                .get("authorization")
                .and_then(|v| v.to_str().ok())
                .ok_or(AuthError::MissingToken)?;

            let token = auth_header
                .strip_prefix("Bearer ")
                .ok_or(AuthError::InvalidToken)?;

            let user = provider.authenticate(token).await?;
            Ok(AuthUser { user })
        }
    }
}

// --- Optional auth extractor ---

#[derive(Debug, Clone)]
pub struct OptionalAuthUser(pub Option<AuthUser>);

impl<S> FromRequestParts<S> for OptionalAuthUser
where
    S: HasAuth + Send + Sync,
{
    type Rejection = std::convert::Infallible;

    fn from_request_parts(
        parts: &mut Parts,
        state: &S,
    ) -> impl std::future::Future<Output = std::result::Result<Self, Self::Rejection>> + Send {
        async move {
            match AuthUser::from_request_parts(parts, state).await {
                Ok(user) => Ok(OptionalAuthUser(Some(user))),
                Err(_) => Ok(OptionalAuthUser(None)),
            }
        }
    }
}

// --- Permission & RBAC ---

#[derive(Debug, Clone, Copy)]
pub enum Permission {
    View,  // Viewer, Member, Owner
    Edit,  // Member, Owner
    Admin, // Owner only
}

pub async fn require_project_role(
    backend: &dyn TaskBackend,
    user_id: i64,
    project_id: i64,
    permission: Permission,
) -> std::result::Result<ProjectMember, AuthError> {
    let member = backend
        .get_project_member(project_id, user_id)
        .await
        .map_err(|_| {
            AuthError::Forbidden(format!(
                "user {user_id} is not a member of project {project_id}"
            ))
        })?;

    let allowed = match permission {
        Permission::View => true,
        Permission::Edit => matches!(member.role(), Role::Owner | Role::Member),
        Permission::Admin => matches!(member.role(), Role::Owner),
    };

    if !allowed {
        return Err(AuthError::Forbidden(format!(
            "insufficient permissions: {:?} role cannot perform {:?} operations",
            member.role(), permission
        )));
    }

    Ok(member)
}
