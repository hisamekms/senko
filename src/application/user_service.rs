use std::sync::Arc;

use anyhow::Result;

use crate::application::port::TaskBackend;
use crate::domain::user::{
    ApiKey, ApiKeyWithSecret, CreateUserParams, NewApiKey, User,
};

pub struct UserService {
    backend: Arc<dyn TaskBackend>,
}

impl UserService {
    pub fn new(backend: Arc<dyn TaskBackend>) -> Self {
        Self { backend }
    }

    pub async fn list_users(&self) -> Result<Vec<User>> {
        self.backend.list_users().await
    }

    pub async fn create_user(&self, params: &CreateUserParams) -> Result<User> {
        self.backend.create_user(params).await
    }

    pub async fn get_user(&self, id: i64) -> Result<User> {
        self.backend.get_user(id).await
    }

    pub async fn get_user_by_username(&self, username: &str) -> Result<User> {
        self.backend.get_user_by_username(username).await
    }

    pub async fn delete_user(&self, id: i64) -> Result<()> {
        self.backend.delete_user(id).await
    }

    // --- API Key management ---

    pub async fn create_api_key(
        &self,
        user_id: i64,
        name: &str,
    ) -> Result<ApiKeyWithSecret> {
        let new_key = NewApiKey::generate();
        self.backend.create_api_key(user_id, name, &new_key).await
    }

    pub async fn list_api_keys(&self, user_id: i64) -> Result<Vec<ApiKey>> {
        self.backend.list_api_keys(user_id).await
    }

    pub async fn delete_api_key(&self, key_id: i64) -> Result<()> {
        self.backend.delete_api_key(key_id).await
    }
}
