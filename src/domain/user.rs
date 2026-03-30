use std::fmt;
use std::str::FromStr;

use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use super::error::DomainError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Owner,
    Member,
    Viewer,
}

impl fmt::Display for Role {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Role::Owner => "owner",
            Role::Member => "member",
            Role::Viewer => "viewer",
        };
        write!(f, "{s}")
    }
}

impl FromStr for Role {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "owner" => Ok(Role::Owner),
            "member" => Ok(Role::Member),
            "viewer" => Ok(Role::Viewer),
            _ => Err(DomainError::InvalidRole { value: s.to_string() }.into()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    id: i64,
    username: String,
    display_name: Option<String>,
    email: Option<String>,
    created_at: String,
}

impl User {
    pub fn new(id: i64, username: String, display_name: Option<String>, email: Option<String>, created_at: String) -> Self {
        Self { id, username, display_name, email, created_at }
    }

    pub fn id(&self) -> i64 {
        self.id
    }

    pub fn username(&self) -> &str {
        &self.username
    }

    pub fn display_name(&self) -> Option<&str> {
        self.display_name.as_deref()
    }

    pub fn email(&self) -> Option<&str> {
        self.email.as_deref()
    }

    pub fn created_at(&self) -> &str {
        &self.created_at
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateUserParams {
    pub username: String,
    pub display_name: Option<String>,
    pub email: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectMember {
    id: i64,
    project_id: i64,
    user_id: i64,
    role: Role,
    created_at: String,
}

impl ProjectMember {
    pub fn new(id: i64, project_id: i64, user_id: i64, role: Role, created_at: String) -> Self {
        Self { id, project_id, user_id, role, created_at }
    }

    pub fn id(&self) -> i64 {
        self.id
    }

    pub fn project_id(&self) -> i64 {
        self.project_id
    }

    pub fn user_id(&self) -> i64 {
        self.user_id
    }

    pub fn role(&self) -> Role {
        self.role
    }

    pub fn created_at(&self) -> &str {
        &self.created_at
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddProjectMemberParams {
    pub user_id: i64,
    pub role: Role,
}

impl AddProjectMemberParams {
    pub fn new(user_id: i64, role: Option<Role>) -> Self {
        Self {
            user_id,
            role: role.unwrap_or(Role::Member),
        }
    }
}

// --- API Key types ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKey {
    id: i64,
    user_id: i64,
    key_prefix: String,
    name: String,
    created_at: String,
    last_used_at: Option<String>,
}

impl ApiKey {
    pub fn new(id: i64, user_id: i64, key_prefix: String, name: String, created_at: String, last_used_at: Option<String>) -> Self {
        Self { id, user_id, key_prefix, name, created_at, last_used_at }
    }

    pub fn id(&self) -> i64 {
        self.id
    }

    pub fn user_id(&self) -> i64 {
        self.user_id
    }

    pub fn key_prefix(&self) -> &str {
        &self.key_prefix
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn created_at(&self) -> &str {
        &self.created_at
    }

    pub fn last_used_at(&self) -> Option<&str> {
        self.last_used_at.as_deref()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKeyWithSecret {
    id: i64,
    user_id: i64,
    key: String,
    key_prefix: String,
    name: String,
    created_at: String,
}

impl ApiKeyWithSecret {
    pub fn new(id: i64, user_id: i64, key: String, key_prefix: String, name: String, created_at: String) -> Self {
        Self { id, user_id, key, key_prefix, name, created_at }
    }

    pub fn id(&self) -> i64 {
        self.id
    }

    pub fn user_id(&self) -> i64 {
        self.user_id
    }

    pub fn key(&self) -> &str {
        &self.key
    }

    pub fn key_prefix(&self) -> &str {
        &self.key_prefix
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn created_at(&self) -> &str {
        &self.created_at
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateApiKeyParams {
    pub name: Option<String>,
}

#[derive(Debug, Clone)]
pub struct NewApiKey {
    pub raw_key: String,
    pub key_hash: String,
    pub key_prefix: String,
}

impl NewApiKey {
    pub fn generate() -> Self {
        let raw_key = format!("lf_{}", Uuid::new_v4().simple());
        let key_hash = hash_api_key(&raw_key);
        let key_prefix = raw_key[..11].to_string();
        Self {
            raw_key,
            key_hash,
            key_prefix,
        }
    }
}

pub fn hash_api_key(key: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(key.as_bytes());
    format!("{:x}", hasher.finalize())
}

#[async_trait]
pub trait UserRepository: Send + Sync {
    async fn create_user(&self, params: &CreateUserParams) -> Result<User>;
    async fn get_user(&self, id: i64) -> Result<User>;
    async fn get_user_by_username(&self, username: &str) -> Result<User>;
    async fn list_users(&self) -> Result<Vec<User>>;
    async fn delete_user(&self, id: i64) -> Result<()>;
}

#[async_trait]
pub trait ApiKeyRepository: Send + Sync {
    /// Whether this backend supports API key CRUD (create, list, delete).
    fn supports_api_key_management(&self) -> bool {
        true
    }

    async fn create_api_key(&self, user_id: i64, name: &str, new_key: &NewApiKey) -> Result<ApiKeyWithSecret>;
    async fn list_api_keys(&self, user_id: i64) -> Result<Vec<ApiKey>>;
    async fn delete_api_key(&self, key_id: i64) -> Result<()>;
}
