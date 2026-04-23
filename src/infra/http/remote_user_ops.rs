use anyhow::Result;
use async_trait::async_trait;
use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};
use serde_json::json;

use crate::application::port::UserOperations;
use crate::application::user_service::is_key_expired;
use crate::domain::pagination::{Cursor, ListPage};
use crate::domain::user::{
    ApiKey, ApiKeyWithSecret, CreateUserParams, ListSessionsFilter, ListUsersFilter,
    UpdateUserParams, User, UserId, Username,
};
use crate::infra::config::SessionConfig;

use super::client::HttpClient;
use super::{check_success, read_json_or_error};

/// HTTP client implementing `UserOperations` directly.
///
/// Each basic method maps to a single API endpoint call. Session management
/// methods are composed from primitive HTTP calls, mirroring `UserService`.
pub struct RemoteUserOperations {
    http: HttpClient,
}

impl RemoteUserOperations {
    pub fn new(base_url: &str, api_key: Option<String>) -> Self {
        Self {
            http: HttpClient::new(base_url, api_key),
        }
    }

    fn url(&self, path: &str) -> String {
        self.http.url(path)
    }

    fn auth(&self, builder: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        self.http.auth(builder)
    }

    fn client(&self) -> &reqwest::Client {
        self.http.reqwest()
    }
}

#[async_trait]
impl UserOperations for RemoteUserOperations {
    // --- User management ---

    async fn list_users(&self, filter: &ListUsersFilter) -> Result<ListPage<User>> {
        let mut url = self.url("/api/v1/users");
        let mut params: Vec<String> = Vec::new();
        if let Some(l) = filter.limit {
            params.push(format!("limit={l}"));
        }
        if let Some(after) = filter.after {
            params.push(format!(
                "after={}",
                utf8_percent_encode(&Cursor::encode(after), NON_ALPHANUMERIC)
            ));
        }
        if !params.is_empty() {
            url = format!("{url}?{}", params.join("&"));
        }
        let resp = self.auth(self.client().get(&url)).send().await?;
        read_json_or_error(resp).await
    }

    async fn create_user(&self, params: &CreateUserParams) -> Result<User> {
        let resp = self
            .auth(self.client().post(self.url("/api/v1/users")).json(params))
            .send()
            .await?;
        read_json_or_error(resp).await
    }

    async fn get_user(&self, id: UserId) -> Result<User> {
        let resp = self
            .auth(self.client().get(self.url(&format!("/api/v1/users/{id}"))))
            .send()
            .await?;
        read_json_or_error(resp).await
    }

    async fn get_user_by_username(&self, username: &Username) -> Result<User> {
        let mut filter = ListUsersFilter::default();
        loop {
            let page = self.list_users(&filter).await?;
            for u in page.items {
                if u.username() == username {
                    return Ok(u);
                }
            }
            match page.next_cursor {
                Some(c) => filter.after = Some(Cursor::decode::<i64>(&c)?),
                None => return Err(anyhow::anyhow!("user not found")),
            }
        }
    }

    async fn get_user_by_sub(&self, sub: &str) -> Result<User> {
        let mut filter = ListUsersFilter::default();
        loop {
            let page = self.list_users(&filter).await?;
            for u in page.items {
                if u.sub() == sub {
                    return Ok(u);
                }
            }
            match page.next_cursor {
                Some(c) => filter.after = Some(Cursor::decode::<i64>(&c)?),
                None => return Err(anyhow::anyhow!("user not found")),
            }
        }
    }

    async fn update_user(&self, id: UserId, params: &UpdateUserParams) -> Result<User> {
        let resp = self
            .auth(
                self.client()
                    .put(self.url(&format!("/api/v1/users/{id}")))
                    .json(params),
            )
            .send()
            .await?;
        read_json_or_error(resp).await
    }

    async fn delete_user(&self, id: UserId) -> Result<()> {
        let resp = self
            .auth(
                self.client()
                    .delete(self.url(&format!("/api/v1/users/{id}"))),
            )
            .send()
            .await?;
        check_success(resp).await
    }

    // --- API Key management ---

    async fn create_api_key(
        &self,
        user_id: UserId,
        name: &str,
        device_name: Option<&str>,
    ) -> Result<ApiKeyWithSecret> {
        let resp = self
            .auth(
                self.client()
                    .post(self.url(&format!("/api/v1/users/{user_id}/api-keys")))
                    .json(&json!({ "name": name, "device_name": device_name })),
            )
            .send()
            .await?;
        read_json_or_error(resp).await
    }

    async fn list_api_keys(&self, user_id: UserId) -> Result<Vec<ApiKey>> {
        let resp = self
            .auth(
                self.client()
                    .get(self.url(&format!("/api/v1/users/{user_id}/api-keys"))),
            )
            .send()
            .await?;
        read_json_or_error(resp).await
    }

    async fn delete_api_key(&self, key_id: i64, user_id: UserId) -> Result<()> {
        let resp = self
            .auth(
                self.client()
                    .delete(self.url(&format!("/api/v1/users/{user_id}/api-keys/{key_id}"))),
            )
            .send()
            .await?;
        check_success(resp).await
    }

    // --- Session management ---

    async fn get_or_create_user(
        &self,
        sub: &str,
        username: &Username,
        display_name: Option<&str>,
        email: Option<&str>,
    ) -> Result<User> {
        match self.get_user_by_sub(sub).await {
            Ok(user) => Ok(user),
            Err(_) => {
                let params = CreateUserParams {
                    username: username.clone(),
                    sub: Some(sub.to_string()),
                    display_name: display_name.map(String::from),
                    email: email.map(String::from),
                };
                self.create_user(&params).await
            }
        }
    }

    async fn create_session_token(
        &self,
        user_id: UserId,
        device_name: Option<&str>,
        session_config: &SessionConfig,
    ) -> Result<ApiKeyWithSecret> {
        if let Some(max) = session_config.max_per_user {
            let keys = self.list_api_keys(user_id).await?;
            if keys.len() as u32 >= max {
                let mut sorted = keys;
                sorted.sort_by(|a, b| a.created_at().cmp(b.created_at()));
                let to_remove = (sorted.len() as u32) - max + 1;
                for key in sorted.iter().take(to_remove as usize) {
                    self.delete_api_key(key.id(), user_id).await?;
                }
            }
        }

        self.create_api_key(user_id, "", device_name).await
    }

    async fn list_active_sessions(
        &self,
        user_id: UserId,
        session_config: &SessionConfig,
        filter: &ListSessionsFilter,
    ) -> Result<ListPage<ApiKey>> {
        // Remote path fetches the raw page from the upstream sessions endpoint,
        // then filters expired keys in-memory to match the local UserService.
        let mut url = self.url(&format!("/api/v1/users/{user_id}/sessions"));
        let mut params: Vec<String> = Vec::new();
        if let Some(l) = filter.limit {
            params.push(format!("limit={l}"));
        }
        if let Some(after) = filter.after {
            params.push(format!(
                "after={}",
                utf8_percent_encode(&Cursor::encode(after), NON_ALPHANUMERIC)
            ));
        }
        if !params.is_empty() {
            url = format!("{url}?{}", params.join("&"));
        }
        let resp = self.auth(self.client().get(&url)).send().await?;
        let raw: ListPage<ApiKey> = read_json_or_error(resp).await?;
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

    async fn revoke_session(&self, key_id: i64, user_id: UserId) -> Result<()> {
        let resp = self
            .auth(
                self.client()
                    .delete(self.url(&format!("/api/v1/users/{user_id}/api-keys/{key_id}"))),
            )
            .send()
            .await?;
        check_success(resp).await
    }

    async fn revoke_all_sessions(&self, user_id: UserId) -> Result<()> {
        let keys = self.list_api_keys(user_id).await?;
        for key in keys {
            self.revoke_session(key.id(), user_id).await?;
        }
        Ok(())
    }

    async fn fetch_me(&self) -> Result<serde_json::Value> {
        let resp = self
            .auth(self.client().get(self.url("/auth/me")))
            .send()
            .await?;
        read_json_or_error(resp).await
    }
}
