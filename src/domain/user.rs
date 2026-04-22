use std::fmt;
use std::str::FromStr;

use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use super::error::DomainError;
use super::project::ProjectId;

/// Newtype wrapper around the user identifier.
///
/// Wraps `i64` with `#[serde(transparent)]` so the JSON wire format stays a
/// bare integer (e.g. `1`), not `{"0": 1}`. The goal is compile-time safety: a
/// `UserId` cannot be accidentally mixed with a `ProjectId`, `TaskId`,
/// `ContractId`, or `api_keys.id` that also happen to be `i64`.
///
/// Like [`crate::domain::project::ProjectId`] and
/// [`crate::domain::contract::ContractId`] (and unlike
/// [`crate::domain::task::TaskId`], which has a sealed `TaskDbId` for the DB
/// PK), `UserId` is the DB primary key itself. The infrastructure layer
/// implements `rusqlite` / `sqlx` traits directly on `UserId` (see
/// `src/infra/mod.rs`), so no separate sealed newtype is needed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct UserId(pub i64);

impl fmt::Display for UserId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl FromStr for UserId {
    type Err = std::num::ParseIntError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        s.parse::<i64>().map(UserId)
    }
}

impl From<i64> for UserId {
    fn from(n: i64) -> Self {
        UserId(n)
    }
}

impl From<UserId> for i64 {
    fn from(id: UserId) -> i64 {
        id.0
    }
}

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
            _ => Err(DomainError::InvalidRole {
                value: s.to_string(),
            }
            .into()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    id: UserId,
    username: String,
    #[serde(default)]
    sub: String,
    display_name: Option<String>,
    email: Option<String>,
    created_at: String,
}

impl User {
    pub fn new(
        id: UserId,
        username: String,
        sub: String,
        display_name: Option<String>,
        email: Option<String>,
        created_at: String,
    ) -> Self {
        Self {
            id,
            username,
            sub,
            display_name,
            email,
            created_at,
        }
    }

    pub fn id(&self) -> UserId {
        self.id
    }

    pub fn username(&self) -> &str {
        &self.username
    }

    pub fn sub(&self) -> &str {
        &self.sub
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
    #[serde(default)]
    pub sub: Option<String>,
    pub display_name: Option<String>,
    pub email: Option<String>,
}

impl CreateUserParams {
    pub fn validate(&self) -> Result<(), DomainError> {
        use super::validator::*;
        validate_string_length("username", &self.username, MAX_USERNAME_LEN)?;
        validate_optional_string_length("display_name", &self.display_name, MAX_DISPLAY_NAME_LEN)?;
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateUserParams {
    pub username: Option<String>,
    pub display_name: Option<Option<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectMember {
    id: i64,
    project_id: ProjectId,
    user_id: UserId,
    role: Role,
    created_at: String,
}

impl ProjectMember {
    pub fn new(
        id: i64,
        project_id: ProjectId,
        user_id: UserId,
        role: Role,
        created_at: String,
    ) -> Self {
        Self {
            id,
            project_id,
            user_id,
            role,
            created_at,
        }
    }

    pub fn id(&self) -> i64 {
        self.id
    }

    pub fn project_id(&self) -> ProjectId {
        self.project_id
    }

    pub fn user_id(&self) -> UserId {
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
    pub user_id: UserId,
    pub role: Role,
}

impl AddProjectMemberParams {
    pub fn new(user_id: UserId, role: Option<Role>) -> Self {
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
    user_id: UserId,
    key_prefix: String,
    name: String,
    device_name: Option<String>,
    created_at: String,
    last_used_at: Option<String>,
}

impl ApiKey {
    pub fn new(
        id: i64,
        user_id: UserId,
        key_prefix: String,
        name: String,
        device_name: Option<String>,
        created_at: String,
        last_used_at: Option<String>,
    ) -> Self {
        Self {
            id,
            user_id,
            key_prefix,
            name,
            device_name,
            created_at,
            last_used_at,
        }
    }

    pub fn id(&self) -> i64 {
        self.id
    }

    pub fn user_id(&self) -> UserId {
        self.user_id
    }

    pub fn key_prefix(&self) -> &str {
        &self.key_prefix
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn device_name(&self) -> Option<&str> {
        self.device_name.as_deref()
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
    user_id: UserId,
    key: String,
    key_prefix: String,
    name: String,
    device_name: Option<String>,
    created_at: String,
}

impl ApiKeyWithSecret {
    pub fn new(
        id: i64,
        user_id: UserId,
        key: String,
        key_prefix: String,
        name: String,
        device_name: Option<String>,
        created_at: String,
    ) -> Self {
        Self {
            id,
            user_id,
            key,
            key_prefix,
            name,
            device_name,
            created_at,
        }
    }

    pub fn id(&self) -> i64 {
        self.id
    }

    pub fn user_id(&self) -> UserId {
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

    pub fn device_name(&self) -> Option<&str> {
        self.device_name.as_deref()
    }

    pub fn created_at(&self) -> &str {
        &self.created_at
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateApiKeyParams {
    pub name: Option<String>,
    pub device_name: Option<String>,
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
    hasher
        .finalize()
        .iter()
        .map(|b| format!("{:02x}", b))
        .collect()
}

#[async_trait]
pub trait UserRepository: Send + Sync {
    async fn create_user(&self, params: &CreateUserParams) -> Result<User>;
    async fn get_user(&self, id: UserId) -> Result<User>;
    async fn get_user_by_username(&self, username: &str) -> Result<User>;
    async fn get_user_by_sub(&self, sub: &str) -> Result<User>;
    async fn update_user(&self, id: UserId, params: &UpdateUserParams) -> Result<User>;
    async fn delete_user(&self, id: UserId) -> Result<()>;
}

#[async_trait]
pub trait ApiKeyRepository: Send + Sync {
    /// Whether this backend supports API key CRUD (create, list, delete).
    fn supports_api_key_management(&self) -> bool {
        true
    }

    async fn create_api_key(
        &self,
        user_id: UserId,
        name: &str,
        device_name: Option<&str>,
        new_key: &NewApiKey,
    ) -> Result<ApiKeyWithSecret>;
    async fn delete_api_key(&self, key_id: i64) -> Result<()>;
    async fn delete_api_key_for_user(&self, key_id: i64, user_id: UserId) -> Result<()>;
    async fn delete_all_api_keys_for_user(&self, user_id: UserId) -> Result<()>;
}
