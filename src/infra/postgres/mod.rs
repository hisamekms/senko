use std::collections::HashMap;

use anyhow::{Context, Result};
use async_trait::async_trait;
use sqlx::Row;
use sqlx::postgres::{PgPool, PgPoolOptions};

use crate::application::port::{
    AuthenticationPort, ProjectQueryPort, TaskQueryPort, UserQueryPort,
};
use crate::domain::contract::{
    Contract, ContractNote, ContractRepository, CreateContractParams, UpdateContractArrayParams,
    UpdateContractParams,
};
use crate::domain::error::DomainError;
use crate::domain::metadata_field::{
    CreateMetadataFieldParams, MetadataField, MetadataFieldType, UpdateMetadataFieldParams,
};
use crate::domain::project::{CreateProjectParams, Project};
use crate::domain::task::{
    self, CreateTaskParams, DodItem, ListTasksFilter, MetadataUpdate, Priority, Task, TaskStatus,
    UpdateTaskArrayParams, UpdateTaskParams, shallow_merge_metadata,
};
use crate::domain::user::{
    AddProjectMemberParams, ApiKey, ApiKeyWithSecret, CreateUserParams, NewApiKey, ProjectMember,
    Role, UpdateUserParams, User,
};
use crate::domain::{
    ApiKeyRepository, MetadataFieldRepository, ProjectMemberRepository, ProjectRepository,
    TaskRepository, UserRepository,
};

pub struct PostgresBackend {
    url: String,
    max_connections: Option<u32>,
    pool: tokio::sync::OnceCell<PgPool>,
}

impl PostgresBackend {
    pub fn new(url: String, max_connections: Option<u32>) -> Self {
        Self {
            url,
            max_connections,
            pool: tokio::sync::OnceCell::new(),
        }
    }

    async fn pool(&self) -> Result<&PgPool> {
        self.pool
            .get_or_try_init(|| async {
                let mut opts = PgPoolOptions::new();
                if let Some(n) = self.max_connections {
                    opts = opts.max_connections(n);
                }
                let pool = opts
                    .connect(&self.url)
                    .await
                    .context("failed to connect to PostgreSQL")?;
                run_migrations(&pool).await?;
                Ok(pool)
            })
            .await
    }
}

fn now_utc() -> String {
    chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

struct Migration {
    version: i64,
    description: &'static str,
    sql: &'static str,
}

const MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        description: "initial_schema",
        sql: include_str!("migrations/20260328000000_initial_schema.sql"),
    },
    Migration {
        version: 2,
        description: "add_task_number",
        sql: include_str!("migrations/20260409000000_add_task_number.sql"),
    },
    Migration {
        version: 3,
        description: "add_api_key_device_name",
        sql: include_str!("migrations/20260411000000_add_api_key_device_name.sql"),
    },
    Migration {
        version: 4,
        description: "add_metadata_fields",
        sql: include_str!("migrations/20260412000000_add_metadata_fields.sql"),
    },
    Migration {
        version: 5,
        description: "add_user_sub",
        sql: include_str!("migrations/20260413000000_add_user_sub.sql"),
    },
    Migration {
        version: 6,
        description: "metadata_to_jsonb",
        sql: include_str!("migrations/20260415000000_metadata_to_jsonb.sql"),
    },
    Migration {
        version: 7,
        description: "add_contracts",
        sql: include_str!("migrations/20260417000000_add_contracts.sql"),
    },
];

async fn run_migrations(pool: &PgPool) -> Result<()> {
    // Create migration tracking table
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS _sqlx_migrations (
            version BIGINT PRIMARY KEY,
            description TEXT NOT NULL,
            applied_at TEXT NOT NULL DEFAULT to_char(NOW() AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"')
        )",
    )
    .execute(pool)
    .await
    .context("failed to create migrations table")?;

    let current_version: i64 =
        sqlx::query("SELECT COALESCE(MAX(version), 0) FROM _sqlx_migrations")
            .fetch_one(pool)
            .await?
            .get(0);

    for m in MIGRATIONS {
        if m.version > current_version {
            for statement in m.sql.split(';') {
                let trimmed = statement.trim();
                if !trimmed.is_empty() {
                    sqlx::query(trimmed).execute(pool).await.with_context(|| {
                        format!(
                            "migration v{} ({}) failed: {}",
                            m.version,
                            m.description,
                            &trimmed[..trimmed.len().min(80)]
                        )
                    })?;
                }
            }
            sqlx::query("INSERT INTO _sqlx_migrations (version, description) VALUES ($1, $2)")
                .bind(m.version)
                .bind(m.description)
                .execute(pool)
                .await?;
        }
    }

    Ok(())
}

// --- Helper: build a Task from a tasks row + child queries ---

async fn get_task_by_id(pool: &PgPool, id: i64) -> Result<Task> {
    let row = sqlx::query(
        "SELECT project_id, task_number, title, background, description, plan, status, priority,
                assignee_session_id, created_at, updated_at, started_at, completed_at,
                canceled_at, cancel_reason, branch, pr_url, metadata, assignee_user_id, contract_id
         FROM tasks WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?
    .ok_or(DomainError::TaskNotFound)?;

    let status_str: String = row.get("status");
    let priority_val: i32 = row.get("priority");
    let metadata_str: Option<String> = row.get("metadata");

    let status: TaskStatus = status_str.parse()?;
    let priority = Priority::try_from(priority_val)?;
    let metadata: Option<serde_json::Value> = metadata_str
        .map(|s| serde_json::from_str(&s))
        .transpose()
        .context("invalid metadata JSON in database")?;

    let definition_of_done = sqlx::query(
        "SELECT content, checked FROM task_definition_of_done WHERE task_id = $1 ORDER BY id",
    )
    .bind(id)
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|r| DodItem::new(r.get("content"), r.get::<i32, _>("checked") != 0))
    .collect();

    let in_scope: Vec<String> =
        sqlx::query("SELECT content FROM task_in_scope WHERE task_id = $1 ORDER BY id")
            .bind(id)
            .fetch_all(pool)
            .await?
            .into_iter()
            .map(|r| r.get("content"))
            .collect();

    let out_of_scope: Vec<String> =
        sqlx::query("SELECT content FROM task_out_of_scope WHERE task_id = $1 ORDER BY id")
            .bind(id)
            .fetch_all(pool)
            .await?
            .into_iter()
            .map(|r| r.get("content"))
            .collect();

    let tags: Vec<String> = sqlx::query("SELECT tag FROM task_tags WHERE task_id = $1 ORDER BY id")
        .bind(id)
        .fetch_all(pool)
        .await?
        .into_iter()
        .map(|r| r.get("tag"))
        .collect();

    // Fetch dependency task_numbers (not internal IDs)
    let dependencies: Vec<i64> = sqlx::query(
        "SELECT t.task_number FROM task_dependencies td JOIN tasks t ON t.id = td.depends_on_task_id WHERE td.task_id = $1 ORDER BY td.id",
    )
    .bind(id)
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|r| r.get("task_number"))
    .collect();

    Ok(Task::new(
        id,
        row.get("task_number"),
        row.get("project_id"),
        row.get("title"),
        row.get("background"),
        row.get("description"),
        row.get("plan"),
        priority,
        status,
        row.get("assignee_session_id"),
        row.get("assignee_user_id"),
        row.get("created_at"),
        row.get("updated_at"),
        row.get("started_at"),
        row.get("completed_at"),
        row.get("canceled_at"),
        row.get("cancel_reason"),
        row.get("branch"),
        row.get("pr_url"),
        row.get("contract_id"),
        metadata,
        definition_of_done,
        in_scope,
        out_of_scope,
        tags,
        dependencies,
    ))
}

/// Resolve a user-facing task_number to internal id, verifying project ownership.
///
/// Accepts any sqlx executor (pool or transaction) so callers inside a
/// transaction can avoid acquiring a second connection from the pool.
async fn resolve_task_number<'e, E>(executor: E, project_id: i64, task_number: i64) -> Result<i64>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    let row = sqlx::query("SELECT id FROM tasks WHERE project_id = $1 AND task_number = $2")
        .bind(project_id)
        .bind(task_number)
        .fetch_optional(executor)
        .await?
        .ok_or(DomainError::TaskNotFound)?;
    Ok(row.get("id"))
}

// =============================================================================
// ProjectRepository
// =============================================================================

#[async_trait]
impl ProjectRepository for PostgresBackend {
    async fn create_project(&self, params: &CreateProjectParams) -> Result<Project> {
        let pool = self.pool().await?;
        let row = sqlx::query(
            "INSERT INTO projects (name, description) VALUES ($1, $2) RETURNING id, created_at",
        )
        .bind(&params.name)
        .bind(&params.description)
        .fetch_one(pool)
        .await?;
        Ok(Project::new(
            row.get("id"),
            params.name.clone(),
            params.description.clone(),
            row.get("created_at"),
        ))
    }

    async fn get_project(&self, id: i64) -> Result<Project> {
        let pool = self.pool().await?;
        let row = sqlx::query("SELECT name, description, created_at FROM projects WHERE id = $1")
            .bind(id)
            .fetch_optional(pool)
            .await?
            .ok_or(DomainError::ProjectNotFound)?;
        Ok(Project::new(
            id,
            row.get("name"),
            row.get("description"),
            row.get("created_at"),
        ))
    }

    async fn get_project_by_name(&self, name: &str) -> Result<Project> {
        let pool = self.pool().await?;
        let row = sqlx::query("SELECT id, description, created_at FROM projects WHERE name = $1")
            .bind(name)
            .fetch_optional(pool)
            .await?
            .ok_or(DomainError::ProjectNotFound)?;
        Ok(Project::new(
            row.get("id"),
            name.to_string(),
            row.get("description"),
            row.get("created_at"),
        ))
    }

    async fn delete_project(&self, id: i64) -> Result<()> {
        let pool = self.pool().await?;
        let result = sqlx::query("DELETE FROM projects WHERE id = $1")
            .bind(id)
            .execute(pool)
            .await?;
        if result.rows_affected() == 0 {
            anyhow::bail!("project not found: {id}");
        }
        Ok(())
    }
}

#[async_trait]
impl ProjectMemberRepository for PostgresBackend {
    async fn add_project_member(
        &self,
        project_id: i64,
        params: &AddProjectMemberParams,
    ) -> Result<ProjectMember> {
        let pool = self.pool().await?;
        let row = sqlx::query(
            "INSERT INTO project_members (project_id, user_id, role) VALUES ($1, $2, $3) RETURNING id, created_at",
        )
        .bind(project_id)
        .bind(params.user_id)
        .bind(params.role.to_string())
        .fetch_one(pool)
        .await?;
        Ok(ProjectMember::new(
            row.get("id"),
            project_id,
            params.user_id,
            params.role,
            row.get("created_at"),
        ))
    }

    async fn remove_project_member(&self, project_id: i64, user_id: i64) -> Result<()> {
        let pool = self.pool().await?;
        let result =
            sqlx::query("DELETE FROM project_members WHERE project_id = $1 AND user_id = $2")
                .bind(project_id)
                .bind(user_id)
                .execute(pool)
                .await?;
        if result.rows_affected() == 0 {
            anyhow::bail!("project member not found: project_id={project_id}, user_id={user_id}");
        }
        Ok(())
    }

    async fn list_project_members(&self, project_id: i64) -> Result<Vec<ProjectMember>> {
        let pool = self.pool().await?;
        let rows = sqlx::query(
            "SELECT id, user_id, role, created_at FROM project_members WHERE project_id = $1 ORDER BY id",
        )
        .bind(project_id)
        .fetch_all(pool)
        .await?;
        rows.into_iter()
            .map(|r| {
                let role_str: String = r.get("role");
                let role: Role = role_str
                    .parse()
                    .map_err(|e| anyhow::anyhow!("invalid role in database: {e}"))?;
                Ok(ProjectMember::new(
                    r.get("id"),
                    project_id,
                    r.get("user_id"),
                    role,
                    r.get("created_at"),
                ))
            })
            .collect()
    }

    async fn get_project_member(&self, project_id: i64, user_id: i64) -> Result<ProjectMember> {
        let pool = self.pool().await?;
        let row = sqlx::query(
            "SELECT id, role, created_at FROM project_members WHERE project_id = $1 AND user_id = $2",
        )
        .bind(project_id)
        .bind(user_id)
        .fetch_optional(pool)
        .await?
        .ok_or(DomainError::ProjectMemberNotFound)?;
        let role_str: String = row.get("role");
        let role: Role = role_str.parse()?;
        Ok(ProjectMember::new(
            row.get("id"),
            project_id,
            user_id,
            role,
            row.get("created_at"),
        ))
    }

    async fn update_member_role(
        &self,
        project_id: i64,
        user_id: i64,
        role: Role,
    ) -> Result<ProjectMember> {
        let pool = self.pool().await?;
        let result = sqlx::query(
            "UPDATE project_members SET role = $3 WHERE project_id = $1 AND user_id = $2",
        )
        .bind(project_id)
        .bind(user_id)
        .bind(role.to_string())
        .execute(pool)
        .await?;
        if result.rows_affected() == 0 {
            anyhow::bail!("project member not found: project_id={project_id}, user_id={user_id}");
        }
        self.get_project_member(project_id, user_id).await
    }
}

#[async_trait]
impl UserRepository for PostgresBackend {
    async fn create_user(&self, params: &CreateUserParams) -> Result<User> {
        let pool = self.pool().await?;
        let effective_sub = params.sub.as_deref().unwrap_or(&params.username);
        let row = sqlx::query(
            "INSERT INTO users (username, sub, display_name, email) VALUES ($1, $2, $3, $4) RETURNING id, created_at",
        )
        .bind(&params.username)
        .bind(effective_sub)
        .bind(&params.display_name)
        .bind(&params.email)
        .fetch_one(pool)
        .await?;
        Ok(User::new(
            row.get("id"),
            params.username.clone(),
            effective_sub.to_string(),
            params.display_name.clone(),
            params.email.clone(),
            row.get("created_at"),
        ))
    }

    async fn get_user(&self, id: i64) -> Result<User> {
        let pool = self.pool().await?;
        let row = sqlx::query(
            "SELECT username, sub, display_name, email, created_at FROM users WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(pool)
        .await?
        .ok_or(DomainError::UserNotFound)?;
        Ok(User::new(
            id,
            row.get("username"),
            row.get("sub"),
            row.get("display_name"),
            row.get("email"),
            row.get("created_at"),
        ))
    }

    async fn get_user_by_username(&self, username: &str) -> Result<User> {
        let pool = self.pool().await?;
        let row = sqlx::query(
            "SELECT id, sub, display_name, email, created_at FROM users WHERE username = $1",
        )
        .bind(username)
        .fetch_optional(pool)
        .await?
        .ok_or(DomainError::UserNotFound)?;
        Ok(User::new(
            row.get("id"),
            username.to_string(),
            row.get("sub"),
            row.get("display_name"),
            row.get("email"),
            row.get("created_at"),
        ))
    }

    async fn get_user_by_sub(&self, sub: &str) -> Result<User> {
        let pool = self.pool().await?;
        let row = sqlx::query(
            "SELECT id, username, display_name, email, created_at FROM users WHERE sub = $1",
        )
        .bind(sub)
        .fetch_optional(pool)
        .await?
        .ok_or(DomainError::UserNotFound)?;
        Ok(User::new(
            row.get("id"),
            row.get("username"),
            sub.to_string(),
            row.get("display_name"),
            row.get("email"),
            row.get("created_at"),
        ))
    }

    async fn update_user(&self, id: i64, params: &UpdateUserParams) -> Result<User> {
        let pool = self.pool().await?;

        let mut sets = Vec::new();
        let mut bind_idx = 1u32;
        if params.username.is_some() {
            sets.push(format!("username = ${bind_idx}"));
            bind_idx += 1;
        }
        if params.display_name.is_some() {
            sets.push(format!("display_name = ${bind_idx}"));
            bind_idx += 1;
        }

        if sets.is_empty() {
            return self.get_user(id).await;
        }

        let sql = format!(
            "UPDATE users SET {} WHERE id = ${bind_idx} RETURNING id, username, sub, display_name, email, created_at",
            sets.join(", "),
        );

        let mut query = sqlx::query(&sql);
        if let Some(ref username) = params.username {
            query = query.bind(username);
        }
        if let Some(ref display_name) = params.display_name {
            query = query.bind(display_name);
        }
        query = query.bind(id);

        let row = query
            .fetch_optional(pool)
            .await?
            .ok_or(DomainError::UserNotFound)?;

        Ok(User::new(
            row.get("id"),
            row.get("username"),
            row.get("sub"),
            row.get("display_name"),
            row.get("email"),
            row.get("created_at"),
        ))
    }

    async fn delete_user(&self, id: i64) -> Result<()> {
        let pool = self.pool().await?;
        let result = sqlx::query("DELETE FROM users WHERE id = $1")
            .bind(id)
            .execute(pool)
            .await?;
        if result.rows_affected() == 0 {
            anyhow::bail!("user not found: {id}");
        }
        Ok(())
    }
}

#[async_trait]
impl AuthenticationPort for PostgresBackend {
    async fn get_user_by_api_key(
        &self,
        key_hash: &str,
    ) -> Result<crate::application::port::ApiKeyAuthResult> {
        let pool = self.pool().await?;

        sqlx::query("UPDATE api_keys SET last_used_at = $2 WHERE key_hash = $1")
            .bind(&key_hash)
            .bind(now_utc())
            .execute(pool)
            .await?;

        let row = sqlx::query(
            "SELECT user_id, created_at, last_used_at FROM api_keys WHERE key_hash = $1",
        )
        .bind(&key_hash)
        .fetch_optional(pool)
        .await?
        .context("invalid api key")?;
        let user_id: i64 = row.get("user_id");
        let key_created_at: String = row.get("created_at");
        let key_last_used_at: Option<String> = row.get("last_used_at");
        let user = self.get_user(user_id).await?;
        Ok(crate::application::port::ApiKeyAuthResult {
            user,
            key_created_at,
            key_last_used_at,
        })
    }
}

#[async_trait]
impl ApiKeyRepository for PostgresBackend {
    async fn create_api_key(
        &self,
        user_id: i64,
        name: &str,
        device_name: Option<&str>,
        new_key: &NewApiKey,
    ) -> Result<ApiKeyWithSecret> {
        let pool = self.pool().await?;
        // Verify user exists
        self.get_user(user_id).await?;

        let row = sqlx::query(
            "INSERT INTO api_keys (user_id, key_hash, key_prefix, name, device_name) VALUES ($1, $2, $3, $4, $5) RETURNING id, created_at",
        )
        .bind(user_id)
        .bind(&new_key.key_hash)
        .bind(&new_key.key_prefix)
        .bind(name)
        .bind(device_name)
        .fetch_one(pool)
        .await?;

        Ok(ApiKeyWithSecret::new(
            row.get("id"),
            user_id,
            new_key.raw_key.clone(),
            new_key.key_prefix.clone(),
            name.to_string(),
            device_name.map(String::from),
            row.get("created_at"),
        ))
    }

    async fn delete_api_key(&self, key_id: i64) -> Result<()> {
        let pool = self.pool().await?;
        let result = sqlx::query("DELETE FROM api_keys WHERE id = $1")
            .bind(key_id)
            .execute(pool)
            .await?;
        if result.rows_affected() == 0 {
            anyhow::bail!("api key not found: {key_id}");
        }
        Ok(())
    }

    async fn delete_api_key_for_user(&self, key_id: i64, user_id: i64) -> Result<()> {
        let pool = self.pool().await?;
        let result = sqlx::query("DELETE FROM api_keys WHERE id = $1 AND user_id = $2")
            .bind(key_id)
            .bind(user_id)
            .execute(pool)
            .await?;
        if result.rows_affected() == 0 {
            anyhow::bail!("api key not found: {key_id}");
        }
        Ok(())
    }

    async fn delete_all_api_keys_for_user(&self, user_id: i64) -> Result<()> {
        let pool = self.pool().await?;
        sqlx::query("DELETE FROM api_keys WHERE user_id = $1")
            .bind(user_id)
            .execute(pool)
            .await?;
        Ok(())
    }
}

// =============================================================================
// ProjectQueryPort / UserQueryPort
// =============================================================================

#[async_trait]
impl ProjectQueryPort for PostgresBackend {
    async fn list_projects(&self) -> Result<Vec<Project>> {
        let pool = self.pool().await?;
        let rows =
            sqlx::query("SELECT id, name, description, created_at FROM projects ORDER BY id")
                .fetch_all(pool)
                .await?;
        Ok(rows
            .into_iter()
            .map(|r| {
                Project::new(
                    r.get("id"),
                    r.get("name"),
                    r.get("description"),
                    r.get("created_at"),
                )
            })
            .collect())
    }
}

#[async_trait]
impl UserQueryPort for PostgresBackend {
    async fn list_users(&self) -> Result<Vec<User>> {
        let pool = self.pool().await?;
        let rows = sqlx::query(
            "SELECT id, username, sub, display_name, email, created_at FROM users ORDER BY id",
        )
        .fetch_all(pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|r| {
                User::new(
                    r.get("id"),
                    r.get("username"),
                    r.get("sub"),
                    r.get("display_name"),
                    r.get("email"),
                    r.get("created_at"),
                )
            })
            .collect())
    }

    async fn list_api_keys(&self, user_id: i64) -> Result<Vec<ApiKey>> {
        let pool = self.pool().await?;
        let rows = sqlx::query(
            "SELECT id, user_id, key_prefix, name, device_name, created_at, last_used_at FROM api_keys WHERE user_id = $1 ORDER BY id",
        )
        .bind(user_id)
        .fetch_all(pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|r| {
                ApiKey::new(
                    r.get("id"),
                    r.get("user_id"),
                    r.get("key_prefix"),
                    r.get("name"),
                    r.get("device_name"),
                    r.get("created_at"),
                    r.get("last_used_at"),
                )
            })
            .collect())
    }
}

// =============================================================================
// TaskRepository
// =============================================================================

#[async_trait]
impl TaskRepository for PostgresBackend {
    async fn create_task(&self, project_id: i64, params: &CreateTaskParams) -> Result<Task> {
        let pool = self.pool().await?;
        // Verify project exists
        self.get_project(project_id).await?;

        let priority: i32 = params.priority.unwrap_or(Priority::P2).into();
        let metadata_str = params
            .metadata
            .as_ref()
            .map(|v| serde_json::to_string(v))
            .transpose()?;

        let mut tx = pool.begin().await?;

        let row = sqlx::query(
            "INSERT INTO tasks (title, background, description, priority, branch, pr_url, metadata, project_id, task_number, assignee_user_id, contract_id)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, (SELECT COALESCE(MAX(task_number), 0) + 1 FROM tasks WHERE project_id = $8), $9, $10)
             RETURNING id, task_number",
        )
        .bind(&params.title)
        .bind(&params.background)
        .bind(&params.description)
        .bind(priority)
        .bind(&params.branch)
        .bind(&params.pr_url)
        .bind(&metadata_str)
        .bind(project_id)
        .bind(params.assignee_user_id.as_ref().and_then(|a| a.as_id()))
        .bind(params.contract_id)
        .fetch_one(&mut *tx)
        .await?;
        let task_id: i64 = row.get("id");
        let task_number: i64 = row.get("task_number");

        if let Some(ref branch) = params.branch {
            if branch.contains("${task_id}") {
                let expanded = task::expand_branch_template(branch, task_number);
                sqlx::query("UPDATE tasks SET branch = $1 WHERE id = $2")
                    .bind(&expanded)
                    .bind(task_id)
                    .execute(&mut *tx)
                    .await?;
            }
        }

        for item in &params.definition_of_done {
            sqlx::query("INSERT INTO task_definition_of_done (task_id, content) VALUES ($1, $2)")
                .bind(task_id)
                .bind(item)
                .execute(&mut *tx)
                .await?;
        }
        for item in &params.in_scope {
            sqlx::query("INSERT INTO task_in_scope (task_id, content) VALUES ($1, $2)")
                .bind(task_id)
                .bind(item)
                .execute(&mut *tx)
                .await?;
        }
        for item in &params.out_of_scope {
            sqlx::query("INSERT INTO task_out_of_scope (task_id, content) VALUES ($1, $2)")
                .bind(task_id)
                .bind(item)
                .execute(&mut *tx)
                .await?;
        }
        for tag in &params.tags {
            sqlx::query("INSERT INTO task_tags (task_id, tag) VALUES ($1, $2)")
                .bind(task_id)
                .bind(tag)
                .execute(&mut *tx)
                .await?;
        }
        for &dep_task_number in &params.dependencies {
            let dep_internal_id =
                resolve_task_number(&mut *tx, project_id, dep_task_number).await?;
            sqlx::query(
                "INSERT INTO task_dependencies (task_id, depends_on_task_id) VALUES ($1, $2)",
            )
            .bind(task_id)
            .bind(dep_internal_id)
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;
        get_task_by_id(pool, task_id).await
    }

    async fn get_task(&self, project_id: i64, id: i64) -> Result<Task> {
        let pool = self.pool().await?;
        let internal_id = resolve_task_number(pool, project_id, id).await?;
        get_task_by_id(pool, internal_id).await
    }

    async fn update_task(
        &self,
        project_id: i64,
        id: i64,
        params: &UpdateTaskParams,
    ) -> Result<Task> {
        let pool = self.pool().await?;
        let id = resolve_task_number(pool, project_id, id).await?;

        let mut tx = pool.begin().await?;

        if let Some(ref title) = params.title {
            sqlx::query("UPDATE tasks SET title = $1, updated_at = $2 WHERE id = $3")
                .bind(title)
                .bind(now_utc())
                .bind(id)
                .execute(&mut *tx)
                .await?;
        }
        if let Some(ref background) = params.background {
            sqlx::query("UPDATE tasks SET background = $1, updated_at = $2 WHERE id = $3")
                .bind(background)
                .bind(now_utc())
                .bind(id)
                .execute(&mut *tx)
                .await?;
        }
        if let Some(ref description) = params.description {
            sqlx::query("UPDATE tasks SET description = $1, updated_at = $2 WHERE id = $3")
                .bind(description)
                .bind(now_utc())
                .bind(id)
                .execute(&mut *tx)
                .await?;
        }
        if let Some(ref plan) = params.plan {
            sqlx::query("UPDATE tasks SET plan = $1, updated_at = $2 WHERE id = $3")
                .bind(plan)
                .bind(now_utc())
                .bind(id)
                .execute(&mut *tx)
                .await?;
        }
        if let Some(priority) = params.priority {
            sqlx::query("UPDATE tasks SET priority = $1, updated_at = $2 WHERE id = $3")
                .bind(i32::from(priority))
                .bind(now_utc())
                .bind(id)
                .execute(&mut *tx)
                .await?;
        }
        if let Some(ref assignee) = params.assignee_session_id {
            sqlx::query("UPDATE tasks SET assignee_session_id = $1, updated_at = $2 WHERE id = $3")
                .bind(assignee)
                .bind(now_utc())
                .bind(id)
                .execute(&mut *tx)
                .await?;
        }
        if let Some(ref assignee_user_id) = params.assignee_user_id {
            sqlx::query("UPDATE tasks SET assignee_user_id = $1, updated_at = $2 WHERE id = $3")
                .bind(assignee_user_id.as_ref().and_then(|a| a.as_id()))
                .bind(now_utc())
                .bind(id)
                .execute(&mut *tx)
                .await?;
        }
        if let Some(ref started_at) = params.started_at {
            sqlx::query("UPDATE tasks SET started_at = $1, updated_at = $2 WHERE id = $3")
                .bind(started_at)
                .bind(now_utc())
                .bind(id)
                .execute(&mut *tx)
                .await?;
        }
        if let Some(ref completed_at) = params.completed_at {
            sqlx::query("UPDATE tasks SET completed_at = $1, updated_at = $2 WHERE id = $3")
                .bind(completed_at)
                .bind(now_utc())
                .bind(id)
                .execute(&mut *tx)
                .await?;
        }
        if let Some(ref canceled_at) = params.canceled_at {
            sqlx::query("UPDATE tasks SET canceled_at = $1, updated_at = $2 WHERE id = $3")
                .bind(canceled_at)
                .bind(now_utc())
                .bind(id)
                .execute(&mut *tx)
                .await?;
        }
        if let Some(ref cancel_reason) = params.cancel_reason {
            sqlx::query("UPDATE tasks SET cancel_reason = $1, updated_at = $2 WHERE id = $3")
                .bind(cancel_reason)
                .bind(now_utc())
                .bind(id)
                .execute(&mut *tx)
                .await?;
        }
        if let Some(ref branch) = params.branch {
            sqlx::query("UPDATE tasks SET branch = $1, updated_at = $2 WHERE id = $3")
                .bind(branch)
                .bind(now_utc())
                .bind(id)
                .execute(&mut *tx)
                .await?;
        }
        if let Some(ref pr_url) = params.pr_url {
            sqlx::query("UPDATE tasks SET pr_url = $1, updated_at = $2 WHERE id = $3")
                .bind(pr_url)
                .bind(now_utc())
                .bind(id)
                .execute(&mut *tx)
                .await?;
        }
        if let Some(ref contract_id) = params.contract_id {
            sqlx::query("UPDATE tasks SET contract_id = $1, updated_at = $2 WHERE id = $3")
                .bind(*contract_id)
                .bind(now_utc())
                .bind(id)
                .execute(&mut *tx)
                .await?;
        }
        if let Some(ref meta_update) = params.metadata {
            let resolved: Option<serde_json::Value> = match meta_update {
                MetadataUpdate::Clear => None,
                MetadataUpdate::Replace(v) => Some(v.clone()),
                MetadataUpdate::Merge(patch) => {
                    let existing_str: Option<String> =
                        sqlx::query_scalar("SELECT metadata FROM tasks WHERE id = $1")
                            .bind(id)
                            .fetch_one(&mut *tx)
                            .await?;
                    let existing: Option<serde_json::Value> = existing_str
                        .as_deref()
                        .map(serde_json::from_str)
                        .transpose()?;
                    shallow_merge_metadata(existing.as_ref(), patch)
                }
            };
            let metadata_str: Option<String> = resolved
                .as_ref()
                .map(|v| serde_json::to_string(v))
                .transpose()?;
            sqlx::query("UPDATE tasks SET metadata = $1, updated_at = $2 WHERE id = $3")
                .bind(&metadata_str)
                .bind(now_utc())
                .bind(id)
                .execute(&mut *tx)
                .await?;
        }

        tx.commit().await?;

        get_task_by_id(pool, id).await
    }

    async fn update_task_arrays(
        &self,
        project_id: i64,
        id: i64,
        params: &UpdateTaskArrayParams,
    ) -> Result<()> {
        let pool = self.pool().await?;
        let id = resolve_task_number(pool, project_id, id).await?;

        let mut tx = pool.begin().await?;

        // tags
        if let Some(ref values) = params.set_tags {
            sqlx::query("DELETE FROM task_tags WHERE task_id = $1")
                .bind(id)
                .execute(&mut *tx)
                .await?;
            for tag in values {
                sqlx::query("INSERT INTO task_tags (task_id, tag) VALUES ($1, $2)")
                    .bind(id)
                    .bind(tag)
                    .execute(&mut *tx)
                    .await?;
            }
        }
        for tag in &params.add_tags {
            sqlx::query(
                "INSERT INTO task_tags (task_id, tag) VALUES ($1, $2) ON CONFLICT DO NOTHING",
            )
            .bind(id)
            .bind(tag)
            .execute(&mut *tx)
            .await?;
        }
        for tag in &params.remove_tags {
            sqlx::query("DELETE FROM task_tags WHERE task_id = $1 AND tag = $2")
                .bind(id)
                .bind(tag)
                .execute(&mut *tx)
                .await?;
        }

        // definition_of_done
        update_content_array(
            &mut tx,
            id,
            "task_definition_of_done",
            &params.set_definition_of_done,
            &params.add_definition_of_done,
            &params.remove_definition_of_done,
        )
        .await?;
        // in_scope
        update_content_array(
            &mut tx,
            id,
            "task_in_scope",
            &params.set_in_scope,
            &params.add_in_scope,
            &params.remove_in_scope,
        )
        .await?;
        // out_of_scope
        update_content_array(
            &mut tx,
            id,
            "task_out_of_scope",
            &params.set_out_of_scope,
            &params.add_out_of_scope,
            &params.remove_out_of_scope,
        )
        .await?;

        // Touch updated_at if there were changes
        let has_changes = params.set_tags.is_some()
            || !params.add_tags.is_empty()
            || !params.remove_tags.is_empty()
            || params.set_definition_of_done.is_some()
            || !params.add_definition_of_done.is_empty()
            || !params.remove_definition_of_done.is_empty()
            || params.set_in_scope.is_some()
            || !params.add_in_scope.is_empty()
            || !params.remove_in_scope.is_empty()
            || params.set_out_of_scope.is_some()
            || !params.add_out_of_scope.is_empty()
            || !params.remove_out_of_scope.is_empty();

        if has_changes {
            sqlx::query("UPDATE tasks SET updated_at = $1 WHERE id = $2")
                .bind(now_utc())
                .bind(id)
                .execute(&mut *tx)
                .await?;
        }

        tx.commit().await?;
        Ok(())
    }

    async fn delete_task(&self, project_id: i64, id: i64) -> Result<()> {
        let pool = self.pool().await?;
        let id = resolve_task_number(pool, project_id, id).await?;
        let result = sqlx::query("DELETE FROM tasks WHERE id = $1")
            .bind(id)
            .execute(pool)
            .await?;
        if result.rows_affected() == 0 {
            anyhow::bail!("task not found: {id}");
        }
        Ok(())
    }

    async fn list_dependencies(&self, project_id: i64, task_id: i64) -> Result<Vec<Task>> {
        let pool = self.pool().await?;
        let internal_id = resolve_task_number(pool, project_id, task_id).await?;
        get_task_by_id(pool, internal_id).await?;

        let rows =
            sqlx::query("SELECT depends_on_task_id FROM task_dependencies WHERE task_id = $1")
                .bind(internal_id)
                .fetch_all(pool)
                .await?;

        let mut tasks = Vec::with_capacity(rows.len());
        for row in rows {
            let dep_id: i64 = row.get("depends_on_task_id");
            tasks.push(get_task_by_id(pool, dep_id).await?);
        }
        Ok(tasks)
    }

    async fn save(&self, task: &Task) -> Result<()> {
        let pool = self.pool().await?;
        let metadata_str: Option<String> = task
            .metadata()
            .map(|v| serde_json::to_string(v))
            .transpose()
            .map_err(|e| anyhow::anyhow!("failed to serialize metadata: {e}"))?;

        let mut tx = pool.begin().await?;

        sqlx::query(
            "UPDATE tasks SET
                title = $2, background = $3, description = $4, plan = $5,
                priority = $6, status = $7,
                assignee_session_id = $8, assignee_user_id = $9,
                started_at = $10, completed_at = $11, canceled_at = $12, cancel_reason = $13,
                branch = $14, pr_url = $15, metadata = $16, contract_id = $17,
                updated_at = $18
            WHERE id = $1",
        )
        .bind(task.id())
        .bind(task.title())
        .bind(task.background())
        .bind(task.description())
        .bind(task.plan())
        .bind(i32::from(task.priority()))
        .bind(task.status().to_string())
        .bind(task.assignee_session_id())
        .bind(task.assignee_user_id())
        .bind(task.started_at())
        .bind(task.completed_at())
        .bind(task.canceled_at())
        .bind(task.cancel_reason())
        .bind(task.branch())
        .bind(task.pr_url())
        .bind(&metadata_str)
        .bind(task.contract_id())
        .bind(task.updated_at())
        .execute(&mut *tx)
        .await?;

        // Sync definition_of_done
        sqlx::query("DELETE FROM task_definition_of_done WHERE task_id = $1")
            .bind(task.id())
            .execute(&mut *tx)
            .await?;
        for dod in task.definition_of_done() {
            let checked_val: i32 = if dod.checked() { 1 } else { 0 };
            sqlx::query(
                "INSERT INTO task_definition_of_done (task_id, content, checked) VALUES ($1, $2, $3)",
            )
            .bind(task.id())
            .bind(dod.content())
            .bind(checked_val)
            .execute(&mut *tx)
            .await?;
        }

        // Sync dependencies (task.dependencies() contains task_numbers, resolve to internal IDs)
        sqlx::query("DELETE FROM task_dependencies WHERE task_id = $1")
            .bind(task.id())
            .execute(&mut *tx)
            .await?;
        for &dep_task_number in task.dependencies() {
            let dep_internal_id =
                resolve_task_number(&mut *tx, task.project_id(), dep_task_number).await?;
            sqlx::query(
                "INSERT INTO task_dependencies (task_id, depends_on_task_id) VALUES ($1, $2)",
            )
            .bind(task.id())
            .bind(dep_internal_id)
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;
        Ok(())
    }
}

#[async_trait]
impl TaskQueryPort for PostgresBackend {
    async fn list_tasks(&self, project_id: i64, filter: &ListTasksFilter) -> Result<Vec<Task>> {
        let pool = self.pool().await?;

        let mut conditions: Vec<String> = Vec::new();
        let mut param_idx: i32 = 1;

        enum BindVal {
            Int(i64),
            Str(String),
        }
        let mut binds: Vec<BindVal> = Vec::new();

        conditions.push(format!("t.project_id = ${param_idx}"));
        binds.push(BindVal::Int(project_id));
        param_idx += 1;

        if !filter.statuses.is_empty() {
            let placeholders: Vec<String> = filter
                .statuses
                .iter()
                .map(|_| {
                    let p = format!("${param_idx}");
                    binds.push(BindVal::Str(String::new()));
                    param_idx += 1;
                    p
                })
                .collect();
            let base = binds.len() - filter.statuses.len();
            for (i, s) in filter.statuses.iter().enumerate() {
                binds[base + i] = BindVal::Str(s.to_string());
            }
            conditions.push(format!("t.status IN ({})", placeholders.join(", ")));
        }

        if !filter.tags.is_empty() {
            let placeholders: Vec<String> = filter
                .tags
                .iter()
                .map(|_| {
                    let p = format!("${param_idx}");
                    binds.push(BindVal::Str(String::new()));
                    param_idx += 1;
                    p
                })
                .collect();
            let base = binds.len() - filter.tags.len();
            for (i, tag) in filter.tags.iter().enumerate() {
                binds[base + i] = BindVal::Str(tag.clone());
            }
            conditions.push(format!(
                "EXISTS (SELECT 1 FROM task_tags tt WHERE tt.task_id = t.id AND tt.tag IN ({}))",
                placeholders.join(", ")
            ));
        }

        if let Some(dep_id) = filter.depends_on {
            conditions.push(format!(
                "EXISTS (SELECT 1 FROM task_dependencies td WHERE td.task_id = t.id AND td.depends_on_task_id = ${param_idx})"
            ));
            binds.push(BindVal::Int(dep_id));
            param_idx += 1;
        }

        if let Some(contract_id) = filter.contract_id {
            conditions.push(format!("t.contract_id = ${param_idx}"));
            binds.push(BindVal::Int(contract_id));
            param_idx += 1;
        }

        if let Some(id_min) = filter.id_min {
            conditions.push(format!("t.id >= ${param_idx}"));
            binds.push(BindVal::Int(id_min));
            param_idx += 1;
        }

        if let Some(id_max) = filter.id_max {
            conditions.push(format!("t.id <= ${param_idx}"));
            binds.push(BindVal::Int(id_max));
            param_idx += 1;
        }

        // SQL-optimized implementation of `crate::domain::task::filter_ready`.
        if filter.ready {
            conditions.push("t.status = 'todo'".to_string());
            conditions.push(
                "NOT EXISTS (SELECT 1 FROM task_dependencies td JOIN tasks dep ON dep.id = td.depends_on_task_id WHERE td.task_id = t.id AND dep.status != 'completed')"
                    .to_string(),
            );
        }

        if let Some(uid) = filter.assignee_user_id {
            if filter.include_unassigned {
                conditions.push(format!(
                    "(t.assignee_user_id = ${param_idx} OR t.assignee_user_id IS NULL)"
                ));
            } else {
                conditions.push(format!("t.assignee_user_id = ${param_idx}"));
            }
            binds.push(BindVal::Int(uid));
            param_idx += 1;
        }

        if !filter.metadata.is_empty() {
            let json_str = serde_json::to_string(&serde_json::Value::Object(
                filter
                    .metadata
                    .iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect(),
            ))
            .context("failed to serialize metadata filter")?;
            conditions.push(format!("t.metadata::jsonb @> ${param_idx}::jsonb"));
            binds.push(BindVal::Str(json_str));
            param_idx += 1;
        }

        let where_clause = if conditions.is_empty() {
            String::new()
        } else {
            format!(" WHERE {}", conditions.join(" AND "))
        };

        let mut sql = format!("SELECT t.id FROM tasks t{where_clause} ORDER BY t.id");
        if let Some(l) = filter.limit {
            sql.push_str(&format!(" LIMIT ${param_idx}"));
            binds.push(BindVal::Int(l as i64));
            param_idx += 1;
        }
        if let Some(o) = filter.offset {
            sql.push_str(&format!(" OFFSET ${param_idx}"));
            binds.push(BindVal::Int(o as i64));
            #[allow(unused_assignments)]
            {
                param_idx += 1;
            }
        }

        let mut query = sqlx::query(&sql);
        for bind in &binds {
            match bind {
                BindVal::Int(v) => query = query.bind(v),
                BindVal::Str(v) => query = query.bind(v),
            }
        }

        let rows = query.fetch_all(pool).await?;
        let ids: Vec<i64> = rows.iter().map(|r| r.get("id")).collect();

        let mut tasks = Vec::with_capacity(ids.len());
        for id in ids {
            tasks.push(get_task_by_id(pool, id).await?);
        }
        Ok(tasks)
    }

    /// SQL-optimized implementation of [`crate::domain::task::select_next`].
    async fn next_task(
        &self,
        project_id: i64,
        user_id: Option<i64>,
        include_unassigned: bool,
    ) -> Result<Option<Task>> {
        let pool = self.pool().await?;
        let assignee_clause = match user_id {
            Some(_) if include_unassigned => {
                " AND (t.assignee_user_id = $2 OR t.assignee_user_id IS NULL)"
            }
            Some(_) => " AND t.assignee_user_id = $2",
            None => "",
        };
        let sql = format!(
            "SELECT t.id FROM tasks t
             WHERE t.project_id = $1
               AND t.status = 'todo'
               AND NOT EXISTS (
                 SELECT 1 FROM task_dependencies td
                 JOIN tasks dep ON dep.id = td.depends_on_task_id
                 WHERE td.task_id = t.id AND dep.status != 'completed'
               ){assignee_clause}
             ORDER BY t.priority ASC, t.created_at ASC, t.id ASC
             LIMIT 1"
        );
        let mut query = sqlx::query(&sql).bind(project_id);
        if let Some(uid) = user_id {
            query = query.bind(uid);
        }
        let row = query.fetch_optional(pool).await?;
        match row {
            Some(r) => {
                let id: i64 = r.get("id");
                Ok(Some(get_task_by_id(pool, id).await?))
            }
            None => Ok(None),
        }
    }

    async fn task_stats(&self, project_id: i64) -> Result<HashMap<String, i64>> {
        let pool = self.pool().await?;
        let rows = sqlx::query(
            "SELECT status, COUNT(*) as cnt FROM tasks WHERE project_id = $1 GROUP BY status",
        )
        .bind(project_id)
        .fetch_all(pool)
        .await?;
        let mut stats = HashMap::new();
        for row in rows {
            let status: String = row.get("status");
            let count: i64 = row.get("cnt");
            stats.insert(status, count);
        }
        Ok(stats)
    }

    /// SQL-optimized implementation of ready-count, equivalent to
    /// `crate::domain::task::filter_ready(...).len()`.
    async fn ready_count(&self, project_id: i64) -> Result<i64> {
        let pool = self.pool().await?;
        let row = sqlx::query(
            "SELECT COUNT(*) as cnt FROM tasks t
             WHERE t.project_id = $1
               AND t.status = 'todo'
               AND NOT EXISTS (
                 SELECT 1 FROM task_dependencies td
                 JOIN tasks dep ON dep.id = td.depends_on_task_id
                 WHERE td.task_id = t.id AND dep.status != 'completed'
               )",
        )
        .bind(project_id)
        .fetch_one(pool)
        .await?;
        Ok(row.get("cnt"))
    }

    async fn list_ready_tasks(&self, project_id: i64) -> Result<Vec<Task>> {
        let filter = ListTasksFilter {
            ready: true,
            ..Default::default()
        };
        self.list_tasks(project_id, &filter).await
    }

    async fn is_task_ready(&self, project_id: i64, task_id: i64) -> Result<bool> {
        let pool = self.pool().await?;
        let row: Option<(i32,)> = sqlx::query_as(
            "SELECT 1 FROM tasks t
             WHERE t.project_id = $1
               AND t.id = $2
               AND t.status = 'todo'
               AND NOT EXISTS (
                 SELECT 1 FROM task_dependencies td
                 JOIN tasks dep ON dep.id = td.depends_on_task_id
                 WHERE td.task_id = t.id AND dep.status != 'completed'
               )
             LIMIT 1",
        )
        .bind(project_id)
        .bind(task_id)
        .fetch_optional(pool)
        .await?;
        Ok(row.is_some())
    }
}

// --- Helper for update_task_arrays ---

async fn update_content_array(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    task_id: i64,
    table: &str,
    set: &Option<Vec<String>>,
    add: &[String],
    remove: &[String],
) -> Result<()> {
    if let Some(values) = set {
        sqlx::query(&format!("DELETE FROM {table} WHERE task_id = $1"))
            .bind(task_id)
            .execute(&mut **tx)
            .await?;
        for item in values {
            sqlx::query(&format!(
                "INSERT INTO {table} (task_id, content) VALUES ($1, $2)"
            ))
            .bind(task_id)
            .bind(item)
            .execute(&mut **tx)
            .await?;
        }
    }
    for item in add {
        sqlx::query(&format!(
            "INSERT INTO {table} (task_id, content) VALUES ($1, $2)"
        ))
        .bind(task_id)
        .bind(item)
        .execute(&mut **tx)
        .await?;
    }
    for item in remove {
        sqlx::query(&format!(
            "DELETE FROM {table} WHERE task_id = $1 AND content = $2"
        ))
        .bind(task_id)
        .bind(item)
        .execute(&mut **tx)
        .await?;
    }
    Ok(())
}

#[async_trait]
impl MetadataFieldRepository for PostgresBackend {
    async fn create_metadata_field(
        &self,
        project_id: i64,
        params: &CreateMetadataFieldParams,
    ) -> Result<MetadataField> {
        let pool = self.pool().await?;
        let result = sqlx::query(
            "INSERT INTO metadata_fields (project_id, name, field_type, required_on_complete, description)
             VALUES ($1, $2, $3, $4, $5)
             RETURNING id, created_at",
        )
        .bind(project_id)
        .bind(&params.name)
        .bind(params.field_type.to_string())
        .bind(params.required_on_complete)
        .bind(&params.description)
        .fetch_one(pool)
        .await;

        match result {
            Ok(row) => Ok(MetadataField::new(
                row.get("id"),
                project_id,
                params.name.clone(),
                params.field_type,
                params.required_on_complete,
                params.description.clone(),
                row.get("created_at"),
            )),
            Err(sqlx::Error::Database(db_err)) if db_err.code().as_deref() == Some("23505") => {
                Err(DomainError::MetadataFieldNameConflict {
                    name: params.name.clone(),
                }
                .into())
            }
            Err(e) => Err(e.into()),
        }
    }

    async fn get_metadata_field(&self, project_id: i64, field_id: i64) -> Result<MetadataField> {
        let pool = self.pool().await?;
        let row = sqlx::query(
            "SELECT id, project_id, name, field_type, required_on_complete, description, created_at
             FROM metadata_fields WHERE id = $1 AND project_id = $2",
        )
        .bind(field_id)
        .bind(project_id)
        .fetch_optional(pool)
        .await?
        .ok_or(DomainError::MetadataFieldNotFound)?;

        let field_type_str: String = row.get("field_type");
        let field_type: MetadataFieldType = field_type_str.parse()?;
        Ok(MetadataField::new(
            row.get("id"),
            row.get("project_id"),
            row.get("name"),
            field_type,
            row.get("required_on_complete"),
            row.get("description"),
            row.get("created_at"),
        ))
    }

    async fn list_metadata_fields(&self, project_id: i64) -> Result<Vec<MetadataField>> {
        let pool = self.pool().await?;
        let rows = sqlx::query(
            "SELECT id, project_id, name, field_type, required_on_complete, description, created_at
             FROM metadata_fields WHERE project_id = $1 ORDER BY id",
        )
        .bind(project_id)
        .fetch_all(pool)
        .await?;

        rows.iter()
            .map(|row| {
                let field_type_str: String = row.get("field_type");
                let field_type: MetadataFieldType = field_type_str.parse()?;
                Ok(MetadataField::new(
                    row.get("id"),
                    row.get("project_id"),
                    row.get("name"),
                    field_type,
                    row.get("required_on_complete"),
                    row.get("description"),
                    row.get("created_at"),
                ))
            })
            .collect()
    }

    async fn update_metadata_field(
        &self,
        project_id: i64,
        field_id: i64,
        params: &UpdateMetadataFieldParams,
    ) -> Result<MetadataField> {
        let pool = self.pool().await?;

        // Verify exists
        let _existing = self.get_metadata_field(project_id, field_id).await?;

        if let Some(req) = params.required_on_complete {
            sqlx::query(
                "UPDATE metadata_fields SET required_on_complete = $1 WHERE id = $2 AND project_id = $3",
            )
            .bind(req)
            .bind(field_id)
            .bind(project_id)
            .execute(pool)
            .await?;
        }
        if let Some(ref desc) = params.description {
            sqlx::query(
                "UPDATE metadata_fields SET description = $1 WHERE id = $2 AND project_id = $3",
            )
            .bind(desc.as_deref())
            .bind(field_id)
            .bind(project_id)
            .execute(pool)
            .await?;
        }

        self.get_metadata_field(project_id, field_id).await
    }

    async fn delete_metadata_field(&self, project_id: i64, field_id: i64) -> Result<()> {
        let pool = self.pool().await?;
        let result = sqlx::query("DELETE FROM metadata_fields WHERE id = $1 AND project_id = $2")
            .bind(field_id)
            .bind(project_id)
            .execute(pool)
            .await?;
        if result.rows_affected() == 0 {
            return Err(DomainError::MetadataFieldNotFound.into());
        }
        Ok(())
    }
}

// =============================================================================
// ContractRepository
// =============================================================================

async fn get_contract_by_id(pool: &PgPool, id: i64) -> Result<Contract> {
    let row = sqlx::query(
        "SELECT project_id, title, description, metadata, created_at, updated_at \
         FROM contracts WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?
    .ok_or(DomainError::ContractNotFound)?;

    let metadata: Option<serde_json::Value> = row.get("metadata");

    let definition_of_done: Vec<DodItem> = sqlx::query(
        "SELECT content, checked FROM contract_definition_of_done WHERE contract_id = $1 ORDER BY id",
    )
    .bind(id)
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|r| DodItem::new(r.get("content"), r.get::<i32, _>("checked") != 0))
    .collect();

    let tags: Vec<String> =
        sqlx::query("SELECT tag FROM contract_tags WHERE contract_id = $1 ORDER BY id")
            .bind(id)
            .fetch_all(pool)
            .await?
            .into_iter()
            .map(|r| r.get("tag"))
            .collect();

    let notes: Vec<ContractNote> = sqlx::query(
        "SELECT content, source_task_id, created_at FROM contract_notes WHERE contract_id = $1 ORDER BY id",
    )
    .bind(id)
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|r| ContractNote::new(r.get("content"), r.get("source_task_id"), r.get("created_at")))
    .collect();

    Ok(Contract::new(
        id,
        row.get("project_id"),
        row.get("title"),
        row.get("description"),
        definition_of_done,
        tags,
        metadata,
        notes,
        row.get("created_at"),
        row.get("updated_at"),
    ))
}

#[async_trait]
impl ContractRepository for PostgresBackend {
    async fn create_contract(
        &self,
        project_id: i64,
        params: &CreateContractParams,
    ) -> Result<Contract> {
        let pool = self.pool().await?;
        self.get_project(project_id).await?;

        let mut tx = pool.begin().await?;

        let row = sqlx::query(
            "INSERT INTO contracts (project_id, title, description, metadata) \
             VALUES ($1, $2, $3, $4) RETURNING id",
        )
        .bind(project_id)
        .bind(&params.title)
        .bind(&params.description)
        .bind(&params.metadata)
        .fetch_one(&mut *tx)
        .await?;
        let contract_id: i64 = row.get("id");

        for content in &params.definition_of_done {
            sqlx::query(
                "INSERT INTO contract_definition_of_done (contract_id, content) VALUES ($1, $2)",
            )
            .bind(contract_id)
            .bind(content)
            .execute(&mut *tx)
            .await?;
        }
        for tag in &params.tags {
            sqlx::query("INSERT INTO contract_tags (contract_id, tag) VALUES ($1, $2)")
                .bind(contract_id)
                .bind(tag)
                .execute(&mut *tx)
                .await?;
        }

        tx.commit().await?;
        get_contract_by_id(pool, contract_id).await
    }

    async fn get_contract(&self, id: i64) -> Result<Contract> {
        let pool = self.pool().await?;
        get_contract_by_id(pool, id).await
    }

    async fn list_contracts(&self, project_id: i64) -> Result<Vec<Contract>> {
        let pool = self.pool().await?;
        let ids: Vec<i64> =
            sqlx::query("SELECT id FROM contracts WHERE project_id = $1 ORDER BY id")
                .bind(project_id)
                .fetch_all(pool)
                .await?
                .into_iter()
                .map(|r| r.get("id"))
                .collect();

        let mut result = Vec::with_capacity(ids.len());
        for id in ids {
            result.push(get_contract_by_id(pool, id).await?);
        }
        Ok(result)
    }

    async fn update_contract(
        &self,
        id: i64,
        update: &UpdateContractParams,
        array_update: &UpdateContractArrayParams,
    ) -> Result<Contract> {
        let pool = self.pool().await?;
        let _existing = get_contract_by_id(pool, id).await?;

        let mut tx = pool.begin().await?;
        let mut touched = false;

        if let Some(ref title) = update.title {
            sqlx::query("UPDATE contracts SET title = $1 WHERE id = $2")
                .bind(title)
                .bind(id)
                .execute(&mut *tx)
                .await?;
            touched = true;
        }
        if let Some(ref description) = update.description {
            sqlx::query("UPDATE contracts SET description = $1 WHERE id = $2")
                .bind(description)
                .bind(id)
                .execute(&mut *tx)
                .await?;
            touched = true;
        }
        if let Some(ref meta_update) = update.metadata {
            let resolved: Option<serde_json::Value> = match meta_update {
                MetadataUpdate::Clear => None,
                MetadataUpdate::Replace(v) => Some(v.clone()),
                MetadataUpdate::Merge(patch) => {
                    let existing: Option<serde_json::Value> =
                        sqlx::query_scalar("SELECT metadata FROM contracts WHERE id = $1")
                            .bind(id)
                            .fetch_one(&mut *tx)
                            .await?;
                    shallow_merge_metadata(existing.as_ref(), patch)
                }
            };
            sqlx::query("UPDATE contracts SET metadata = $1 WHERE id = $2")
                .bind(&resolved)
                .bind(id)
                .execute(&mut *tx)
                .await?;
            touched = true;
        }

        // Tags
        if let Some(ref set) = array_update.set_tags {
            sqlx::query("DELETE FROM contract_tags WHERE contract_id = $1")
                .bind(id)
                .execute(&mut *tx)
                .await?;
            for tag in set {
                sqlx::query("INSERT INTO contract_tags (contract_id, tag) VALUES ($1, $2)")
                    .bind(id)
                    .bind(tag)
                    .execute(&mut *tx)
                    .await?;
            }
            touched = true;
        }
        for tag in &array_update.add_tags {
            sqlx::query(
                "INSERT INTO contract_tags (contract_id, tag) VALUES ($1, $2) ON CONFLICT DO NOTHING",
            )
            .bind(id)
            .bind(tag)
            .execute(&mut *tx)
            .await?;
            touched = true;
        }
        for tag in &array_update.remove_tags {
            sqlx::query("DELETE FROM contract_tags WHERE contract_id = $1 AND tag = $2")
                .bind(id)
                .bind(tag)
                .execute(&mut *tx)
                .await?;
            touched = true;
        }

        // DoD
        if let Some(ref set) = array_update.set_definition_of_done {
            sqlx::query("DELETE FROM contract_definition_of_done WHERE contract_id = $1")
                .bind(id)
                .execute(&mut *tx)
                .await?;
            for content in set {
                sqlx::query(
                    "INSERT INTO contract_definition_of_done (contract_id, content) VALUES ($1, $2)",
                )
                .bind(id)
                .bind(content)
                .execute(&mut *tx)
                .await?;
            }
            touched = true;
        }
        for content in &array_update.add_definition_of_done {
            sqlx::query(
                "INSERT INTO contract_definition_of_done (contract_id, content) VALUES ($1, $2)",
            )
            .bind(id)
            .bind(content)
            .execute(&mut *tx)
            .await?;
            touched = true;
        }
        for content in &array_update.remove_definition_of_done {
            sqlx::query(
                "DELETE FROM contract_definition_of_done WHERE contract_id = $1 AND content = $2",
            )
            .bind(id)
            .bind(content)
            .execute(&mut *tx)
            .await?;
            touched = true;
        }

        if touched {
            sqlx::query("UPDATE contracts SET updated_at = $1 WHERE id = $2")
                .bind(now_utc())
                .bind(id)
                .execute(&mut *tx)
                .await?;
        }

        tx.commit().await?;
        get_contract_by_id(pool, id).await
    }

    async fn delete_contract(&self, id: i64) -> Result<()> {
        let pool = self.pool().await?;
        let result = sqlx::query("DELETE FROM contracts WHERE id = $1")
            .bind(id)
            .execute(pool)
            .await?;
        if result.rows_affected() == 0 {
            return Err(DomainError::ContractNotFound.into());
        }
        Ok(())
    }

    async fn add_note(&self, contract_id: i64, note: &ContractNote) -> Result<ContractNote> {
        let pool = self.pool().await?;
        let _existing = get_contract_by_id(pool, contract_id).await?;

        let mut tx = pool.begin().await?;
        sqlx::query(
            "INSERT INTO contract_notes (contract_id, content, source_task_id, created_at) \
             VALUES ($1, $2, $3, $4)",
        )
        .bind(contract_id)
        .bind(note.content())
        .bind(note.source_task_id())
        .bind(note.created_at())
        .execute(&mut *tx)
        .await?;
        sqlx::query("UPDATE contracts SET updated_at = $1 WHERE id = $2")
            .bind(now_utc())
            .bind(contract_id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;

        Ok(ContractNote::new(
            note.content().to_string(),
            note.source_task_id(),
            note.created_at().to_string(),
        ))
    }

    async fn check_dod(&self, contract_id: i64, index: usize) -> Result<Contract> {
        set_contract_dod_checked_pg(self, contract_id, index, true).await
    }

    async fn uncheck_dod(&self, contract_id: i64, index: usize) -> Result<Contract> {
        set_contract_dod_checked_pg(self, contract_id, index, false).await
    }
}

async fn set_contract_dod_checked_pg(
    backend: &PostgresBackend,
    contract_id: i64,
    index: usize,
    checked: bool,
) -> Result<Contract> {
    let pool = backend.pool().await?;
    let contract = get_contract_by_id(pool, contract_id).await?;
    let dod_len = contract.definition_of_done().len();
    if index == 0 || index > dod_len {
        return Err(DomainError::DodIndexOutOfRange {
            index,
            task_id: contract_id,
            count: dod_len,
        }
        .into());
    }

    let dod_row_id: i64 = sqlx::query_scalar(
        "SELECT id FROM contract_definition_of_done WHERE contract_id = $1 ORDER BY id OFFSET $2 LIMIT 1",
    )
    .bind(contract_id)
    .bind((index - 1) as i64)
    .fetch_one(pool)
    .await?;

    let mut tx = pool.begin().await?;
    sqlx::query("UPDATE contract_definition_of_done SET checked = $1 WHERE id = $2")
        .bind(if checked { 1i32 } else { 0i32 })
        .bind(dod_row_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("UPDATE contracts SET updated_at = $1 WHERE id = $2")
        .bind(now_utc())
        .bind(contract_id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;

    get_contract_by_id(pool, contract_id).await
}

crate::impl_task_transition_default!(PostgresBackend);

#[cfg(test)]
mod tests {
    use super::*;

    fn test_url() -> Option<String> {
        std::env::var("SENKO_TEST_POSTGRES_URL").ok()
    }

    async fn setup() -> PostgresBackend {
        let url = test_url().expect("SENKO_TEST_POSTGRES_URL must be set for postgres tests");
        let backend = PostgresBackend::new(url, None);
        let pool = backend.pool().await.unwrap();

        // Clean all data for test isolation (reverse FK order)
        sqlx::query("DELETE FROM metadata_fields")
            .execute(pool)
            .await
            .unwrap();
        sqlx::query("DELETE FROM task_dependencies")
            .execute(pool)
            .await
            .unwrap();
        sqlx::query("DELETE FROM task_definition_of_done")
            .execute(pool)
            .await
            .unwrap();
        sqlx::query("DELETE FROM task_in_scope")
            .execute(pool)
            .await
            .unwrap();
        sqlx::query("DELETE FROM task_out_of_scope")
            .execute(pool)
            .await
            .unwrap();
        sqlx::query("DELETE FROM task_tags")
            .execute(pool)
            .await
            .unwrap();
        sqlx::query("DELETE FROM api_keys")
            .execute(pool)
            .await
            .unwrap();
        sqlx::query("DELETE FROM project_members")
            .execute(pool)
            .await
            .unwrap();
        sqlx::query("DELETE FROM tasks")
            .execute(pool)
            .await
            .unwrap();
        sqlx::query("DELETE FROM users")
            .execute(pool)
            .await
            .unwrap();
        sqlx::query("DELETE FROM projects")
            .execute(pool)
            .await
            .unwrap();

        // Re-seed defaults
        sqlx::query(
            "INSERT INTO projects (id, name, description) VALUES (1, 'default', 'Default project')",
        )
        .execute(pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO users (id, username, display_name) VALUES (1, 'default', 'Default User')",
        )
        .execute(pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO project_members (project_id, user_id, role) VALUES (1, 1, 'owner')",
        )
        .execute(pool)
        .await
        .unwrap();

        // Reset sequences
        sqlx::query(
            "SELECT setval('projects_id_seq', GREATEST((SELECT MAX(id) FROM projects), 1))",
        )
        .execute(pool)
        .await
        .unwrap();
        sqlx::query("SELECT setval('users_id_seq', GREATEST((SELECT MAX(id) FROM users), 1))")
            .execute(pool)
            .await
            .unwrap();
        sqlx::query(
            "SELECT setval('tasks_id_seq', GREATEST((SELECT COALESCE(MAX(id), 0) FROM tasks), 1))",
        )
        .execute(pool)
        .await
        .unwrap();

        backend
    }

    fn params(title: &str) -> CreateTaskParams {
        CreateTaskParams {
            title: title.to_string(),
            background: None,
            description: None,
            priority: None,
            definition_of_done: vec![],
            in_scope: vec![],
            out_of_scope: vec![],
            branch: None,
            pr_url: None,
            metadata: None,
            tags: vec![],
            dependencies: vec![],
            assignee_user_id: None,
            contract_id: None,
        }
    }

    #[tokio::test]
    async fn test_create_and_get_task() {
        if test_url().is_none() {
            return;
        }
        let backend = setup().await;
        let task = backend.create_task(1, &params("Test task")).await.unwrap();
        assert_eq!(task.title(), "Test task");
        assert_eq!(task.status(), TaskStatus::Draft);
        assert_eq!(task.priority(), Priority::P2);

        let fetched = backend.get_task(1, task.id()).await.unwrap();
        assert_eq!(fetched.id(), task.id());
        assert_eq!(fetched.title(), "Test task");
    }

    #[tokio::test]
    async fn test_task_lifecycle() {
        if test_url().is_none() {
            return;
        }
        let backend = setup().await;

        let task = backend
            .create_task(1, &params("Lifecycle test"))
            .await
            .unwrap();
        assert_eq!(task.status(), TaskStatus::Draft);

        // Draft → Todo
        let (task, _) = task.publish(now_utc()).unwrap();
        backend.save(&task).await.unwrap();
        let task = backend.get_task(1, task.id()).await.unwrap();
        assert_eq!(task.status(), TaskStatus::Todo);

        // Todo → InProgress
        let (task, _) = task.start(None, None, now_utc(), None).unwrap();
        backend.save(&task).await.unwrap();
        let task = backend.get_task(1, task.id()).await.unwrap();
        assert_eq!(task.status(), TaskStatus::InProgress);

        // InProgress → Completed
        let (task, _) = task.complete(now_utc()).unwrap();
        backend.save(&task).await.unwrap();
        let task = backend.get_task(1, task.id()).await.unwrap();
        assert_eq!(task.status(), TaskStatus::Completed);
    }

    #[tokio::test]
    async fn test_task_with_dod_and_tags() {
        if test_url().is_none() {
            return;
        }
        let backend = setup().await;

        let mut p = params("DoD test");
        p.definition_of_done = vec!["Write tests".to_string(), "Review code".to_string()];
        p.tags = vec!["backend".to_string(), "postgres".to_string()];

        let task = backend.create_task(1, &p).await.unwrap();
        assert_eq!(task.definition_of_done().len(), 2);
        assert_eq!(task.definition_of_done()[0].content(), "Write tests");
        assert!(!task.definition_of_done()[0].checked());
        assert_eq!(task.tags().len(), 2);
        assert!(task.tags().contains(&"backend".to_string()));
    }

    #[tokio::test]
    async fn test_dependencies() {
        if test_url().is_none() {
            return;
        }
        let backend = setup().await;

        let t1 = backend.create_task(1, &params("Task 1")).await.unwrap();
        let t2 = backend.create_task(1, &params("Task 2")).await.unwrap();

        let (t2, _) = t2.add_dependency(t1.id(), Some(now_utc())).unwrap();
        backend.save(&t2).await.unwrap();
        let t2 = backend.get_task(1, t2.id()).await.unwrap();
        assert_eq!(t2.dependencies(), vec![t1.id()]);

        let deps = backend.list_dependencies(1, t2.id()).await.unwrap();
        assert_eq!(deps.len(), 1);
        assert_eq!(deps[0].id(), t1.id());

        let (t2, _) = t2.remove_dependency(t1.id(), Some(now_utc())).unwrap();
        backend.save(&t2).await.unwrap();
        let t2 = backend.get_task(1, t2.id()).await.unwrap();
        assert!(t2.dependencies().is_empty());
    }

    #[tokio::test]
    async fn test_is_task_ready() {
        if test_url().is_none() {
            return;
        }
        let backend = setup().await;

        // Draft → false
        let draft = backend.create_task(1, &params("Draft")).await.unwrap();
        assert!(!backend.is_task_ready(1, draft.id()).await.unwrap());

        // Todo, no deps → true
        let (free, _) = draft.ready(now_utc()).unwrap();
        backend.save(&free).await.unwrap();
        assert!(backend.is_task_ready(1, free.id()).await.unwrap());

        // In-progress → false
        let (wip, _) = free.start(None, None, now_utc(), None).unwrap();
        backend.save(&wip).await.unwrap();
        assert!(!backend.is_task_ready(1, wip.id()).await.unwrap());

        // Completed → false
        let (done, _) = wip.complete(now_utc()).unwrap();
        backend.save(&done).await.unwrap();
        assert!(!backend.is_task_ready(1, done.id()).await.unwrap());

        // Todo with completed dep → true
        let unblocked_raw = backend.create_task(1, &params("Unblocked")).await.unwrap();
        let (unblocked_raw, _) = unblocked_raw
            .add_dependency(done.id(), Some(now_utc()))
            .unwrap();
        backend.save(&unblocked_raw).await.unwrap();
        let (unblocked, _) = unblocked_raw.ready(now_utc()).unwrap();
        backend.save(&unblocked).await.unwrap();
        assert!(backend.is_task_ready(1, unblocked.id()).await.unwrap());

        // Todo with incomplete dep → false
        let dep = backend.create_task(1, &params("Dep")).await.unwrap();
        let blocked_raw = backend.create_task(1, &params("Blocked")).await.unwrap();
        let (blocked_raw, _) = blocked_raw
            .add_dependency(dep.id(), Some(now_utc()))
            .unwrap();
        backend.save(&blocked_raw).await.unwrap();
        let (blocked, _) = blocked_raw.ready(now_utc()).unwrap();
        backend.save(&blocked).await.unwrap();
        assert!(!backend.is_task_ready(1, blocked.id()).await.unwrap());

        // Missing task → false
        assert!(!backend.is_task_ready(1, 999_999).await.unwrap());
    }

    #[tokio::test]
    async fn test_list_tasks_with_filter() {
        if test_url().is_none() {
            return;
        }
        let backend = setup().await;

        let t1 = backend.create_task(1, &params("Todo task")).await.unwrap();
        let (t1, _) = t1.publish(now_utc()).unwrap();
        backend.save(&t1).await.unwrap();

        let _t2 = backend.create_task(1, &params("Draft task")).await.unwrap();

        let filter = ListTasksFilter {
            statuses: vec![TaskStatus::Todo],
            ..Default::default()
        };
        let tasks = backend.list_tasks(1, &filter).await.unwrap();
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].title(), "Todo task");
    }

    #[tokio::test]
    async fn test_next_task() {
        if test_url().is_none() {
            return;
        }
        let backend = setup().await;

        // No tasks → None
        let next = backend.next_task(1, None, false).await.unwrap();
        assert!(next.is_none());

        let t1 = backend
            .create_task(1, &params("High priority"))
            .await
            .unwrap();
        let (t1, _) = t1.publish(now_utc()).unwrap();
        backend.save(&t1).await.unwrap();

        let next = backend.next_task(1, None, false).await.unwrap();
        assert!(next.is_some());
        assert_eq!(next.unwrap().title(), "High priority");
    }

    #[tokio::test]
    async fn test_project_crud() {
        if test_url().is_none() {
            return;
        }
        let backend = setup().await;

        let project = backend
            .create_project(&CreateProjectParams {
                name: "test-project".to_string(),
                description: Some("A test".to_string()),
            })
            .await
            .unwrap();
        assert_eq!(project.name(), "test-project");

        let fetched = backend.get_project(project.id()).await.unwrap();
        assert_eq!(fetched.name(), "test-project");

        let by_name = backend.get_project_by_name("test-project").await.unwrap();
        assert_eq!(by_name.id(), project.id());

        let all = backend.list_projects().await.unwrap();
        assert!(all.len() >= 2);

        backend.delete_project(project.id()).await.unwrap();
    }

    #[tokio::test]
    async fn test_user_crud() {
        if test_url().is_none() {
            return;
        }
        let backend = setup().await;

        let user = backend
            .create_user(&CreateUserParams {
                username: "testuser".to_string(),
                sub: None,
                display_name: Some("Test User".to_string()),
                email: Some("test@example.com".to_string()),
            })
            .await
            .unwrap();
        assert_eq!(user.username(), "testuser");
        assert_eq!(user.sub(), "testuser");

        let fetched = backend.get_user(user.id()).await.unwrap();
        assert_eq!(fetched.username(), "testuser");

        let by_name = backend.get_user_by_username("testuser").await.unwrap();
        assert_eq!(by_name.id(), user.id());

        backend.delete_user(user.id()).await.unwrap();
    }

    #[tokio::test]
    async fn test_update_task() {
        if test_url().is_none() {
            return;
        }
        let backend = setup().await;

        let task = backend.create_task(1, &params("Original")).await.unwrap();
        let updated = backend
            .update_task(
                1,
                task.id(),
                &UpdateTaskParams {
                    title: Some("Updated".to_string()),
                    description: Some(Some("A description".to_string())),
                    background: None,
                    plan: None,
                    priority: None,
                    assignee_session_id: None,
                    assignee_user_id: None,
                    started_at: None,
                    completed_at: None,
                    canceled_at: None,
                    cancel_reason: None,
                    branch: None,
                    pr_url: None,
                    contract_id: None,
                    metadata: None,
                },
            )
            .await
            .unwrap();
        assert_eq!(updated.title(), "Updated");
        assert_eq!(updated.description(), Some("A description"));
    }

    #[tokio::test]
    async fn test_update_task_arrays() {
        if test_url().is_none() {
            return;
        }
        let backend = setup().await;

        let task = backend.create_task(1, &params("Array test")).await.unwrap();

        backend
            .update_task_arrays(
                1,
                task.id(),
                &UpdateTaskArrayParams {
                    set_tags: None,
                    add_tags: vec!["tag1".to_string(), "tag2".to_string()],
                    remove_tags: vec![],
                    set_definition_of_done: None,
                    add_definition_of_done: vec!["DoD item".to_string()],
                    remove_definition_of_done: vec![],
                    set_in_scope: None,
                    add_in_scope: vec![],
                    remove_in_scope: vec![],
                    set_out_of_scope: None,
                    add_out_of_scope: vec![],
                    remove_out_of_scope: vec![],
                },
            )
            .await
            .unwrap();

        let task = backend.get_task(1, task.id()).await.unwrap();
        assert_eq!(task.tags().len(), 2);
        assert_eq!(task.definition_of_done().len(), 1);
        assert_eq!(task.definition_of_done()[0].content(), "DoD item");
    }

    #[tokio::test]
    async fn test_task_stats() {
        if test_url().is_none() {
            return;
        }
        let backend = setup().await;

        backend.create_task(1, &params("Task A")).await.unwrap();
        backend.create_task(1, &params("Task B")).await.unwrap();

        let stats = backend.task_stats(1).await.unwrap();
        assert_eq!(*stats.get("draft").unwrap_or(&0), 2);
    }

    // --- MetadataField tests ---

    #[tokio::test]
    async fn test_create_and_get_metadata_field() {
        if test_url().is_none() {
            return;
        }
        let backend = setup().await;
        let params = CreateMetadataFieldParams {
            name: "sprint".to_string(),
            field_type: MetadataFieldType::String,
            required_on_complete: false,
            description: Some("Sprint name".to_string()),
        };
        let field = backend.create_metadata_field(1, &params).await.unwrap();
        assert_eq!(field.name(), "sprint");
        assert_eq!(field.field_type(), MetadataFieldType::String);
        assert!(!field.required_on_complete());
        assert_eq!(field.description(), Some("Sprint name"));

        let fetched = backend.get_metadata_field(1, field.id()).await.unwrap();
        assert_eq!(fetched.id(), field.id());
    }

    #[tokio::test]
    async fn test_list_metadata_fields() {
        if test_url().is_none() {
            return;
        }
        let backend = setup().await;
        backend
            .create_metadata_field(
                1,
                &CreateMetadataFieldParams {
                    name: "sprint".to_string(),
                    field_type: MetadataFieldType::String,
                    required_on_complete: false,
                    description: None,
                },
            )
            .await
            .unwrap();
        backend
            .create_metadata_field(
                1,
                &CreateMetadataFieldParams {
                    name: "points".to_string(),
                    field_type: MetadataFieldType::Number,
                    required_on_complete: true,
                    description: None,
                },
            )
            .await
            .unwrap();

        let fields = backend.list_metadata_fields(1).await.unwrap();
        assert_eq!(fields.len(), 2);
    }

    #[tokio::test]
    async fn test_update_metadata_field_description() {
        if test_url().is_none() {
            return;
        }
        let backend = setup().await;
        let field = backend
            .create_metadata_field(
                1,
                &CreateMetadataFieldParams {
                    name: "sprint".to_string(),
                    field_type: MetadataFieldType::String,
                    required_on_complete: false,
                    description: Some("old".to_string()),
                },
            )
            .await
            .unwrap();

        // Clear description
        let updated = backend
            .update_metadata_field(
                1,
                field.id(),
                &UpdateMetadataFieldParams {
                    required_on_complete: None,
                    description: Some(None),
                },
            )
            .await
            .unwrap();
        assert_eq!(updated.description(), None);

        // Set description
        let updated = backend
            .update_metadata_field(
                1,
                field.id(),
                &UpdateMetadataFieldParams {
                    required_on_complete: None,
                    description: Some(Some("new desc".to_string())),
                },
            )
            .await
            .unwrap();
        assert_eq!(updated.description(), Some("new desc"));
    }

    #[tokio::test]
    async fn test_delete_metadata_field() {
        if test_url().is_none() {
            return;
        }
        let backend = setup().await;
        let field = backend
            .create_metadata_field(
                1,
                &CreateMetadataFieldParams {
                    name: "sprint".to_string(),
                    field_type: MetadataFieldType::String,
                    required_on_complete: false,
                    description: None,
                },
            )
            .await
            .unwrap();
        backend.delete_metadata_field(1, field.id()).await.unwrap();
        let result = backend.get_metadata_field(1, field.id()).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_metadata_field_name_conflict() {
        if test_url().is_none() {
            return;
        }
        let backend = setup().await;
        let params = CreateMetadataFieldParams {
            name: "sprint".to_string(),
            field_type: MetadataFieldType::String,
            required_on_complete: false,
            description: None,
        };
        backend.create_metadata_field(1, &params).await.unwrap();
        let result = backend.create_metadata_field(1, &params).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_migrations_on_embedded_postgres() {
        let mut pg = postgresql_embedded::PostgreSQL::default();
        pg.setup()
            .await
            .expect("failed to setup embedded PostgreSQL");
        pg.start()
            .await
            .expect("failed to start embedded PostgreSQL");

        let db_name = "senko_migration_test";
        pg.create_database(db_name)
            .await
            .expect("failed to create test database");

        let url = pg.settings().url(db_name);
        let backend = PostgresBackend::new(url, Some(1));
        backend
            .pool()
            .await
            .expect("all migrations should succeed on a clean database");

        let pool = backend.pool().await.unwrap();
        let version: i64 = sqlx::query("SELECT COALESCE(MAX(version), 0) FROM _sqlx_migrations")
            .fetch_one(pool)
            .await
            .unwrap()
            .get(0);
        assert_eq!(
            version,
            MIGRATIONS.last().unwrap().version,
            "all migrations should be applied"
        );

        pg.stop().await.expect("failed to stop embedded PostgreSQL");
    }

    // --- Contract tests ---

    fn contract_params(title: &str) -> CreateContractParams {
        CreateContractParams {
            title: title.to_string(),
            description: Some("spec".to_string()),
            definition_of_done: vec!["item1".to_string(), "item2".to_string()],
            tags: vec!["api".to_string()],
            metadata: Some(serde_json::json!({"owner": "team-a"})),
        }
    }

    #[tokio::test]
    async fn test_contract_create_and_get() {
        if test_url().is_none() {
            return;
        }
        let backend = setup().await;
        let created = backend
            .create_contract(1, &contract_params("Spec"))
            .await
            .unwrap();
        assert_eq!(created.title(), "Spec");
        assert_eq!(created.project_id(), 1);
        assert_eq!(created.definition_of_done().len(), 2);
        assert_eq!(created.tags(), &["api".to_string()]);

        let got = backend.get_contract(created.id()).await.unwrap();
        assert_eq!(got.id(), created.id());
        assert_eq!(got.definition_of_done()[0].content(), "item1");
    }

    #[tokio::test]
    async fn test_contract_update_and_check_dod() {
        if test_url().is_none() {
            return;
        }
        let backend = setup().await;
        let c = backend
            .create_contract(1, &contract_params("Update me"))
            .await
            .unwrap();

        let updated = backend
            .update_contract(
                c.id(),
                &UpdateContractParams {
                    title: Some("Renamed".to_string()),
                    description: None,
                    metadata: Some(MetadataUpdate::Merge(
                        serde_json::json!({"stage": "review"}),
                    )),
                },
                &UpdateContractArrayParams {
                    add_tags: vec!["backend".to_string()],
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(updated.title(), "Renamed");
        assert!(updated.tags().contains(&"backend".to_string()));
        assert_eq!(
            updated.metadata(),
            Some(&serde_json::json!({"owner": "team-a", "stage": "review"}))
        );

        let checked = backend.check_dod(c.id(), 1).await.unwrap();
        assert!(checked.definition_of_done()[0].checked());
        assert!(!checked.definition_of_done()[1].checked());

        let unchecked = backend.uncheck_dod(c.id(), 1).await.unwrap();
        assert!(!unchecked.definition_of_done()[0].checked());
    }

    #[tokio::test]
    async fn test_contract_delete_cascades() {
        if test_url().is_none() {
            return;
        }
        let backend = setup().await;
        let c = backend
            .create_contract(1, &contract_params("Delete"))
            .await
            .unwrap();
        backend
            .add_note(
                c.id(),
                &ContractNote::new("n".to_string(), None, "2026-04-17T00:00:00Z".to_string()),
            )
            .await
            .unwrap();

        backend.delete_contract(c.id()).await.unwrap();
        assert!(backend.get_contract(c.id()).await.is_err());
    }

    #[tokio::test]
    async fn test_create_task_with_contract_id() {
        if test_url().is_none() {
            return;
        }
        let backend = setup().await;
        let c = backend
            .create_contract(1, &contract_params("linked at create"))
            .await
            .unwrap();
        let mut p = params("task with contract");
        p.contract_id = Some(c.id());
        let task = backend.create_task(1, &p).await.unwrap();
        assert_eq!(task.contract_id(), Some(c.id()));

        let got = backend.get_task(1, task.task_number()).await.unwrap();
        assert_eq!(got.contract_id(), Some(c.id()));
    }

    #[tokio::test]
    async fn test_task_contract_id_roundtrip() {
        if test_url().is_none() {
            return;
        }
        let backend = setup().await;
        let c = backend
            .create_contract(1, &contract_params("linked"))
            .await
            .unwrap();
        let task = backend
            .create_task(1, &params("linked task"))
            .await
            .unwrap();
        assert_eq!(task.contract_id(), None);

        let updated = backend
            .update_task(
                1,
                task.id(),
                &UpdateTaskParams {
                    title: None,
                    background: None,
                    description: None,
                    plan: None,
                    priority: None,
                    assignee_session_id: None,
                    assignee_user_id: None,
                    started_at: None,
                    completed_at: None,
                    canceled_at: None,
                    cancel_reason: None,
                    branch: None,
                    pr_url: None,
                    contract_id: Some(Some(c.id())),
                    metadata: None,
                },
            )
            .await
            .unwrap();
        assert_eq!(updated.contract_id(), Some(c.id()));
    }
}
