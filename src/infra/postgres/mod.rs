use std::collections::HashMap;

use anyhow::{Context, Result};
use async_trait::async_trait;
use sqlx::postgres::PgPool;
use sqlx::Row;

use crate::domain::project::{CreateProjectParams, Project};
use crate::domain::repository::{ProjectRepository, TaskRepository};
use crate::domain::task::{
    CreateTaskParams, DodItem, ListTasksFilter, Priority, Task, TaskStatus, UpdateTaskArrayParams,
    UpdateTaskParams,
};
use crate::domain::user::{
    AddProjectMemberParams, ApiKey, ApiKeyWithSecret, CreateUserParams, NewApiKey, ProjectMember,
    Role, User,
};

pub struct PostgresBackend {
    url: String,
    pool: tokio::sync::OnceCell<PgPool>,
}

impl PostgresBackend {
    pub fn new(url: String) -> Self {
        Self {
            url,
            pool: tokio::sync::OnceCell::new(),
        }
    }

    async fn pool(&self) -> Result<&PgPool> {
        self.pool
            .get_or_try_init(|| async {
                let pool = PgPool::connect(&self.url)
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

const MIGRATION_SQL: &str = include_str!("migrations/20260328000000_initial_schema.sql");

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

    // Check if initial migration was already applied
    let applied: bool = sqlx::query("SELECT 1 FROM _sqlx_migrations WHERE version = 1")
        .fetch_optional(pool)
        .await?
        .is_some();

    if !applied {
        // Split on semicolons and execute each statement
        for statement in MIGRATION_SQL.split(';') {
            let trimmed = statement.trim();
            if !trimmed.is_empty() {
                sqlx::query(trimmed)
                    .execute(pool)
                    .await
                    .with_context(|| format!("migration statement failed: {}", &trimmed[..trimmed.len().min(80)]))?;
            }
        }
        sqlx::query(
            "INSERT INTO _sqlx_migrations (version, description) VALUES (1, 'initial_schema')",
        )
        .execute(pool)
        .await?;
    }

    Ok(())
}

// --- Helper: build a Task from a tasks row + child queries ---

async fn get_task_by_id(pool: &PgPool, id: i64) -> Result<Task> {
    let row = sqlx::query(
        "SELECT project_id, title, background, description, plan, status, priority,
                assignee_session_id, created_at, updated_at, started_at, completed_at,
                canceled_at, cancel_reason, branch, pr_url, metadata, assignee_user_id
         FROM tasks WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?
    .context("task not found")?;

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

    let tags: Vec<String> =
        sqlx::query("SELECT tag FROM task_tags WHERE task_id = $1 ORDER BY id")
            .bind(id)
            .fetch_all(pool)
            .await?
            .into_iter()
            .map(|r| r.get("tag"))
            .collect();

    let dependencies: Vec<i64> = sqlx::query(
        "SELECT depends_on_task_id FROM task_dependencies WHERE task_id = $1 ORDER BY id",
    )
    .bind(id)
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|r| r.get("depends_on_task_id"))
    .collect();

    Ok(Task::new(
        id,
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
        metadata,
        definition_of_done,
        in_scope,
        out_of_scope,
        tags,
        dependencies,
    ))
}

async fn verify_task_project(pool: &PgPool, project_id: i64, task_id: i64) -> Result<()> {
    let row = sqlx::query("SELECT project_id FROM tasks WHERE id = $1")
        .bind(task_id)
        .fetch_optional(pool)
        .await?
        .context("task not found")?;
    let actual: i64 = row.get("project_id");
    if actual != project_id {
        anyhow::bail!("task not found");
    }
    Ok(())
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
            .context("project not found")?;
        Ok(Project::new(
            id,
            row.get("name"),
            row.get("description"),
            row.get("created_at"),
        ))
    }

    async fn get_project_by_name(&self, name: &str) -> Result<Project> {
        let pool = self.pool().await?;
        let row =
            sqlx::query("SELECT id, description, created_at FROM projects WHERE name = $1")
                .bind(name)
                .fetch_optional(pool)
                .await?
                .context("project not found")?;
        Ok(Project::new(
            row.get("id"),
            name.to_string(),
            row.get("description"),
            row.get("created_at"),
        ))
    }

    async fn list_projects(&self) -> Result<Vec<Project>> {
        let pool = self.pool().await?;
        let rows =
            sqlx::query("SELECT id, name, description, created_at FROM projects ORDER BY id")
                .fetch_all(pool)
                .await?;
        Ok(rows
            .into_iter()
            .map(|r| Project::new(
                r.get("id"),
                r.get("name"),
                r.get("description"),
                r.get("created_at"),
            ))
            .collect())
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

    // --- User management ---

    async fn create_user(&self, params: &CreateUserParams) -> Result<User> {
        let pool = self.pool().await?;
        let row = sqlx::query(
            "INSERT INTO users (username, display_name, email) VALUES ($1, $2, $3) RETURNING id, created_at",
        )
        .bind(&params.username)
        .bind(&params.display_name)
        .bind(&params.email)
        .fetch_one(pool)
        .await?;
        Ok(User::new(
            row.get("id"),
            params.username.clone(),
            params.display_name.clone(),
            params.email.clone(),
            row.get("created_at"),
        ))
    }

    async fn get_user(&self, id: i64) -> Result<User> {
        let pool = self.pool().await?;
        let row = sqlx::query(
            "SELECT username, display_name, email, created_at FROM users WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(pool)
        .await?
        .context("user not found")?;
        Ok(User::new(
            id,
            row.get("username"),
            row.get("display_name"),
            row.get("email"),
            row.get("created_at"),
        ))
    }

    async fn get_user_by_username(&self, username: &str) -> Result<User> {
        let pool = self.pool().await?;
        let row = sqlx::query(
            "SELECT id, display_name, email, created_at FROM users WHERE username = $1",
        )
        .bind(username)
        .fetch_optional(pool)
        .await?
        .context("user not found")?;
        Ok(User::new(
            row.get("id"),
            username.to_string(),
            row.get("display_name"),
            row.get("email"),
            row.get("created_at"),
        ))
    }

    async fn list_users(&self) -> Result<Vec<User>> {
        let pool = self.pool().await?;
        let rows =
            sqlx::query("SELECT id, username, display_name, email, created_at FROM users ORDER BY id")
                .fetch_all(pool)
                .await?;
        Ok(rows
            .into_iter()
            .map(|r| User::new(
                r.get("id"),
                r.get("username"),
                r.get("display_name"),
                r.get("email"),
                r.get("created_at"),
            ))
            .collect())
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

    // --- Project membership ---

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
        let result = sqlx::query(
            "DELETE FROM project_members WHERE project_id = $1 AND user_id = $2",
        )
        .bind(project_id)
        .bind(user_id)
        .execute(pool)
        .await?;
        if result.rows_affected() == 0 {
            anyhow::bail!(
                "project member not found: project_id={project_id}, user_id={user_id}"
            );
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
        .context("project member not found")?;
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
            anyhow::bail!(
                "project member not found: project_id={project_id}, user_id={user_id}"
            );
        }
        self.get_project_member(project_id, user_id).await
    }

    // --- API key management ---

    async fn create_api_key(&self, user_id: i64, name: &str, new_key: &NewApiKey) -> Result<ApiKeyWithSecret> {
        let pool = self.pool().await?;
        // Verify user exists
        self.get_user(user_id).await?;

        let row = sqlx::query(
            "INSERT INTO api_keys (user_id, key_hash, key_prefix, name) VALUES ($1, $2, $3, $4) RETURNING id, created_at",
        )
        .bind(user_id)
        .bind(&new_key.key_hash)
        .bind(&new_key.key_prefix)
        .bind(name)
        .fetch_one(pool)
        .await?;

        Ok(ApiKeyWithSecret::new(
            row.get("id"),
            user_id,
            new_key.raw_key.clone(),
            new_key.key_prefix.clone(),
            name.to_string(),
            row.get("created_at"),
        ))
    }

    async fn get_user_by_api_key(&self, key_hash: &str) -> Result<User> {
        let pool = self.pool().await?;

        sqlx::query(
            "UPDATE api_keys SET last_used_at = $2 WHERE key_hash = $1",
        )
        .bind(&key_hash)
        .bind(now_utc())
        .execute(pool)
        .await?;

        let row = sqlx::query("SELECT user_id FROM api_keys WHERE key_hash = $1")
            .bind(&key_hash)
            .fetch_optional(pool)
            .await?
            .context("invalid api key")?;
        let user_id: i64 = row.get("user_id");
        self.get_user(user_id).await
    }

    async fn list_api_keys(&self, user_id: i64) -> Result<Vec<ApiKey>> {
        let pool = self.pool().await?;
        let rows = sqlx::query(
            "SELECT id, user_id, key_prefix, name, created_at, last_used_at FROM api_keys WHERE user_id = $1 ORDER BY id",
        )
        .bind(user_id)
        .fetch_all(pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|r| ApiKey::new(
                r.get("id"),
                r.get("user_id"),
                r.get("key_prefix"),
                r.get("name"),
                r.get("created_at"),
                r.get("last_used_at"),
            ))
            .collect())
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
            "INSERT INTO tasks (title, background, description, priority, branch, pr_url, metadata, project_id)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8) RETURNING id",
        )
        .bind(&params.title)
        .bind(&params.background)
        .bind(&params.description)
        .bind(priority)
        .bind(&params.branch)
        .bind(&params.pr_url)
        .bind(&metadata_str)
        .bind(project_id)
        .fetch_one(&mut *tx)
        .await?;
        let task_id: i64 = row.get("id");

        for item in &params.definition_of_done {
            sqlx::query(
                "INSERT INTO task_definition_of_done (task_id, content) VALUES ($1, $2)",
            )
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
        for dep_id in &params.dependencies {
            sqlx::query(
                "INSERT INTO task_dependencies (task_id, depends_on_task_id) VALUES ($1, $2)",
            )
            .bind(task_id)
            .bind(dep_id)
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;
        get_task_by_id(pool, task_id).await
    }

    async fn get_task(&self, project_id: i64, id: i64) -> Result<Task> {
        let pool = self.pool().await?;
        verify_task_project(pool, project_id, id).await?;
        get_task_by_id(pool, id).await
    }

    async fn update_task(
        &self,
        project_id: i64,
        id: i64,
        params: &UpdateTaskParams,
    ) -> Result<Task> {
        let pool = self.pool().await?;
        verify_task_project(pool, project_id, id).await?;

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
            sqlx::query(
                "UPDATE tasks SET assignee_session_id = $1, updated_at = $2 WHERE id = $3",
            )
            .bind(assignee)
            .bind(now_utc())
            .bind(id)
            .execute(&mut *tx)
            .await?;
        }
        if let Some(ref assignee_user_id) = params.assignee_user_id {
            sqlx::query("UPDATE tasks SET assignee_user_id = $1, updated_at = $2 WHERE id = $3")
                .bind(assignee_user_id)
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
        if let Some(ref metadata) = params.metadata {
            let metadata_str: Option<String> = metadata
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
        verify_task_project(pool, project_id, id).await?;

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
        verify_task_project(pool, project_id, id).await?;
        let result = sqlx::query("DELETE FROM tasks WHERE id = $1")
            .bind(id)
            .execute(pool)
            .await?;
        if result.rows_affected() == 0 {
            anyhow::bail!("task not found: {id}");
        }
        Ok(())
    }

    async fn list_tasks(
        &self,
        project_id: i64,
        filter: &ListTasksFilter,
    ) -> Result<Vec<Task>> {
        let pool = self.pool().await?;

        let mut conditions: Vec<String> = Vec::new();
        let mut param_idx: i32 = 1;

        // We'll build SQL with numbered params and collect bind values
        // Since we can't dynamically bind heterogeneous types easily with sqlx,
        // we build the query string with casted params.

        // Approach: build conditions, then execute with a single query using string interpolation
        // for param numbers, and bind them sequentially using a query builder.

        // Actually, the cleanest approach: build the WHERE clause and use sqlx::query with raw SQL.
        // We'll bind all params as strings (since statuses and tags are strings, project_id is i64).

        // Collect all bind values as enum
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
                    binds.push(BindVal::Str(String::new())); // placeholder, filled below
                    param_idx += 1;
                    p
                })
                .collect();
            // Fix: replace placeholder binds with actual values
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
            #[allow(unused_assignments)]
            { param_idx += 1; }
        }

        if filter.ready {
            conditions.push("t.status = 'todo'".to_string());
            conditions.push(
                "NOT EXISTS (SELECT 1 FROM task_dependencies td JOIN tasks dep ON dep.id = td.depends_on_task_id WHERE td.task_id = t.id AND dep.status != 'completed')"
                    .to_string(),
            );
        }

        let where_clause = if conditions.is_empty() {
            String::new()
        } else {
            format!(" WHERE {}", conditions.join(" AND "))
        };

        let sql = format!("SELECT t.id FROM tasks t{where_clause} ORDER BY t.id");

        // Build the query and bind all values
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

    async fn next_task(&self, project_id: i64) -> Result<Option<Task>> {
        let pool = self.pool().await?;
        let row = sqlx::query(
            "SELECT t.id FROM tasks t
             WHERE t.project_id = $1
               AND t.status = 'todo'
               AND NOT EXISTS (
                 SELECT 1 FROM task_dependencies td
                 JOIN tasks dep ON dep.id = td.depends_on_task_id
                 WHERE td.task_id = t.id AND dep.status != 'completed'
               )
             ORDER BY t.priority ASC, t.created_at ASC, t.id ASC
             LIMIT 1",
        )
        .bind(project_id)
        .fetch_optional(pool)
        .await?;
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

    async fn add_dependency(
        &self,
        project_id: i64,
        task_id: i64,
        dep_id: i64,
    ) -> Result<Task> {
        let pool = self.pool().await?;
        verify_task_project(pool, project_id, task_id).await?;
        // Verify both tasks exist
        get_task_by_id(pool, task_id)
            .await
            .context("task not found")?;
        get_task_by_id(pool, dep_id)
            .await
            .context("dependency task not found")?;

        sqlx::query(
            "INSERT INTO task_dependencies (task_id, depends_on_task_id) VALUES ($1, $2) ON CONFLICT DO NOTHING",
        )
        .bind(task_id)
        .bind(dep_id)
        .execute(pool)
        .await?;

        sqlx::query("UPDATE tasks SET updated_at = $1 WHERE id = $2")
            .bind(now_utc())
            .bind(task_id)
            .execute(pool)
            .await?;

        get_task_by_id(pool, task_id).await
    }

    async fn remove_dependency(
        &self,
        project_id: i64,
        task_id: i64,
        dep_id: i64,
    ) -> Result<Task> {
        let pool = self.pool().await?;
        verify_task_project(pool, project_id, task_id).await?;
        get_task_by_id(pool, task_id)
            .await
            .context("task not found")?;

        let result = sqlx::query(
            "DELETE FROM task_dependencies WHERE task_id = $1 AND depends_on_task_id = $2",
        )
        .bind(task_id)
        .bind(dep_id)
        .execute(pool)
        .await?;
        if result.rows_affected() == 0 {
            anyhow::bail!(
                "dependency not found: task {} does not depend on {}",
                task_id,
                dep_id
            );
        }

        sqlx::query("UPDATE tasks SET updated_at = $1 WHERE id = $2")
            .bind(now_utc())
            .bind(task_id)
            .execute(pool)
            .await?;

        get_task_by_id(pool, task_id).await
    }

    async fn set_dependencies(
        &self,
        project_id: i64,
        task_id: i64,
        dep_ids: &[i64],
    ) -> Result<Task> {
        let pool = self.pool().await?;
        verify_task_project(pool, project_id, task_id).await?;
        get_task_by_id(pool, task_id)
            .await
            .context("task not found")?;
        for &dep_id in dep_ids {
            get_task_by_id(pool, dep_id)
                .await
                .with_context(|| format!("dependency task not found: {}", dep_id))?;
        }

        let mut tx = pool.begin().await?;
        sqlx::query("DELETE FROM task_dependencies WHERE task_id = $1")
            .bind(task_id)
            .execute(&mut *tx)
            .await?;
        for &dep_id in dep_ids {
            sqlx::query(
                "INSERT INTO task_dependencies (task_id, depends_on_task_id) VALUES ($1, $2)",
            )
            .bind(task_id)
            .bind(dep_id)
            .execute(&mut *tx)
            .await?;
        }
        sqlx::query("UPDATE tasks SET updated_at = $1 WHERE id = $2")
            .bind(now_utc())
            .bind(task_id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;

        get_task_by_id(pool, task_id).await
    }

    async fn list_dependencies(&self, project_id: i64, task_id: i64) -> Result<Vec<Task>> {
        let pool = self.pool().await?;
        verify_task_project(pool, project_id, task_id).await?;
        get_task_by_id(pool, task_id)
            .await
            .context("task not found")?;

        let rows = sqlx::query(
            "SELECT depends_on_task_id FROM task_dependencies WHERE task_id = $1",
        )
        .bind(task_id)
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
                branch = $14, pr_url = $15, metadata = $16,
                updated_at = $17
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

        tx.commit().await?;
        Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;

    fn test_url() -> Option<String> {
        std::env::var("SENKO_TEST_POSTGRES_URL").ok()
    }

    async fn setup() -> PostgresBackend {
        let url = test_url().expect("SENKO_TEST_POSTGRES_URL must be set for postgres tests");
        let backend = PostgresBackend::new(url);
        let pool = backend.pool().await.unwrap();

        // Clean all data for test isolation (reverse FK order)
        sqlx::query("DELETE FROM task_dependencies").execute(pool).await.unwrap();
        sqlx::query("DELETE FROM task_definition_of_done").execute(pool).await.unwrap();
        sqlx::query("DELETE FROM task_in_scope").execute(pool).await.unwrap();
        sqlx::query("DELETE FROM task_out_of_scope").execute(pool).await.unwrap();
        sqlx::query("DELETE FROM task_tags").execute(pool).await.unwrap();
        sqlx::query("DELETE FROM api_keys").execute(pool).await.unwrap();
        sqlx::query("DELETE FROM project_members").execute(pool).await.unwrap();
        sqlx::query("DELETE FROM tasks").execute(pool).await.unwrap();
        sqlx::query("DELETE FROM users").execute(pool).await.unwrap();
        sqlx::query("DELETE FROM projects").execute(pool).await.unwrap();

        // Re-seed defaults
        sqlx::query("INSERT INTO projects (id, name, description) VALUES (1, 'default', 'Default project')")
            .execute(pool).await.unwrap();
        sqlx::query("INSERT INTO users (id, username, display_name) VALUES (1, 'default', 'Default User')")
            .execute(pool).await.unwrap();
        sqlx::query("INSERT INTO project_members (project_id, user_id, role) VALUES (1, 1, 'owner')")
            .execute(pool).await.unwrap();

        // Reset sequences
        sqlx::query("SELECT setval('projects_id_seq', GREATEST((SELECT MAX(id) FROM projects), 1))")
            .execute(pool).await.unwrap();
        sqlx::query("SELECT setval('users_id_seq', GREATEST((SELECT MAX(id) FROM users), 1))")
            .execute(pool).await.unwrap();
        sqlx::query("SELECT setval('tasks_id_seq', GREATEST((SELECT COALESCE(MAX(id), 0) FROM tasks), 1))")
            .execute(pool).await.unwrap();

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

        let task = backend.create_task(1, &params("Lifecycle test")).await.unwrap();
        assert_eq!(task.status(), TaskStatus::Draft);

        // Draft → Todo
        let (task, _) = task.ready(now_utc()).unwrap();
        backend.save(&task).await.unwrap();
        let task = backend.get_task(1, task.id()).await.unwrap();
        assert_eq!(task.status(), TaskStatus::Todo);

        // Todo → InProgress
        let (task, _) = task.start(None, None, now_utc()).unwrap();
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

        let t2 = backend.add_dependency(1, t2.id(), t1.id()).await.unwrap();
        assert_eq!(t2.dependencies(), vec![t1.id()]);

        let deps = backend.list_dependencies(1, t2.id()).await.unwrap();
        assert_eq!(deps.len(), 1);
        assert_eq!(deps[0].id(), t1.id());

        let t2 = backend.remove_dependency(1, t2.id(), t1.id()).await.unwrap();
        assert!(t2.dependencies().is_empty());
    }

    #[tokio::test]
    async fn test_list_tasks_with_filter() {
        if test_url().is_none() {
            return;
        }
        let backend = setup().await;

        let t1 = backend.create_task(1, &params("Todo task")).await.unwrap();
        let (t1, _) = t1.ready(now_utc()).unwrap();
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
        let next = backend.next_task(1).await.unwrap();
        assert!(next.is_none());

        let t1 = backend.create_task(1, &params("High priority")).await.unwrap();
        let (t1, _) = t1.ready(now_utc()).unwrap();
        backend.save(&t1).await.unwrap();

        let next = backend.next_task(1).await.unwrap();
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
                display_name: Some("Test User".to_string()),
                email: Some("test@example.com".to_string()),
            })
            .await
            .unwrap();
        assert_eq!(user.username(), "testuser");

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
                    description: Some("A description".to_string()),
                    ..Default::default()
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
                    add_tags: vec!["tag1".to_string(), "tag2".to_string()],
                    add_definition_of_done: vec!["DoD item".to_string()],
                    ..Default::default()
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
}
