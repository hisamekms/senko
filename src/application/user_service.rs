use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;

use crate::application::port::TaskBackend;
use crate::application::port::user_operations::UserOperations;
use crate::domain::duration::parse_duration;
use crate::domain::pagination::ListPage;
use crate::domain::user::{
    ApiKey, ApiKeyId, ApiKeyWithSecret, CreateUserParams, ListSessionsFilter, ListUsersFilter,
    NewApiKey, SessionId, SessionRevokeScope, UpdateUserParams, User, UserCreationSource,
    UserEvent, UserId, Username,
};
use crate::infra::config::SessionConfig;

/// Emit Contract #8 business events for a `UserService` mutation.
///
/// The `target_user_id` is the user the operation acts on (the User aggregate
/// identifier in OTel terms — `senko.user.id`). The actor (`enduser.*`) is
/// auto-attached by `BusinessAttributesProcessor` and must NOT be passed here.
fn emit_user_events(target_user_id: UserId, events: &[UserEvent]) {
    for ev in events {
        match ev {
            UserEvent::Created { source } => {
                crate::emit_business_event!(
                    "senko.user.created",
                    senko.user.id = target_user_id.0,
                    source = source.as_str(),
                );
            }
            UserEvent::Updated { changed_fields } => {
                let changed_fields_json = serde_json::to_string(changed_fields).unwrap_or_default();
                crate::emit_business_event!(
                    "senko.user.updated",
                    senko.user.id = target_user_id.0,
                    changed_fields = changed_fields_json.as_str(),
                );
            }
            UserEvent::ApiKeyIssued { api_key_id } => {
                crate::emit_business_event!(
                    "senko.user.api_key_issued",
                    senko.user.id = target_user_id.0,
                    api_key_id = api_key_id.0,
                );
            }
            UserEvent::ApiKeyRevoked { api_key_id } => {
                crate::emit_business_event!(
                    "senko.user.api_key_revoked",
                    senko.user.id = target_user_id.0,
                    api_key_id = api_key_id.0,
                );
            }
            UserEvent::SessionRevoked { session_id, scope } => {
                crate::emit_business_event!(
                    "senko.user.session_revoked",
                    senko.user.id = target_user_id.0,
                    session.id = session_id.0,
                    scope = scope.as_str(),
                );
            }
        }
    }
}

pub struct UserService {
    backend: Arc<dyn TaskBackend>,
}

impl UserService {
    pub fn new(backend: Arc<dyn TaskBackend>) -> Self {
        Self { backend }
    }
}

#[async_trait]
impl UserOperations for UserService {
    // --- User management ---

    async fn list_users(&self, filter: &ListUsersFilter) -> Result<ListPage<User>> {
        self.backend.list_users(filter).await
    }

    async fn create_user(
        &self,
        params: &CreateUserParams,
        source: UserCreationSource,
    ) -> Result<(User, Vec<UserEvent>)> {
        params.validate()?;
        let user = self.backend.create_user(params).await?;
        let events = vec![UserEvent::Created { source }];
        emit_user_events(user.id(), &events);
        Ok((user, events))
    }

    async fn get_user(&self, id: UserId) -> Result<User> {
        self.backend.get_user(id).await
    }

    async fn get_user_by_username(&self, username: &Username) -> Result<User> {
        self.backend.get_user_by_username(username).await
    }

    async fn get_user_by_sub(&self, sub: &str) -> Result<User> {
        self.backend.get_user_by_sub(sub).await
    }

    async fn update_user(
        &self,
        id: UserId,
        params: &UpdateUserParams,
    ) -> Result<(User, Vec<UserEvent>)> {
        let mut changed_fields: Vec<String> = Vec::new();
        if params.username.is_some() {
            changed_fields.push("username".to_string());
        }
        if params.display_name.is_some() {
            changed_fields.push("display_name".to_string());
        }
        let user = self.backend.update_user(id, params).await?;
        let events = if changed_fields.is_empty() {
            vec![]
        } else {
            vec![UserEvent::Updated { changed_fields }]
        };
        emit_user_events(id, &events);
        Ok((user, events))
    }

    async fn delete_user(&self, id: UserId) -> Result<()> {
        self.backend.delete_user(id).await
    }

    // --- API Key management ---

    async fn create_api_key(
        &self,
        user_id: UserId,
        name: &str,
        device_name: Option<&str>,
    ) -> Result<(ApiKeyWithSecret, Vec<UserEvent>)> {
        let new_key = NewApiKey::generate();
        let key = self
            .backend
            .create_api_key(user_id, name, device_name, &new_key)
            .await?;
        let events = vec![UserEvent::ApiKeyIssued {
            api_key_id: ApiKeyId(key.id()),
        }];
        emit_user_events(user_id, &events);
        Ok((key, events))
    }

    async fn list_api_keys(&self, user_id: UserId) -> Result<Vec<ApiKey>> {
        self.backend.list_api_keys(user_id).await
    }

    async fn delete_api_key(&self, key_id: i64, user_id: UserId) -> Result<Vec<UserEvent>> {
        self.backend
            .delete_api_key_for_user(key_id, user_id)
            .await?;
        let events = vec![UserEvent::ApiKeyRevoked {
            api_key_id: ApiKeyId(key_id),
        }];
        emit_user_events(user_id, &events);
        Ok(events)
    }

    // --- Session management ---

    /// Get a user by sub, creating them if they don't exist. When a creation
    /// happens, returns `[UserEvent::Created { source }]`; for an existing
    /// user the event vec is empty.
    async fn get_or_create_user(
        &self,
        sub: &str,
        username: &Username,
        display_name: Option<&str>,
        email: Option<&str>,
        source: UserCreationSource,
    ) -> Result<(User, Vec<UserEvent>)> {
        match self.backend.get_user_by_sub(sub).await {
            Ok(user) => Ok((user, vec![])),
            Err(_) => {
                let params = CreateUserParams {
                    username: username.clone(),
                    sub: Some(sub.to_string()),
                    display_name: display_name.map(String::from),
                    email: email.map(String::from),
                };
                let user = self.backend.create_user(&params).await?;
                let events = vec![UserEvent::Created { source }];
                emit_user_events(user.id(), &events);
                Ok((user, events))
            }
        }
    }

    /// Create a session token (API key) for a user, enforcing `max_per_user`.
    /// When the limit is reached, the oldest key is revoked to make room.
    async fn create_session_token(
        &self,
        user_id: UserId,
        device_name: Option<&str>,
        session_config: &SessionConfig,
    ) -> Result<ApiKeyWithSecret> {
        if let Some(max) = session_config.max_per_user {
            let keys = self.backend.list_api_keys(user_id).await?;
            if keys.len() as u32 >= max {
                // Revoke oldest keys to make room
                let mut sorted = keys;
                sorted.sort_by(|a, b| a.created_at().cmp(b.created_at()));
                let to_remove = (sorted.len() as u32) - max + 1;
                for key in sorted.iter().take(to_remove as usize) {
                    self.backend.delete_api_key(key.id()).await?;
                }
            }
        }

        let new_key = NewApiKey::generate();
        self.backend
            .create_api_key(user_id, "", device_name, &new_key)
            .await
    }

    /// List active (non-expired) sessions for a user.
    ///
    /// Paginates raw `api_keys` by id via `list_api_keys_page`, filtering expired
    /// rows in-memory. `next_cursor` is derived from the raw last row so that
    /// in-memory filtering never breaks the cursor chain. Pages may therefore
    /// contain fewer items than `filter.limit`.
    async fn list_active_sessions(
        &self,
        user_id: UserId,
        session_config: &SessionConfig,
        filter: &ListSessionsFilter,
    ) -> Result<ListPage<ApiKey>> {
        let raw = self.backend.list_api_keys_page(user_id, filter).await?;
        let now = chrono::Utc::now();
        let items = raw
            .items
            .into_iter()
            .filter(|k| !is_key_expired(k, session_config, now))
            .collect();
        Ok(ListPage {
            items,
            next_cursor: raw.next_cursor,
        })
    }

    /// Revoke a specific session, verifying ownership.
    async fn revoke_session(&self, key_id: i64, user_id: UserId) -> Result<Vec<UserEvent>> {
        self.backend
            .delete_api_key_for_user(key_id, user_id)
            .await?;
        let events = vec![UserEvent::SessionRevoked {
            session_id: SessionId(key_id),
            scope: SessionRevokeScope::Single,
        }];
        emit_user_events(user_id, &events);
        Ok(events)
    }

    /// Revoke all sessions for a user. Lists existing keys, deletes them all
    /// in one statement, and emits one `SessionRevoked` event per session id.
    async fn revoke_all_sessions(&self, user_id: UserId) -> Result<Vec<UserEvent>> {
        let keys = self.backend.list_api_keys(user_id).await?;
        self.backend.delete_all_api_keys_for_user(user_id).await?;
        let events: Vec<UserEvent> = keys
            .into_iter()
            .map(|k| UserEvent::SessionRevoked {
                session_id: SessionId(k.id()),
                scope: SessionRevokeScope::All,
            })
            .collect();
        emit_user_events(user_id, &events);
        Ok(events)
    }

    async fn fetch_me(&self) -> Result<serde_json::Value> {
        anyhow::bail!("fetch_me is only supported in proxy mode")
    }
}

/// Check whether an API key is expired based on token config.
pub fn is_key_expired(
    key: &ApiKey,
    session_config: &SessionConfig,
    now: chrono::DateTime<chrono::Utc>,
) -> bool {
    // Check absolute TTL
    if let Some(ref ttl_str) = session_config.ttl
        && let Ok(ttl) = parse_duration(ttl_str)
        && let Ok(created) = chrono::DateTime::parse_from_rfc3339(key.created_at())
    {
        let elapsed = now.signed_duration_since(created);
        if elapsed > chrono::Duration::from_std(ttl).unwrap_or(chrono::Duration::MAX) {
            return true;
        }
    }

    // Check inactive TTL
    if let Some(ref inactive_ttl_str) = session_config.inactive_ttl
        && let Ok(inactive_ttl) = parse_duration(inactive_ttl_str)
        && let Some(last_used) = key.last_used_at()
        && let Ok(last) = chrono::DateTime::parse_from_rfc3339(last_used)
    {
        let elapsed = now.signed_duration_since(last);
        if elapsed > chrono::Duration::from_std(inactive_ttl).unwrap_or(chrono::Duration::MAX) {
            return true;
        }
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::user::{CreateUserParams, UpdateUserParams, Username};
    use crate::infra::sqlite::SqliteBackend;

    fn new_service() -> UserService {
        UserService::new(Arc::new(SqliteBackend::new_in_memory().unwrap()))
    }

    fn user_params(name: &str) -> CreateUserParams {
        CreateUserParams {
            username: Username(name.to_string()),
            sub: Some(format!("sub-{name}")),
            display_name: None,
            email: None,
        }
    }

    #[tokio::test]
    async fn create_user_emits_created_with_manual_source() {
        let svc = new_service();
        let (user, events) = svc
            .create_user(&user_params("alice"), UserCreationSource::Manual)
            .await
            .unwrap();
        assert_eq!(user.username(), "alice");
        assert_eq!(
            events,
            vec![UserEvent::Created {
                source: UserCreationSource::Manual
            }]
        );
    }

    #[tokio::test]
    async fn create_user_emits_created_with_oidc_source() {
        let svc = new_service();
        let (_user, events) = svc
            .create_user(&user_params("bob"), UserCreationSource::OidcProvisioning)
            .await
            .unwrap();
        assert_eq!(
            events,
            vec![UserEvent::Created {
                source: UserCreationSource::OidcProvisioning
            }]
        );
    }

    #[tokio::test]
    async fn create_user_emits_created_with_trusted_headers_source() {
        let svc = new_service();
        let (_user, events) = svc
            .create_user(
                &user_params("carol"),
                UserCreationSource::TrustedHeadersProvisioning,
            )
            .await
            .unwrap();
        assert_eq!(
            events,
            vec![UserEvent::Created {
                source: UserCreationSource::TrustedHeadersProvisioning
            }]
        );
    }

    #[tokio::test]
    async fn update_user_emits_updated_with_changed_fields() {
        let svc = new_service();
        let (user, _) = svc
            .create_user(&user_params("dave"), UserCreationSource::Manual)
            .await
            .unwrap();

        // Username only.
        let (_, events) = svc
            .update_user(
                user.id(),
                &UpdateUserParams {
                    username: Some(Username("dave2".to_string())),
                    display_name: None,
                },
            )
            .await
            .unwrap();
        assert_eq!(
            events,
            vec![UserEvent::Updated {
                changed_fields: vec!["username".to_string()]
            }]
        );

        // display_name only.
        let (_, events) = svc
            .update_user(
                user.id(),
                &UpdateUserParams {
                    username: None,
                    display_name: Some(Some("Dave".to_string())),
                },
            )
            .await
            .unwrap();
        assert_eq!(
            events,
            vec![UserEvent::Updated {
                changed_fields: vec!["display_name".to_string()]
            }]
        );

        // Both fields.
        let (_, events) = svc
            .update_user(
                user.id(),
                &UpdateUserParams {
                    username: Some(Username("dave3".to_string())),
                    display_name: Some(None),
                },
            )
            .await
            .unwrap();
        assert_eq!(
            events,
            vec![UserEvent::Updated {
                changed_fields: vec!["username".to_string(), "display_name".to_string()]
            }]
        );

        // No fields → empty events.
        let (_, events) = svc
            .update_user(
                user.id(),
                &UpdateUserParams {
                    username: None,
                    display_name: None,
                },
            )
            .await
            .unwrap();
        assert!(events.is_empty());
    }

    #[tokio::test]
    async fn create_api_key_emits_api_key_issued() {
        let svc = new_service();
        let (user, _) = svc
            .create_user(&user_params("erin"), UserCreationSource::Manual)
            .await
            .unwrap();
        let (key, events) = svc
            .create_api_key(user.id(), "key-1", Some("laptop"))
            .await
            .unwrap();
        assert_eq!(
            events,
            vec![UserEvent::ApiKeyIssued {
                api_key_id: ApiKeyId(key.id())
            }]
        );
    }

    #[tokio::test]
    async fn delete_api_key_emits_api_key_revoked() {
        let svc = new_service();
        let (user, _) = svc
            .create_user(&user_params("frank"), UserCreationSource::Manual)
            .await
            .unwrap();
        let (key, _) = svc.create_api_key(user.id(), "key-1", None).await.unwrap();
        let events = svc.delete_api_key(key.id(), user.id()).await.unwrap();
        assert_eq!(
            events,
            vec![UserEvent::ApiKeyRevoked {
                api_key_id: ApiKeyId(key.id())
            }]
        );
    }

    #[tokio::test]
    async fn revoke_session_emits_session_revoked_single() {
        let svc = new_service();
        let (user, _) = svc
            .create_user(&user_params("gina"), UserCreationSource::Manual)
            .await
            .unwrap();
        let (key, _) = svc
            .create_api_key(user.id(), "session-1", None)
            .await
            .unwrap();
        let events = svc.revoke_session(key.id(), user.id()).await.unwrap();
        assert_eq!(
            events,
            vec![UserEvent::SessionRevoked {
                session_id: SessionId(key.id()),
                scope: SessionRevokeScope::Single
            }]
        );
    }

    #[tokio::test]
    async fn revoke_all_sessions_emits_per_session_with_all_scope() {
        let svc = new_service();
        let (user, _) = svc
            .create_user(&user_params("henry"), UserCreationSource::Manual)
            .await
            .unwrap();
        let mut session_ids = Vec::new();
        for i in 0..3 {
            let (k, _) = svc
                .create_api_key(user.id(), &format!("s-{i}"), None)
                .await
                .unwrap();
            session_ids.push(k.id());
        }
        let events = svc.revoke_all_sessions(user.id()).await.unwrap();
        assert_eq!(events.len(), 3);
        for ev in &events {
            assert!(matches!(
                ev,
                UserEvent::SessionRevoked {
                    scope: SessionRevokeScope::All,
                    ..
                }
            ));
        }
        let observed: Vec<i64> = events
            .iter()
            .map(|ev| match ev {
                UserEvent::SessionRevoked { session_id, .. } => session_id.0,
                _ => unreachable!(),
            })
            .collect();
        let mut expected = session_ids;
        expected.sort();
        let mut observed = observed;
        observed.sort();
        assert_eq!(observed, expected);
    }

    #[tokio::test]
    async fn get_or_create_user_existing_user_returns_no_events() {
        let svc = new_service();
        let (created, _) = svc
            .create_user(&user_params("ivy"), UserCreationSource::Manual)
            .await
            .unwrap();
        let (fetched, events) = svc
            .get_or_create_user(
                "sub-ivy",
                created.username(),
                None,
                None,
                UserCreationSource::OidcProvisioning,
            )
            .await
            .unwrap();
        assert_eq!(fetched.id(), created.id());
        assert!(events.is_empty());
    }

    #[tokio::test]
    async fn get_or_create_user_new_user_emits_created() {
        let svc = new_service();
        let username = Username("jane".to_string());
        let (user, events) = svc
            .get_or_create_user(
                "sub-jane",
                &username,
                Some("Jane"),
                Some("jane@example.com"),
                UserCreationSource::TrustedHeadersProvisioning,
            )
            .await
            .unwrap();
        assert_eq!(user.username(), "jane");
        assert_eq!(
            events,
            vec![UserEvent::Created {
                source: UserCreationSource::TrustedHeadersProvisioning
            }]
        );
    }

    // --- Phase B3: business-event emission wire-up check ------------------

    #[tokio::test(flavor = "current_thread")]
    async fn create_user_emits_otel_log_record_with_source() {
        use crate::application::telemetry::test_support::{
            build_capture_provider, capture_layer, lookup_attr,
        };
        use opentelemetry::logs::AnyValue;
        use tracing_subscriber::layer::SubscriberExt;

        let svc = new_service();
        let (exporter, provider) = build_capture_provider();
        let subscriber = tracing_subscriber::registry().with(capture_layer(&provider));

        let user = {
            let _guard = tracing::subscriber::set_default(subscriber);
            let (user, _events) = svc
                .create_user(
                    &user_params("trace-target"),
                    UserCreationSource::OidcProvisioning,
                )
                .await
                .unwrap();
            user
        };

        provider.force_flush().expect("flush ok");
        let logs = exporter.get_emitted_logs().expect("logs exported");
        let record = logs
            .iter()
            .find(|d| d.record.event_name() == Some("senko.user.created"))
            .expect("senko.user.created should be emitted");

        assert_eq!(
            lookup_attr(&record.record, "senko.user.id"),
            Some(AnyValue::Int(user.id().0))
        );
        assert_eq!(
            lookup_attr(&record.record, "source"),
            Some(AnyValue::String("oidc_provisioning".into()))
        );
    }
}
