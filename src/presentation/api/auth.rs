use axum::Json;
use axum::extract::FromRequestParts;
use axum::http::StatusCode;
use axum::http::request::Parts;
use axum::response::{IntoResponse, Response};

use crate::application::port::auth::AuthError;
use crate::bootstrap::AuthMode;
use crate::domain::user::User;

use super::ErrorBody;

// --- AppState trait for auth extraction ---

pub trait HasAuth {
    fn auth_mode(&self) -> Option<&AuthMode>;
}

// --- AuthUser extractor ---

#[derive(Debug, Clone)]
pub struct AuthUser {
    pub user: User,
    #[allow(dead_code)]
    pub groups: Vec<String>,
    #[allow(dead_code)]
    pub scopes: Vec<String>,
    pub is_master: bool,
}

impl<S> FromRequestParts<S> for AuthUser
where
    S: HasAuth + Send + Sync,
{
    type Rejection = AuthError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &S,
    ) -> std::result::Result<Self, Self::Rejection> {
        let mode = match state.auth_mode() {
            Some(m) => m,
            None => {
                return Err(AuthError::MissingToken);
            }
        };

        match mode {
            AuthMode::Token(provider) => {
                let auth_header = parts
                    .headers
                    .get("authorization")
                    .and_then(|v| v.to_str().ok())
                    .ok_or(AuthError::MissingToken)?;

                let token = auth_header
                    .strip_prefix("Bearer ")
                    .ok_or(AuthError::InvalidToken)?;

                let auth_result = provider.authenticate(token).await?;
                Ok(AuthUser {
                    user: auth_result.user,
                    groups: Vec::new(),
                    scopes: Vec::new(),
                    is_master: auth_result.is_master,
                })
            }
            AuthMode::TrustedHeaders(provider) => {
                let result = provider.authenticate_from_headers(&parts.headers).await?;
                Ok(AuthUser {
                    user: result.user,
                    groups: result.groups,
                    scopes: result.scopes,
                    is_master: result.is_master,
                })
            }
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

    async fn from_request_parts(
        parts: &mut Parts,
        state: &S,
    ) -> std::result::Result<Self, Self::Rejection> {
        match AuthUser::from_request_parts(parts, state).await {
            Ok(user) => Ok(OptionalAuthUser(Some(user))),
            Err(_) => Ok(OptionalAuthUser(None)),
        }
    }
}

// --- IntoResponse for AuthError (presentation-layer conversion) ---

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
