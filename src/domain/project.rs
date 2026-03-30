use serde::{Deserialize, Serialize};

/// The default project (id=1) cannot be deleted.
pub const DEFAULT_PROJECT_ID: i64 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    id: i64,
    name: String,
    description: Option<String>,
    created_at: String,
}

impl Project {
    pub fn new(id: i64, name: String, description: Option<String>, created_at: String) -> Self {
        Self { id, name, description, created_at }
    }

    pub fn id(&self) -> i64 {
        self.id
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn description(&self) -> Option<&str> {
        self.description.as_deref()
    }

    pub fn created_at(&self) -> &str {
        &self.created_at
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateProjectParams {
    pub name: String,
    pub description: Option<String>,
}
