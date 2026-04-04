use std::collections::HashMap;
use std::path::Path;

use anyhow::{Context, Result};
use rusqlite::{params, Connection, OptionalExtension};
use crate::domain::error::DomainError;
use crate::domain::project::{CreateProjectParams, Project};
use crate::domain::task::{
    self, CreateTaskParams, DodItem, ListTasksFilter, Priority, Task, TaskStatus,
    UpdateTaskArrayParams, UpdateTaskParams,
};
use crate::domain::user::{
    AddProjectMemberParams, ApiKey, ApiKeyWithSecret, CreateUserParams, NewApiKey, ProjectMember,
    Role, User,
};

struct Migration {
    version: i64,
    name: &'static str,
    sql: &'static str,
}

const MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        name: "initial_schema",
        sql: "
            CREATE TABLE IF NOT EXISTS tasks (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                title TEXT NOT NULL,
                background TEXT,
                description TEXT,
                plan TEXT,
                status TEXT NOT NULL DEFAULT 'draft',
                priority INTEGER NOT NULL DEFAULT 2,
                assignee_session_id TEXT,
                created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
                updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
                started_at TEXT,
                completed_at TEXT,
                canceled_at TEXT,
                cancel_reason TEXT,
                branch TEXT,
                pr_url TEXT,
                metadata TEXT
            );

            CREATE TABLE IF NOT EXISTS task_definition_of_done (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                task_id INTEGER NOT NULL,
                content TEXT NOT NULL,
                checked INTEGER NOT NULL DEFAULT 0,
                FOREIGN KEY (task_id) REFERENCES tasks(id) ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS task_in_scope (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                task_id INTEGER NOT NULL,
                content TEXT NOT NULL,
                FOREIGN KEY (task_id) REFERENCES tasks(id) ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS task_out_of_scope (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                task_id INTEGER NOT NULL,
                content TEXT NOT NULL,
                FOREIGN KEY (task_id) REFERENCES tasks(id) ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS task_tags (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                task_id INTEGER NOT NULL,
                tag TEXT NOT NULL,
                UNIQUE(task_id, tag),
                FOREIGN KEY (task_id) REFERENCES tasks(id) ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS task_dependencies (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                task_id INTEGER NOT NULL,
                depends_on_task_id INTEGER NOT NULL,
                UNIQUE(task_id, depends_on_task_id),
                FOREIGN KEY (task_id) REFERENCES tasks(id) ON DELETE CASCADE,
                FOREIGN KEY (depends_on_task_id) REFERENCES tasks(id) ON DELETE CASCADE
            );
        ",
    },
    Migration {
        version: 2,
        name: "add_projects",
        sql: "
            CREATE TABLE projects (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL UNIQUE,
                description TEXT,
                created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
            );

            INSERT INTO projects (id, name, description) VALUES (1, 'default', 'Default project');

            ALTER TABLE tasks ADD COLUMN project_id INTEGER NOT NULL DEFAULT 1;

            CREATE INDEX idx_tasks_project_id ON tasks(project_id);
        ",
    },
    Migration {
        version: 3,
        name: "add_users_and_members",
        sql: "
            CREATE TABLE users (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                username TEXT NOT NULL UNIQUE,
                display_name TEXT,
                email TEXT UNIQUE,
                created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
            );

            CREATE TABLE project_members (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                project_id INTEGER NOT NULL,
                user_id INTEGER NOT NULL,
                role TEXT NOT NULL DEFAULT 'member',
                created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
                UNIQUE(project_id, user_id),
                FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE,
                FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
            );

            CREATE INDEX idx_project_members_project_id ON project_members(project_id);
            CREATE INDEX idx_project_members_user_id ON project_members(user_id);

            ALTER TABLE tasks ADD COLUMN assignee_user_id INTEGER REFERENCES users(id);
        ",
    },
    Migration {
        version: 4,
        name: "add_api_keys",
        sql: "
            CREATE TABLE api_keys (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                user_id INTEGER NOT NULL,
                key_hash TEXT NOT NULL UNIQUE,
                key_prefix TEXT NOT NULL,
                name TEXT NOT NULL DEFAULT '',
                created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
                last_used_at TEXT,
                FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
            );

            CREATE INDEX idx_api_keys_key_hash ON api_keys(key_hash);
            CREATE INDEX idx_api_keys_user_id ON api_keys(user_id);
        ",
    },
    Migration {
        version: 5,
        name: "add_default_user",
        sql: "
            INSERT OR IGNORE INTO users (id, username, display_name)
            VALUES (1, 'default', 'Default User');

            INSERT OR IGNORE INTO project_members (project_id, user_id, role)
            VALUES (1, 1, 'owner');
        ",
    },
];

fn run_migrations(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_migrations (
            version INTEGER PRIMARY KEY,
            name TEXT NOT NULL,
            applied_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
        );",
    )?;

    let max_version: Option<i64> = conn
        .query_row(
            "SELECT MAX(version) FROM schema_migrations",
            [],
            |row| row.get(0),
        )
        .optional()?
        .flatten();

    if max_version.is_none() {
        let has_tasks: bool = conn
            .prepare("SELECT 1 FROM sqlite_master WHERE type='table' AND name='tasks'")
            .and_then(|mut s| s.exists([]))
            .unwrap_or(false);

        if has_tasks {
            // Legacy DB: apply old idempotent migrations, then mark version 1
            migrate_dod_checked(conn)?;
            migrate_legacy(conn)?;
            conn.execute(
                "INSERT INTO schema_migrations (version, name) VALUES (1, 'initial_schema')",
                [],
            )?;
            // Fall through to apply remaining migrations (v2+)
        }
    }

    let current_version: i64 = conn
        .query_row(
            "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
            [],
            |row| row.get(0),
        )?;
    for m in MIGRATIONS {
        if m.version > current_version {
            let tx_sql = format!("BEGIN;\n{}\nCOMMIT;", m.sql);
            conn.execute_batch(&tx_sql)?;
            conn.execute(
                "INSERT INTO schema_migrations (version, name) VALUES (?1, ?2)",
                params![m.version, m.name],
            )?;
        }
    }

    Ok(())
}

pub fn current_schema_version(conn: &Connection) -> Result<i64> {
    let has_table: bool = conn
        .prepare("SELECT 1 FROM sqlite_master WHERE type='table' AND name='schema_migrations'")
        .and_then(|mut s| s.exists([]))
        .unwrap_or(false);
    if !has_table {
        return Ok(0);
    }
    let version: Option<i64> = conn
        .query_row(
            "SELECT MAX(version) FROM schema_migrations",
            [],
            |row| row.get(0),
        )
        .optional()?
        .flatten();
    Ok(version.unwrap_or(0))
}

/// Resolve the XDG data directory base.
/// Returns `$XDG_DATA_HOME` or `~/.local/share`.
fn xdg_data_base() -> Option<std::path::PathBuf> {
    std::env::var("XDG_DATA_HOME")
        .map(std::path::PathBuf::from)
        .ok()
        .filter(|p| p.is_absolute())
        .or_else(|| {
            std::env::var("HOME")
                .ok()
                .map(|h| std::path::PathBuf::from(h).join(".local").join("share"))
        })
}

/// Compute a per-project XDG database path using the project directory name.
/// Returns `$XDG_DATA_HOME/senko/projects/<dir-name>/data.db`.
fn xdg_project_db_path(project_root: &Path) -> Option<std::path::PathBuf> {
    let data_dir = xdg_data_base()?;
    let dir_name = project_root.file_name()?.to_string_lossy();
    Some(data_dir.join("senko").join("projects").join(dir_name.as_ref()).join("data.db"))
}

/// Legacy hash-based per-project XDG database path (for migration).
/// Returns `$XDG_DATA_HOME/senko/projects/<sha256-16chars>/data.db`.
fn xdg_project_db_path_legacy_hash(project_root: &Path) -> Option<std::path::PathBuf> {
    use sha2::{Sha256, Digest};
    let data_dir = xdg_data_base()?;
    let canonical = project_root.canonicalize().ok()
        .unwrap_or_else(|| project_root.to_path_buf());
    let hash = format!("{:x}", Sha256::digest(canonical.to_string_lossy().as_bytes()));
    let short_hash = &hash[..16];
    Some(data_dir.join("senko").join("projects").join(short_hash).join("data.db"))
}

/// Old global XDG path (pre-per-project migration).
/// Returns `$XDG_DATA_HOME/senko/data.db`.
fn xdg_global_db_path() -> Option<std::path::PathBuf> {
    let data_dir = xdg_data_base()?;
    Some(data_dir.join("senko").join("data.db"))
}

/// Copy a database file and its WAL/SHM companions to a new location.
fn copy_db_files(src: &Path, dst: &Path) -> Result<()> {
    if let Some(parent) = dst.parent() {
        std::fs::create_dir_all(parent)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700))?;
        }
    }
    std::fs::copy(src, dst)?;
    let src_wal = src.with_extension("db-wal");
    let src_shm = src.with_extension("db-shm");
    if src_wal.exists() {
        std::fs::copy(&src_wal, dst.with_extension("db-wal"))?;
    }
    if src_shm.exists() {
        std::fs::copy(&src_shm, dst.with_extension("db-shm"))?;
    }
    Ok(())
}

/// Resolve the expected database path without side effects (no migration, no file checks).
/// Used by `resolve_backend_info()` to report the DB path in hook metadata.
///
/// Priority: config_db_path → per-project XDG path.
/// Returns `None` only when neither `XDG_DATA_HOME` nor `HOME` is set.
pub fn resolve_db_path_preview(
    project_root: &Path,
    config_db_path: Option<&str>,
) -> Option<std::path::PathBuf> {
    if let Some(p) = config_db_path {
        return Some(std::path::PathBuf::from(p));
    }
    xdg_project_db_path(project_root)
}

/// Resolve the database path with the following priority (high → low):
/// 1. `explicit_db_path` (CLI --db-path or SENKO_DB_PATH env)
/// 2. `config_db_path` (config.toml [storage] db_path)
/// 3. Per-project XDG path (already exists)
/// 4. Migration from hash-based XDG path → dir-name-based XDG path
/// 5. Migration from legacy `.senko/data.db` → per-project XDG path
/// 6. Migration from old global XDG path → per-project XDG path
/// 7. New installation: per-project XDG default
fn resolve_db_path(
    project_root: &Path,
    explicit_db_path: Option<&Path>,
    config_db_path: Option<&str>,
) -> Result<std::path::PathBuf> {
    // 1. CLI / env var
    if let Some(p) = explicit_db_path {
        return Ok(p.to_path_buf());
    }

    // 2. config.toml [storage] db_path
    if let Some(p) = config_db_path {
        return Ok(std::path::PathBuf::from(p));
    }

    // 3. Per-project XDG path (already exists)
    let xdg_path = xdg_project_db_path(project_root)
        .ok_or_else(|| anyhow::anyhow!("cannot determine XDG_DATA_HOME or HOME directory"))?;

    if xdg_path.exists() {
        return Ok(xdg_path);
    }

    // 4. Migrate from hash-based XDG path → dir-name-based XDG path
    if let Some(hash_path) = xdg_project_db_path_legacy_hash(project_root) {
        if hash_path.exists() {
            copy_db_files(&hash_path, &xdg_path)?;
            eprintln!(
                "warning: migrated database from {} to {}. \
                 The hash-based path has been kept. You can remove it after verifying the migration.",
                hash_path.display(),
                xdg_path.display()
            );
            return Ok(xdg_path);
        }
    }

    // 5. Migrate from legacy project-local path
    let legacy_path = project_root.join(".senko").join("data.db");
    if legacy_path.exists() {
        copy_db_files(&legacy_path, &xdg_path)?;
        eprintln!(
            "warning: migrated database from {} to {}. \
             The original file has been kept. You can remove it after verifying the migration.",
            legacy_path.display(),
            xdg_path.display()
        );
        return Ok(xdg_path);
    }

    // 6. Migrate from old global XDG path (pre-per-project layout)
    if let Some(global_path) = xdg_global_db_path() {
        if global_path.exists() {
            copy_db_files(&global_path, &xdg_path)?;
            eprintln!(
                "warning: migrated database from {} to {}. \
                 The global database was shared across all projects. \
                 If you have multiple projects, only the first to run gets this data. \
                 The original file has been kept.",
                global_path.display(),
                xdg_path.display()
            );
            // Remove the global file so the next project doesn't also get it
            let _ = std::fs::remove_file(&global_path);
            let _ = std::fs::remove_file(global_path.with_extension("db-wal"));
            let _ = std::fs::remove_file(global_path.with_extension("db-shm"));
            return Ok(xdg_path);
        }
    }

    // 7. New installation: per-project XDG default
    Ok(xdg_path)
}

fn open_db(
    project_root: &Path,
    explicit_db_path: Option<&Path>,
    config_db_path: Option<&str>,
) -> Result<Connection> {
    let db_path = resolve_db_path(project_root, explicit_db_path, config_db_path)?;

    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700))?;
        }
    }

    let conn = Connection::open(&db_path)?;

    conn.execute_batch("PRAGMA journal_mode=WAL;")?;
    conn.execute_batch("PRAGMA foreign_keys=ON;")?;
    conn.execute_batch("PRAGMA busy_timeout=5000;")?;

    run_migrations(&conn)?;

    Ok(conn)
}

fn migrate_dod_checked(conn: &Connection) -> Result<()> {
    let has_checked: bool = conn
        .prepare("PRAGMA table_info(task_definition_of_done)")?
        .query_map([], |row| row.get::<_, String>(1))?
        .any(|name| name.as_deref() == Ok("checked"));
    if !has_checked {
        conn.execute_batch(
            "ALTER TABLE task_definition_of_done ADD COLUMN checked INTEGER NOT NULL DEFAULT 0",
        )?;
    }
    Ok(())
}

/// Legacy migration for pre-migration-system databases.
/// Only called when upgrading an existing DB that lacks schema_migrations.
fn migrate_legacy(conn: &Connection) -> Result<()> {
    // Add branch column if it doesn't exist (for databases created before this field)
    let has_branch: bool = conn
        .prepare("SELECT branch FROM tasks LIMIT 0")
        .is_ok();
    if !has_branch {
        conn.execute_batch("ALTER TABLE tasks ADD COLUMN branch TEXT")?;
    }

    // Rename details → description and add plan column
    let has_description: bool = conn
        .prepare("SELECT description FROM tasks LIMIT 0")
        .is_ok();
    if !has_description {
        conn.execute_batch("ALTER TABLE tasks RENAME COLUMN details TO description")?;
        conn.execute_batch("ALTER TABLE tasks ADD COLUMN plan TEXT")?;
    }

    // Add metadata column if it doesn't exist
    let has_metadata: bool = conn
        .prepare("SELECT metadata FROM tasks LIMIT 0")
        .is_ok();
    if !has_metadata {
        conn.execute_batch("ALTER TABLE tasks ADD COLUMN metadata TEXT")?;
    }

    // Add pr_url column if it doesn't exist
    let has_pr_url: bool = conn
        .prepare("SELECT pr_url FROM tasks LIMIT 0")
        .is_ok();
    if !has_pr_url {
        conn.execute_batch("ALTER TABLE tasks ADD COLUMN pr_url TEXT")?;
    }

    Ok(())
}

// --- Project functions ---

fn create_project(conn: &Connection, params: &CreateProjectParams) -> Result<Project> {
    conn.execute(
        "INSERT INTO projects (name, description) VALUES (?1, ?2)",
        rusqlite::params![params.name, params.description],
    )?;
    let id = conn.last_insert_rowid();
    get_project(conn, id)
}

fn get_project(conn: &Connection, id: i64) -> Result<Project> {
    let (name, description, created_at): (String, Option<String>, String) = conn
        .query_row(
            "SELECT name, description, created_at FROM projects WHERE id = ?1",
            params![id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()?
        .ok_or(DomainError::ProjectNotFound)?;
    Ok(Project::new(id, name, description, created_at))
}

fn get_project_by_name(conn: &Connection, name: &str) -> Result<Project> {
    let (id, description, created_at): (i64, Option<String>, String) = conn
        .query_row(
            "SELECT id, description, created_at FROM projects WHERE name = ?1",
            params![name],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()?
        .ok_or(DomainError::ProjectNotFound)?;
    Ok(Project::new(id, name.to_string(), description, created_at))
}

fn list_projects(conn: &Connection) -> Result<Vec<Project>> {
    let mut stmt = conn.prepare("SELECT id, name, description, created_at FROM projects ORDER BY id")?;
    let projects = stmt
        .query_map([], |row| {
            Ok(Project::new(
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
            ))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(projects)
}

fn delete_project(conn: &Connection, id: i64) -> Result<()> {
    let affected = conn.execute("DELETE FROM projects WHERE id = ?1", params![id])?;
    if affected == 0 {
        return Err(DomainError::ProjectNotFound.into());
    }
    Ok(())
}

// --- User CRUD ---

fn create_user(conn: &Connection, params: &CreateUserParams) -> Result<User> {
    conn.execute(
        "INSERT INTO users (username, display_name, email) VALUES (?1, ?2, ?3)",
        rusqlite::params![params.username, params.display_name, params.email],
    )?;
    let id = conn.last_insert_rowid();
    get_user(conn, id)
}

fn get_user(conn: &Connection, id: i64) -> Result<User> {
    let (username, display_name, email, created_at): (String, Option<String>, Option<String>, String) = conn
        .query_row(
            "SELECT username, display_name, email, created_at FROM users WHERE id = ?1",
            rusqlite::params![id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .optional()?
        .ok_or(DomainError::UserNotFound)?;
    Ok(User::new(id, username, display_name, email, created_at))
}

fn get_user_by_username(conn: &Connection, username: &str) -> Result<User> {
    let (id, display_name, email, created_at): (i64, Option<String>, Option<String>, String) = conn
        .query_row(
            "SELECT id, display_name, email, created_at FROM users WHERE username = ?1",
            rusqlite::params![username],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .optional()?
        .ok_or(DomainError::UserNotFound)?;
    Ok(User::new(id, username.to_string(), display_name, email, created_at))
}

fn list_users(conn: &Connection) -> Result<Vec<User>> {
    let mut stmt = conn.prepare(
        "SELECT id, username, display_name, email, created_at FROM users ORDER BY id",
    )?;
    let users = stmt
        .query_map([], |row| {
            Ok(User::new(
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
            ))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(users)
}

fn delete_user(conn: &Connection, id: i64) -> Result<()> {
    let affected = conn.execute("DELETE FROM users WHERE id = ?1", rusqlite::params![id])?;
    if affected == 0 {
        return Err(DomainError::UserNotFound.into());
    }
    Ok(())
}

// --- API Key CRUD ---

fn create_api_key(conn: &Connection, user_id: i64, name: &str, new_key: &NewApiKey) -> Result<ApiKeyWithSecret> {
    get_user(conn, user_id)?;

    conn.execute(
        "INSERT INTO api_keys (user_id, key_hash, key_prefix, name) VALUES (?1, ?2, ?3, ?4)",
        params![user_id, new_key.key_hash, new_key.key_prefix, name],
    )?;
    let id = conn.last_insert_rowid();
    let created_at: String = conn.query_row(
        "SELECT created_at FROM api_keys WHERE id = ?1",
        params![id],
        |row| row.get(0),
    )?;

    Ok(ApiKeyWithSecret::new(
        id,
        user_id,
        new_key.raw_key.clone(),
        new_key.key_prefix.clone(),
        name.to_string(),
        created_at,
    ))
}

fn get_user_by_api_key(conn: &Connection, key_hash: &str) -> Result<User> {
    conn.execute(
        "UPDATE api_keys SET last_used_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now') WHERE key_hash = ?1",
        params![key_hash],
    )?;

    let (user_id,): (i64,) = conn
        .query_row(
            "SELECT user_id FROM api_keys WHERE key_hash = ?1",
            params![key_hash],
            |row| Ok((row.get(0)?,)),
        )
        .optional()?
        .ok_or(DomainError::ApiKeyNotFound)?;

    get_user(conn, user_id)
}

fn list_api_keys(conn: &Connection, user_id: i64) -> Result<Vec<ApiKey>> {
    let mut stmt = conn.prepare(
        "SELECT id, user_id, key_prefix, name, created_at, last_used_at FROM api_keys WHERE user_id = ?1 ORDER BY id",
    )?;
    let keys = stmt
        .query_map(params![user_id], |row| {
            Ok(ApiKey::new(
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                row.get(5)?,
            ))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(keys)
}

fn delete_api_key(conn: &Connection, key_id: i64) -> Result<()> {
    let affected = conn.execute("DELETE FROM api_keys WHERE id = ?1", params![key_id])?;
    if affected == 0 {
        return Err(DomainError::ApiKeyNotFound.into());
    }
    Ok(())
}

// --- Project Member CRUD ---

fn add_project_member(
    conn: &Connection,
    project_id: i64,
    params: &AddProjectMemberParams,
) -> Result<ProjectMember> {
    conn.execute(
        "INSERT INTO project_members (project_id, user_id, role) VALUES (?1, ?2, ?3)",
        rusqlite::params![project_id, params.user_id, params.role.to_string()],
    )?;
    let id = conn.last_insert_rowid();
    let created_at: String = conn.query_row(
        "SELECT created_at FROM project_members WHERE id = ?1",
        rusqlite::params![id],
        |row| row.get(0),
    )?;
    Ok(ProjectMember::new(id, project_id, params.user_id, params.role, created_at))
}

fn remove_project_member(conn: &Connection, project_id: i64, user_id: i64) -> Result<()> {
    let affected = conn.execute(
        "DELETE FROM project_members WHERE project_id = ?1 AND user_id = ?2",
        rusqlite::params![project_id, user_id],
    )?;
    if affected == 0 {
        return Err(DomainError::ProjectMemberNotFound.into());
    }
    Ok(())
}

fn list_project_members(conn: &Connection, project_id: i64) -> Result<Vec<ProjectMember>> {
    let mut stmt = conn.prepare(
        "SELECT id, user_id, role, created_at FROM project_members WHERE project_id = ?1 ORDER BY id",
    )?;
    let members = stmt
        .query_map(rusqlite::params![project_id], |row| {
            let role_str: String = row.get(2)?;
            Ok((row.get(0)?, row.get(1)?, role_str, row.get(3)?))
        })?
        .collect::<std::result::Result<Vec<(i64, i64, String, String)>, _>>()?;

    members
        .into_iter()
        .map(|(id, user_id, role_str, created_at)| {
            let role: Role = role_str
                .parse()
                .map_err(|e| anyhow::anyhow!("invalid role in database: {e}"))?;
            Ok(ProjectMember::new(id, project_id, user_id, role, created_at))
        })
        .collect()
}

fn get_project_member(conn: &Connection, project_id: i64, user_id: i64) -> Result<ProjectMember> {
    let (id, role_str, created_at): (i64, String, String) = conn
        .query_row(
            "SELECT id, role, created_at FROM project_members WHERE project_id = ?1 AND user_id = ?2",
            rusqlite::params![project_id, user_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()?
        .ok_or(DomainError::ProjectMemberNotFound)?;
    let role: Role = role_str.parse()?;
    Ok(ProjectMember::new(id, project_id, user_id, role, created_at))
}

fn update_member_role(
    conn: &Connection,
    project_id: i64,
    user_id: i64,
    role: Role,
) -> Result<ProjectMember> {
    let affected = conn.execute(
        "UPDATE project_members SET role = ?3 WHERE project_id = ?1 AND user_id = ?2",
        rusqlite::params![project_id, user_id, role.to_string()],
    )?;
    if affected == 0 {
        return Err(DomainError::ProjectMemberNotFound.into());
    }
    get_project_member(conn, project_id, user_id)
}

/// Verify that a task belongs to the given project.
fn verify_task_project(conn: &Connection, project_id: i64, task_id: i64) -> Result<()> {
    let actual_project_id: i64 = conn
        .query_row(
            "SELECT project_id FROM tasks WHERE id = ?1",
            params![task_id],
            |row| row.get(0),
        )
        .optional()?
        .ok_or(DomainError::TaskNotFound)?;
    if actual_project_id != project_id {
        return Err(DomainError::TaskNotFound.into());
    }
    Ok(())
}

// --- Task functions ---

fn create_task(conn: &Connection, project_id: i64, params: &CreateTaskParams) -> Result<Task> {
    // Verify project exists
    get_project(conn, project_id)?;
    let priority: i32 = params.priority.unwrap_or(Priority::P2).into();
    let metadata_str = params
        .metadata
        .as_ref()
        .map(|v| serde_json::to_string(v))
        .transpose()?;
    conn.execute(
        "INSERT INTO tasks (title, background, description, priority, branch, pr_url, metadata, project_id) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        rusqlite::params![params.title, params.background, params.description, priority, params.branch, params.pr_url, metadata_str, project_id],
    )?;
    let task_id = conn.last_insert_rowid();

    if let Some(ref branch) = params.branch {
        if branch.contains("${task_id}") {
            let expanded = task::expand_branch_template(branch, task_id);
            conn.execute(
                "UPDATE tasks SET branch = ?1 WHERE id = ?2",
                params![expanded, task_id],
            )?;
        }
    }

    for item in &params.definition_of_done {
        conn.execute(
            "INSERT INTO task_definition_of_done (task_id, content) VALUES (?1, ?2)",
            params![task_id, item],
        )?;
    }
    for item in &params.in_scope {
        conn.execute(
            "INSERT INTO task_in_scope (task_id, content) VALUES (?1, ?2)",
            params![task_id, item],
        )?;
    }
    for item in &params.out_of_scope {
        conn.execute(
            "INSERT INTO task_out_of_scope (task_id, content) VALUES (?1, ?2)",
            params![task_id, item],
        )?;
    }
    for tag in &params.tags {
        conn.execute(
            "INSERT INTO task_tags (task_id, tag) VALUES (?1, ?2)",
            params![task_id, tag],
        )?;
    }
    for dep_id in &params.dependencies {
        conn.execute(
            "INSERT INTO task_dependencies (task_id, depends_on_task_id) VALUES (?1, ?2)",
            params![task_id, dep_id],
        )?;
    }

    get_task(conn, task_id)
}

fn get_task(conn: &Connection, id: i64) -> Result<Task> {
    let (project_id, title, background, description, plan, status_str, priority_val, assignee_session_id, created_at, updated_at, started_at, completed_at, canceled_at, cancel_reason, branch, pr_url, metadata_str, assignee_user_id): (
        i64, String, Option<String>, Option<String>, Option<String>, String, i32, Option<String>, String, String, Option<String>, Option<String>, Option<String>, Option<String>, Option<String>, Option<String>, Option<String>, Option<i64>,
    ) = conn
        .query_row(
            "SELECT project_id, title, background, description, plan, status, priority, assignee_session_id, created_at, updated_at, started_at, completed_at, canceled_at, cancel_reason, branch, pr_url, metadata, assignee_user_id FROM tasks WHERE id = ?1",
            params![id],
            |row| {
                Ok((
                    row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?,
                    row.get(4)?, row.get(5)?, row.get(6)?, row.get(7)?,
                    row.get(8)?, row.get(9)?, row.get(10)?, row.get(11)?,
                    row.get(12)?, row.get(13)?, row.get(14)?, row.get(15)?,
                    row.get(16)?, row.get(17)?,
                ))
            },
        )
        .optional()?
        .ok_or(DomainError::TaskNotFound)?;

    let status: TaskStatus = status_str.parse()?;
    let priority = Priority::try_from(priority_val)?;
    let metadata: Option<serde_json::Value> = metadata_str
        .map(|s| serde_json::from_str(&s))
        .transpose()
        .context("invalid metadata JSON in database")?;

    let definition_of_done = query_dod_list(conn, id)?;
    let in_scope = query_string_list(
        conn,
        "SELECT content FROM task_in_scope WHERE task_id = ?1",
        id,
    )?;
    let out_of_scope = query_string_list(
        conn,
        "SELECT content FROM task_out_of_scope WHERE task_id = ?1",
        id,
    )?;
    let tags = query_string_list(conn, "SELECT tag FROM task_tags WHERE task_id = ?1", id)?;
    let dependencies = query_i64_list(
        conn,
        "SELECT depends_on_task_id FROM task_dependencies WHERE task_id = ?1",
        id,
    )?;

    Ok(Task::new(
        id,
        project_id,
        title,
        background,
        description,
        plan,
        priority,
        status,
        assignee_session_id,
        assignee_user_id,
        created_at,
        updated_at,
        started_at,
        completed_at,
        canceled_at,
        cancel_reason,
        branch,
        pr_url,
        metadata,
        definition_of_done,
        in_scope,
        out_of_scope,
        tags,
        dependencies,
    ))
}

fn update_task(conn: &Connection, id: i64, params: &UpdateTaskParams) -> Result<Task> {
    let mut columns = Vec::new();
    let mut values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

    if let Some(ref title) = params.title {
        columns.push(TaskColumn::Title);
        values.push(Box::new(title.clone()));
    }
    if let Some(ref background) = params.background {
        columns.push(TaskColumn::Background);
        values.push(Box::new(background.clone()));
    }
    if let Some(ref description) = params.description {
        columns.push(TaskColumn::Description);
        values.push(Box::new(description.clone()));
    }
    if let Some(ref plan) = params.plan {
        columns.push(TaskColumn::Plan);
        values.push(Box::new(plan.clone()));
    }
    if let Some(priority) = params.priority {
        columns.push(TaskColumn::Priority);
        values.push(Box::new(i32::from(priority)));
    }
    if let Some(ref assignee) = params.assignee_session_id {
        columns.push(TaskColumn::AssigneeSessionId);
        values.push(Box::new(assignee.clone()));
    }
    if let Some(ref assignee_user_id) = params.assignee_user_id {
        columns.push(TaskColumn::AssigneeUserId);
        values.push(Box::new(*assignee_user_id));
    }
    if let Some(ref started_at) = params.started_at {
        columns.push(TaskColumn::StartedAt);
        values.push(Box::new(started_at.clone()));
    }
    if let Some(ref completed_at) = params.completed_at {
        columns.push(TaskColumn::CompletedAt);
        values.push(Box::new(completed_at.clone()));
    }
    if let Some(ref canceled_at) = params.canceled_at {
        columns.push(TaskColumn::CanceledAt);
        values.push(Box::new(canceled_at.clone()));
    }
    if let Some(ref cancel_reason) = params.cancel_reason {
        columns.push(TaskColumn::CancelReason);
        values.push(Box::new(cancel_reason.clone()));
    }
    if let Some(ref branch) = params.branch {
        columns.push(TaskColumn::Branch);
        values.push(Box::new(branch.clone()));
    }
    if let Some(ref pr_url) = params.pr_url {
        columns.push(TaskColumn::PrUrl);
        values.push(Box::new(pr_url.clone()));
    }
    if let Some(ref metadata) = params.metadata {
        columns.push(TaskColumn::Metadata);
        let metadata_str: Option<String> = metadata
            .as_ref()
            .map(|v| serde_json::to_string(v))
            .transpose()
            .map_err(|e| anyhow::anyhow!("failed to serialize metadata: {e}"))?;
        values.push(Box::new(metadata_str));
    }

    if !columns.is_empty() {
        let set_clause: Vec<String> = columns.iter().map(|c| format!("{} = ?", c.as_str())).collect();
        let sql = format!("UPDATE tasks SET {}, updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now') WHERE id = ?", set_clause.join(", "));
        values.push(Box::new(id));
        let param_refs: Vec<&dyn rusqlite::types::ToSql> = values.iter().map(|v| v.as_ref()).collect();
        conn.execute(&sql, param_refs.as_slice())?;
    }

    get_task(conn, id)
}

fn update_task_arrays(conn: &Connection, id: i64, params: &UpdateTaskArrayParams) -> Result<()> {
    // tags
    if let Some(ref values) = params.set_tags {
        conn.execute("DELETE FROM task_tags WHERE task_id = ?1", params![id])?;
        for tag in values {
            conn.execute(
                "INSERT INTO task_tags (task_id, tag) VALUES (?1, ?2)",
                params![id, tag],
            )?;
        }
    }
    for tag in &params.add_tags {
        conn.execute(
            "INSERT OR IGNORE INTO task_tags (task_id, tag) VALUES (?1, ?2)",
            params![id, tag],
        )?;
    }
    for tag in &params.remove_tags {
        conn.execute(
            "DELETE FROM task_tags WHERE task_id = ?1 AND tag = ?2",
            params![id, tag],
        )?;
    }

    // definition_of_done
    update_content_array(conn, id, ContentTable::DefinitionOfDone, &params.set_definition_of_done, &params.add_definition_of_done, &params.remove_definition_of_done)?;
    // in_scope
    update_content_array(conn, id, ContentTable::InScope, &params.set_in_scope, &params.add_in_scope, &params.remove_in_scope)?;
    // out_of_scope
    update_content_array(conn, id, ContentTable::OutOfScope, &params.set_out_of_scope, &params.add_out_of_scope, &params.remove_out_of_scope)?;

    // Touch updated_at
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
        conn.execute(
            "UPDATE tasks SET updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now') WHERE id = ?1",
            params![id],
        )?;
    }

    Ok(())
}

fn save_task(conn: &Connection, task: &Task) -> Result<()> {
    let metadata_str: Option<String> = task
        .metadata()
        .map(|v| serde_json::to_string(v))
        .transpose()
        .map_err(|e| anyhow::anyhow!("failed to serialize metadata: {e}"))?;

    conn.execute(
        "UPDATE tasks SET
            title = ?2, background = ?3, description = ?4, plan = ?5,
            priority = ?6, status = ?7,
            assignee_session_id = ?8, assignee_user_id = ?9,
            started_at = ?10, completed_at = ?11, canceled_at = ?12, cancel_reason = ?13,
            branch = ?14, pr_url = ?15, metadata = ?16,
            updated_at = ?17
        WHERE id = ?1",
        params![
            task.id(),
            task.title(),
            task.background(),
            task.description(),
            task.plan(),
            i32::from(task.priority()),
            task.status().to_string(),
            task.assignee_session_id(),
            task.assignee_user_id(),
            task.started_at(),
            task.completed_at(),
            task.canceled_at(),
            task.cancel_reason(),
            task.branch(),
            task.pr_url(),
            metadata_str,
            task.updated_at(),
        ],
    )?;

    // Sync definition_of_done
    conn.execute(
        "DELETE FROM task_definition_of_done WHERE task_id = ?1",
        params![task.id()],
    )?;
    for dod in task.definition_of_done() {
        let checked_val: i32 = if dod.checked() { 1 } else { 0 };
        conn.execute(
            "INSERT INTO task_definition_of_done (task_id, content, checked) VALUES (?1, ?2, ?3)",
            params![task.id(), dod.content(), checked_val],
        )?;
    }

    // Sync dependencies
    conn.execute(
        "DELETE FROM task_dependencies WHERE task_id = ?1",
        params![task.id()],
    )?;
    for &dep_id in task.dependencies() {
        conn.execute(
            "INSERT INTO task_dependencies (task_id, depends_on_task_id) VALUES (?1, ?2)",
            params![task.id(), dep_id],
        )?;
    }

    Ok(())
}

enum TaskColumn {
    Title,
    Background,
    Description,
    Plan,
    Priority,
    AssigneeSessionId,
    AssigneeUserId,
    StartedAt,
    CompletedAt,
    CanceledAt,
    CancelReason,
    Branch,
    PrUrl,
    Metadata,
}

impl TaskColumn {
    fn as_str(&self) -> &'static str {
        match self {
            TaskColumn::Title => "title",
            TaskColumn::Background => "background",
            TaskColumn::Description => "description",
            TaskColumn::Plan => "plan",
            TaskColumn::Priority => "priority",
            TaskColumn::AssigneeSessionId => "assignee_session_id",
            TaskColumn::AssigneeUserId => "assignee_user_id",
            TaskColumn::StartedAt => "started_at",
            TaskColumn::CompletedAt => "completed_at",
            TaskColumn::CanceledAt => "canceled_at",
            TaskColumn::CancelReason => "cancel_reason",
            TaskColumn::Branch => "branch",
            TaskColumn::PrUrl => "pr_url",
            TaskColumn::Metadata => "metadata",
        }
    }
}

enum ContentTable {
    DefinitionOfDone,
    InScope,
    OutOfScope,
}

impl ContentTable {
    fn as_str(&self) -> &'static str {
        match self {
            ContentTable::DefinitionOfDone => "task_definition_of_done",
            ContentTable::InScope => "task_in_scope",
            ContentTable::OutOfScope => "task_out_of_scope",
        }
    }
}

fn update_content_array(
    conn: &Connection,
    task_id: i64,
    table: ContentTable,
    set: &Option<Vec<String>>,
    add: &[String],
    remove: &[String],
) -> Result<()> {
    let table = table.as_str();
    if let Some(values) = set {
        conn.execute(&format!("DELETE FROM {table} WHERE task_id = ?1"), params![task_id])?;
        for item in values {
            conn.execute(
                &format!("INSERT INTO {table} (task_id, content) VALUES (?1, ?2)"),
                params![task_id, item],
            )?;
        }
    }
    for item in add {
        conn.execute(
            &format!("INSERT INTO {table} (task_id, content) VALUES (?1, ?2)"),
            params![task_id, item],
        )?;
    }
    for item in remove {
        conn.execute(
            &format!("DELETE FROM {table} WHERE task_id = ?1 AND content = ?2"),
            params![task_id, item],
        )?;
    }
    Ok(())
}

fn delete_task(conn: &Connection, id: i64) -> Result<()> {
    let affected = conn.execute("DELETE FROM tasks WHERE id = ?1", params![id])?;
    if affected == 0 {
        return Err(DomainError::TaskNotFound.into());
    }
    Ok(())
}

fn list_tasks(conn: &Connection, project_id: i64, filter: &ListTasksFilter) -> Result<Vec<Task>> {
    let mut conditions = Vec::new();
    let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

    conditions.push("t.project_id = ?".to_string());
    param_values.push(Box::new(project_id));

    if !filter.statuses.is_empty() {
        let placeholders: Vec<&str> = filter.statuses.iter().map(|_| "?").collect();
        conditions.push(format!("t.status IN ({})", placeholders.join(", ")));
        for s in &filter.statuses {
            param_values.push(Box::new(s.to_string()));
        }
    }

    if !filter.tags.is_empty() {
        let placeholders: Vec<&str> = filter.tags.iter().map(|_| "?").collect();
        conditions.push(format!(
            "EXISTS (SELECT 1 FROM task_tags tt WHERE tt.task_id = t.id AND tt.tag IN ({}))",
            placeholders.join(", ")
        ));
        for tag in &filter.tags {
            param_values.push(Box::new(tag.clone()));
        }
    }

    if let Some(dep_id) = filter.depends_on {
        conditions.push(
            "EXISTS (SELECT 1 FROM task_dependencies td WHERE td.task_id = t.id AND td.depends_on_task_id = ?)".to_string(),
        );
        param_values.push(Box::new(dep_id));
    }

    // SQL-optimized implementation of `crate::domain::task::filter_ready`.
    // Equivalence with domain logic is verified by integration tests.
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

    let sql = format!("SELECT t.id FROM tasks t{} ORDER BY t.id", where_clause);
    let param_refs: Vec<&dyn rusqlite::types::ToSql> =
        param_values.iter().map(|v| v.as_ref()).collect();

    let mut stmt = conn.prepare(&sql)?;
    let ids: Vec<i64> = stmt
        .query_map(param_refs.as_slice(), |row| row.get(0))?
        .collect::<std::result::Result<Vec<_>, _>>()?;

    let mut tasks = Vec::with_capacity(ids.len());
    for id in ids {
        tasks.push(get_task(conn, id)?);
    }
    Ok(tasks)
}

/// SQL-optimized implementation of [`crate::domain::task::select_next`].
/// Equivalence with domain logic is verified by integration tests.
fn next_task(conn: &Connection, project_id: i64) -> Result<Option<Task>> {
    let sql = "
        SELECT t.id FROM tasks t
        WHERE t.project_id = ?1
          AND t.status = 'todo'
          AND NOT EXISTS (
            SELECT 1 FROM task_dependencies td
            JOIN tasks dep ON dep.id = td.depends_on_task_id
            WHERE td.task_id = t.id AND dep.status != 'completed'
          )
        ORDER BY t.priority ASC, t.created_at ASC, t.id ASC
        LIMIT 1
    ";
    let id: Option<i64> = conn
        .query_row(sql, params![project_id], |row| row.get(0))
        .optional()?;
    match id {
        Some(id) => Ok(Some(get_task(conn, id)?)),
        None => Ok(None),
    }
}

fn task_stats(conn: &Connection, project_id: i64) -> Result<HashMap<String, i64>> {
    let mut stmt = conn.prepare("SELECT status, COUNT(*) FROM tasks WHERE project_id = ?1 GROUP BY status")?;
    let rows = stmt.query_map(params![project_id], |row| {
        let status: String = row.get(0)?;
        let count: i64 = row.get(1)?;
        Ok((status, count))
    })?;
    let mut stats = HashMap::new();
    for row in rows {
        let (status, count) = row?;
        stats.insert(status, count);
    }
    Ok(stats)
}

/// SQL-optimized implementation of ready-count, equivalent to
/// `crate::domain::task::filter_ready(...).len()`.
fn ready_count(conn: &Connection, project_id: i64) -> Result<i64> {
    let sql = "
        SELECT COUNT(*) FROM tasks t
        WHERE t.project_id = ?1
          AND t.status = 'todo'
          AND NOT EXISTS (
            SELECT 1 FROM task_dependencies td
            JOIN tasks dep ON dep.id = td.depends_on_task_id
            WHERE td.task_id = t.id AND dep.status != 'completed'
          )
    ";
    let count: i64 = conn.query_row(sql, params![project_id], |row| row.get(0))?;
    Ok(count)
}

fn list_ready_tasks(conn: &Connection, project_id: i64) -> Result<Vec<Task>> {
    let filter = ListTasksFilter {
        ready: true,
        ..Default::default()
    };
    list_tasks(conn, project_id, &filter)
}



fn list_dependencies(conn: &Connection, task_id: i64) -> Result<Vec<Task>> {
    get_task(conn, task_id)?;
    let dep_ids = query_i64_list(
        conn,
        "SELECT depends_on_task_id FROM task_dependencies WHERE task_id = ?1",
        task_id,
    )?;
    let mut tasks = Vec::with_capacity(dep_ids.len());
    for id in dep_ids {
        tasks.push(get_task(conn, id)?);
    }
    Ok(tasks)
}

fn query_dod_list(conn: &Connection, task_id: i64) -> Result<Vec<DodItem>> {
    let mut stmt = conn.prepare(
        "SELECT content, checked FROM task_definition_of_done WHERE task_id = ?1 ORDER BY id",
    )?;
    let items = stmt
        .query_map(params![task_id], |row| {
            Ok(DodItem::new(row.get(0)?, row.get::<_, i32>(1)? != 0))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(items)
}

fn query_string_list(conn: &Connection, sql: &str, task_id: i64) -> Result<Vec<String>> {
    let mut stmt = conn.prepare(sql)?;
    let items: Vec<String> = stmt
        .query_map(params![task_id], |row| row.get(0))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(items)
}

fn query_i64_list(conn: &Connection, sql: &str, task_id: i64) -> Result<Vec<i64>> {
    let mut stmt = conn.prepare(sql)?;
    let items: Vec<i64> = stmt
        .query_map(params![task_id], |row| row.get(0))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(items)
}

// --- Default record sync ---

fn update_project_name(conn: &Connection, id: i64, name: &str) -> Result<()> {
    conn.execute(
        "UPDATE projects SET name = ?1 WHERE id = ?2",
        params![name, id],
    )?;
    Ok(())
}

fn update_user_username(conn: &Connection, id: i64, username: &str) -> Result<()> {
    conn.execute(
        "UPDATE users SET username = ?1 WHERE id = ?2",
        params![username, id],
    )?;
    Ok(())
}

// --- SqliteBackend implementation ---

use std::sync::Arc;

use async_trait::async_trait;

use crate::application::port::{AuthenticationPort, ProjectQueryPort, TaskQueryPort, UserQueryPort};
use crate::infra::config::Config;
use crate::domain::{ApiKeyRepository, ProjectMemberRepository, ProjectRepository, TaskRepository, UserRepository};

pub struct SqliteBackend {
    conn: Arc<std::sync::Mutex<Connection>>,
}

impl SqliteBackend {
    pub fn new(
        project_root: &Path,
        explicit_db_path: Option<&Path>,
        config_db_path: Option<&str>,
    ) -> Result<Self> {
        let conn = open_db(project_root, explicit_db_path, config_db_path)?;
        Ok(Self {
            conn: Arc::new(std::sync::Mutex::new(conn)),
        })
    }

    /// Create a backend backed by an in-memory SQLite database.
    /// Useful for integration tests where no filesystem state is desired.
    pub fn new_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch("PRAGMA foreign_keys = ON;")?;
        run_migrations(&conn)?;
        Ok(Self {
            conn: Arc::new(std::sync::Mutex::new(conn)),
        })
    }

    /// Sync config.toml project/user names to the id=1 default records.
    /// Called once at backend creation time for SQLite single-mode usage.
    pub fn sync_config_defaults(&self, config: &Config) -> Result<()> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("mutex lock failed: {e}"))?;
        if let Some(ref name) = config.project.name {
            update_project_name(&conn, 1, name)
                .with_context(|| format!(
                    "failed to sync project name '{name}' to default project (id=1): name may already be used by another project"
                ))?;
        }
        if let Some(ref name) = config.user.name {
            update_user_username(&conn, 1, name)
                .with_context(|| format!(
                    "failed to sync user name '{name}' to default user (id=1): username may already be used by another user"
                ))?;
        }
        Ok(())
    }
}

macro_rules! blocking {
    ($self:ident, $body:expr) => {{
        let conn = $self.conn.clone();
        tokio::task::spawn_blocking(move || {
            let conn = conn.lock().map_err(|e| anyhow::anyhow!("mutex lock failed: {e}"))?;
            $body(&conn)
        }).await?
    }};
}

#[async_trait]
impl ProjectRepository for SqliteBackend {
    async fn create_project(&self, params: &CreateProjectParams) -> Result<Project> {
        let params = params.clone();
        blocking!(self, |conn: &Connection| create_project(conn, &params))
    }

    async fn get_project(&self, id: i64) -> Result<Project> {
        blocking!(self, |conn: &Connection| get_project(conn, id))
    }

    async fn get_project_by_name(&self, name: &str) -> Result<Project> {
        let name = name.to_owned();
        blocking!(self, |conn: &Connection| get_project_by_name(conn, &name))
    }

    async fn delete_project(&self, id: i64) -> Result<()> {
        blocking!(self, |conn: &Connection| delete_project(conn, id))
    }
}

#[async_trait]
impl ProjectMemberRepository for SqliteBackend {
    async fn add_project_member(&self, project_id: i64, params: &AddProjectMemberParams) -> Result<ProjectMember> {
        let params = params.clone();
        blocking!(self, |conn: &Connection| add_project_member(conn, project_id, &params))
    }

    async fn remove_project_member(&self, project_id: i64, user_id: i64) -> Result<()> {
        blocking!(self, |conn: &Connection| remove_project_member(conn, project_id, user_id))
    }

    async fn list_project_members(&self, project_id: i64) -> Result<Vec<ProjectMember>> {
        blocking!(self, |conn: &Connection| list_project_members(conn, project_id))
    }

    async fn get_project_member(&self, project_id: i64, user_id: i64) -> Result<ProjectMember> {
        blocking!(self, |conn: &Connection| get_project_member(conn, project_id, user_id))
    }

    async fn update_member_role(&self, project_id: i64, user_id: i64, role: Role) -> Result<ProjectMember> {
        blocking!(self, |conn: &Connection| update_member_role(conn, project_id, user_id, role))
    }
}

#[async_trait]
impl UserRepository for SqliteBackend {
    async fn create_user(&self, params: &CreateUserParams) -> Result<User> {
        let params = params.clone();
        blocking!(self, |conn: &Connection| create_user(conn, &params))
    }

    async fn get_user(&self, id: i64) -> Result<User> {
        blocking!(self, |conn: &Connection| get_user(conn, id))
    }

    async fn get_user_by_username(&self, username: &str) -> Result<User> {
        let username = username.to_owned();
        blocking!(self, |conn: &Connection| get_user_by_username(conn, &username))
    }

    async fn delete_user(&self, id: i64) -> Result<()> {
        blocking!(self, |conn: &Connection| delete_user(conn, id))
    }
}

#[async_trait]
impl AuthenticationPort for SqliteBackend {
    async fn get_user_by_api_key(&self, key_hash: &str) -> Result<User> {
        let key_hash = key_hash.to_owned();
        blocking!(self, |conn: &Connection| get_user_by_api_key(conn, &key_hash))
    }
}

#[async_trait]
impl ApiKeyRepository for SqliteBackend {
    async fn create_api_key(&self, user_id: i64, name: &str, new_key: &NewApiKey) -> Result<ApiKeyWithSecret> {
        let name = name.to_owned();
        let new_key = new_key.clone();
        blocking!(self, |conn: &Connection| create_api_key(conn, user_id, &name, &new_key))
    }

    async fn delete_api_key(&self, key_id: i64) -> Result<()> {
        blocking!(self, |conn: &Connection| delete_api_key(conn, key_id))
    }
}

#[async_trait]
impl ProjectQueryPort for SqliteBackend {
    async fn list_projects(&self) -> Result<Vec<Project>> {
        blocking!(self, |conn: &Connection| list_projects(conn))
    }
}

#[async_trait]
impl UserQueryPort for SqliteBackend {
    async fn list_users(&self) -> Result<Vec<User>> {
        blocking!(self, |conn: &Connection| list_users(conn))
    }

    async fn list_api_keys(&self, user_id: i64) -> Result<Vec<ApiKey>> {
        blocking!(self, |conn: &Connection| list_api_keys(conn, user_id))
    }
}

#[async_trait]
impl TaskRepository for SqliteBackend {
    async fn create_task(&self, project_id: i64, params: &CreateTaskParams) -> Result<Task> {
        let params = params.clone();
        blocking!(self, |conn: &Connection| create_task(conn, project_id, &params))
    }

    async fn get_task(&self, project_id: i64, id: i64) -> Result<Task> {
        blocking!(self, |conn: &Connection| {
            verify_task_project(conn, project_id, id)?;
            get_task(conn, id)
        })
    }

    async fn update_task(&self, project_id: i64, id: i64, params: &UpdateTaskParams) -> Result<Task> {
        let params = params.clone();
        blocking!(self, |conn: &Connection| {
            verify_task_project(conn, project_id, id)?;
            update_task(conn, id, &params)
        })
    }

    async fn update_task_arrays(&self, project_id: i64, id: i64, params: &UpdateTaskArrayParams) -> Result<()> {
        let params = params.clone();
        blocking!(self, |conn: &Connection| {
            verify_task_project(conn, project_id, id)?;
            update_task_arrays(conn, id, &params)
        })
    }

    async fn delete_task(&self, project_id: i64, id: i64) -> Result<()> {
        blocking!(self, |conn: &Connection| {
            verify_task_project(conn, project_id, id)?;
            delete_task(conn, id)
        })
    }

    async fn list_dependencies(&self, project_id: i64, task_id: i64) -> Result<Vec<Task>> {
        blocking!(self, |conn: &Connection| {
            verify_task_project(conn, project_id, task_id)?;
            list_dependencies(conn, task_id)
        })
    }

    async fn save(&self, task: &Task) -> Result<()> {
        let task = task.clone();
        blocking!(self, |conn: &Connection| save_task(conn, &task))
    }
}

#[async_trait]
impl TaskQueryPort for SqliteBackend {
    async fn list_tasks(&self, project_id: i64, filter: &ListTasksFilter) -> Result<Vec<Task>> {
        let filter = filter.clone();
        blocking!(self, |conn: &Connection| list_tasks(conn, project_id, &filter))
    }

    async fn next_task(&self, project_id: i64) -> Result<Option<Task>> {
        blocking!(self, |conn: &Connection| next_task(conn, project_id))
    }

    async fn task_stats(&self, project_id: i64) -> Result<HashMap<String, i64>> {
        blocking!(self, |conn: &Connection| task_stats(conn, project_id))
    }

    async fn ready_count(&self, project_id: i64) -> Result<i64> {
        blocking!(self, |conn: &Connection| ready_count(conn, project_id))
    }

    async fn list_ready_tasks(&self, project_id: i64) -> Result<Vec<Task>> {
        blocking!(self, |conn: &Connection| list_ready_tasks(conn, project_id))
    }
}

crate::impl_task_transition_default!(SqliteBackend);

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> (tempfile::TempDir, Connection) {
        let tmp = tempfile::tempdir().unwrap();
        let db_path = tmp.path().join("data.db");
        let conn = open_db(tmp.path(), Some(db_path.as_path()), None).unwrap();
        (tmp, conn)
    }

    fn default_create_params(title: &str) -> CreateTaskParams {
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

    /// Helper to transition a task through statuses using domain methods + save
    fn transition_to(conn: &Connection, id: i64, target: TaskStatus) {
        let task = get_task(conn, id).unwrap();
        match target {
            TaskStatus::Draft => {} // already draft
            TaskStatus::Todo => {
                let (task, _) = task.ready("2025-01-01T00:00:00Z".to_string()).unwrap();
                save_task(conn, &task).unwrap();
            }
            TaskStatus::InProgress => {
                let (task, _) = task.ready("2025-01-01T00:00:00Z".to_string()).unwrap();
                let (task, _) = task.start(None, None, "2025-01-01T00:00:00Z".to_string(), None).unwrap();
                save_task(conn, &task).unwrap();
            }
            TaskStatus::Completed => {
                let (task, _) = task.ready("2025-01-01T00:00:00Z".to_string()).unwrap();
                let (task, _) = task.start(None, None, "2025-01-01T00:00:00Z".to_string(), None).unwrap();
                let (task, _) = task.complete("2025-01-01T00:00:00Z".to_string()).unwrap();
                save_task(conn, &task).unwrap();
            }
            TaskStatus::Canceled => {
                let (task, _) = task.cancel("2025-01-01T00:00:00Z".to_string(), None).unwrap();
                save_task(conn, &task).unwrap();
            }
        }
    }

    #[test]
    fn creates_db_at_explicit_path() {
        let tmp = tempfile::tempdir().unwrap();
        let db_path = tmp.path().join("custom.db");
        let conn = open_db(tmp.path(), Some(db_path.as_path()), None).unwrap();
        assert!(db_path.exists());
        drop(conn);
    }

    #[test]
    fn tables_exist() {
        let (_tmp, conn) = setup();

        let tables: Vec<String> = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();

        assert!(tables.contains(&"tasks".to_string()));
        assert!(tables.contains(&"task_definition_of_done".to_string()));
        assert!(tables.contains(&"task_in_scope".to_string()));
        assert!(tables.contains(&"task_out_of_scope".to_string()));
        assert!(tables.contains(&"task_tags".to_string()));
        assert!(tables.contains(&"task_dependencies".to_string()));
        assert!(tables.contains(&"schema_migrations".to_string()));
        assert!(tables.contains(&"projects".to_string()));
    }

    #[test]
    fn wal_mode_enabled() {
        let (_tmp, conn) = setup();
        let mode: String = conn
            .query_row("PRAGMA journal_mode", [], |row| row.get(0))
            .unwrap();
        assert_eq!(mode, "wal");
    }

    #[test]
    fn foreign_keys_enabled() {
        let (_tmp, conn) = setup();
        let fk: i32 = conn
            .query_row("PRAGMA foreign_keys", [], |row| row.get(0))
            .unwrap();
        assert_eq!(fk, 1);
    }

    #[test]
    fn busy_timeout_set() {
        let (_tmp, conn) = setup();
        let timeout: i32 = conn
            .query_row("PRAGMA busy_timeout", [], |row| row.get(0))
            .unwrap();
        assert_eq!(timeout, 5000);
    }

    #[test]
    fn idempotent_open() {
        let tmp = tempfile::tempdir().unwrap();
        let db_path = tmp.path().join("data.db");
        let _conn1 = open_db(tmp.path(), Some(db_path.as_path()), None).unwrap();
        drop(_conn1);
        let _conn2 = open_db(tmp.path(), Some(db_path.as_path()), None).unwrap();
    }

    #[test]
    fn create_and_get_task() {
        let (_tmp, conn) = setup();
        let task = create_task(
            &conn,
            1,
            &CreateTaskParams {
                title: "Test task".to_string(),
                background: Some("bg".to_string()),
                description: Some("det".to_string()),
                priority: Some(Priority::P1),
                definition_of_done: vec!["done1".to_string(), "done2".to_string()],
                in_scope: vec!["scope1".to_string()],
                out_of_scope: vec!["out1".to_string()],
                branch: None,
                pr_url: None,
                metadata: None,
                tags: vec!["rust".to_string(), "cli".to_string()],
                dependencies: vec![],
            },
        )
        .unwrap();

        assert_eq!(task.title(), "Test task");
        assert_eq!(task.background(), Some("bg"));
        assert_eq!(task.description(), Some("det"));
        assert_eq!(task.priority(), Priority::P1);
        assert_eq!(task.status(), TaskStatus::Draft);
        assert_eq!(
            task.definition_of_done(),
            &[
                DodItem::new("done1".to_string(), false),
                DodItem::new("done2".to_string(), false),
            ]
        );
        assert_eq!(task.in_scope(), &["scope1"]);
        assert_eq!(task.out_of_scope(), &["out1"]);
        assert_eq!(task.tags().len(), 2);
        assert!(task.tags().contains(&"rust".to_string()));
        assert!(task.tags().contains(&"cli".to_string()));
        assert!(task.dependencies().is_empty());
        assert!(task.assignee_session_id().is_none());
        assert!(task.started_at().is_none());
        assert!(task.canceled_at().is_none());
        assert!(task.cancel_reason().is_none());

        let fetched = get_task(&conn, task.id()).unwrap();
        assert_eq!(fetched.title(), task.title());
        assert_eq!(fetched.tags(), task.tags());
    }

    #[test]
    fn create_task_default_priority() {
        let (_tmp, conn) = setup();
        let task = create_task(&conn, 1, &default_create_params("default prio")).unwrap();
        assert_eq!(task.priority(), Priority::P2);
    }

    #[test]
    fn update_task_fields() {
        let (_tmp, conn) = setup();
        let task = create_task(&conn, 1, &default_create_params("original")).unwrap();

        let updated = update_task(
            &conn,
            task.id(),
            &UpdateTaskParams {
                title: Some("updated".to_string()),
                background: Some(Some("new bg".to_string())),
                description: Some(Some("new description".to_string())),
                plan: None,
                priority: Some(Priority::P0),
                assignee_session_id: Some(Some("session-1".to_string())),
                assignee_user_id: None,
                started_at: None,
                completed_at: None,
                canceled_at: None,
                cancel_reason: None,
                branch: None,
                pr_url: None,
                metadata: None,
            },
        )
        .unwrap();

        assert_eq!(updated.title(), "updated");
        assert_eq!(updated.background(), Some("new bg"));
        assert_eq!(updated.description(), Some("new description"));
        assert_eq!(updated.priority(), Priority::P0);
        assert_eq!(updated.assignee_session_id(), Some("session-1"));
        assert!(updated.updated_at() >= task.updated_at());
    }

    #[test]
    fn status_transition_saved() {
        let (_tmp, conn) = setup();
        let task = create_task(&conn, 1, &default_create_params("t")).unwrap();
        assert_eq!(task.status(), TaskStatus::Draft);

        // draft -> todo via domain method + save
        let task = get_task(&conn, task.id()).unwrap();
        let (task, _) = task.ready("2025-01-01T00:00:00Z".to_string()).unwrap();
        save_task(&conn, &task).unwrap();
        let updated = get_task(&conn, task.id()).unwrap();
        assert_eq!(updated.status(), TaskStatus::Todo);

        // todo -> in_progress
        let (task, _) = updated.start(Some("session-1".into()), None, "2025-01-01T00:00:00Z".to_string(), None).unwrap();
        save_task(&conn, &task).unwrap();
        let updated = get_task(&conn, task.id()).unwrap();
        assert_eq!(updated.status(), TaskStatus::InProgress);
        assert_eq!(updated.assignee_session_id(), Some("session-1"));
        assert_eq!(updated.started_at(), Some("2025-01-01T00:00:00Z"));

        // in_progress -> completed
        let (task, _) = updated.complete("2025-01-01T01:00:00Z".to_string()).unwrap();
        save_task(&conn, &task).unwrap();
        let updated = get_task(&conn, task.id()).unwrap();
        assert_eq!(updated.status(), TaskStatus::Completed);
        assert_eq!(updated.completed_at(), Some("2025-01-01T01:00:00Z"));
    }

    #[test]
    fn cancel_task_from_any_active_status() {
        let (_tmp, conn) = setup();

        // cancel from draft
        let t1 = create_task(&conn, 1, &default_create_params("t1")).unwrap();
        let task = get_task(&conn, t1.id()).unwrap();
        let (task, _) = task.cancel("2025-01-01T00:00:00Z".to_string(), Some("reason1".into())).unwrap();
        save_task(&conn, &task).unwrap();
        let canceled = get_task(&conn, t1.id()).unwrap();
        assert_eq!(canceled.status(), TaskStatus::Canceled);
        assert_eq!(canceled.cancel_reason(), Some("reason1"));

        // cancel from todo
        let t2 = create_task(&conn, 1, &default_create_params("t2")).unwrap();
        transition_to(&conn, t2.id(), TaskStatus::Todo);
        let task = get_task(&conn, t2.id()).unwrap();
        let (task, _) = task.cancel("2025-01-01T00:00:00Z".to_string(), None).unwrap();
        save_task(&conn, &task).unwrap();
        let canceled = get_task(&conn, t2.id()).unwrap();
        assert_eq!(canceled.status(), TaskStatus::Canceled);

        // cancel from in_progress
        let t3 = create_task(&conn, 1, &default_create_params("t3")).unwrap();
        transition_to(&conn, t3.id(), TaskStatus::InProgress);
        let task = get_task(&conn, t3.id()).unwrap();
        let (task, _) = task.cancel("2025-01-01T00:00:00Z".to_string(), None).unwrap();
        save_task(&conn, &task).unwrap();
        let canceled = get_task(&conn, t3.id()).unwrap();
        assert_eq!(canceled.status(), TaskStatus::Canceled);
    }

    #[test]
    fn delete_task_cascade() {
        let (_tmp, conn) = setup();
        let task = create_task(
            &conn,
            1,
            &CreateTaskParams {
                title: "to delete".to_string(),
                background: None,
                description: None,
                priority: None,
                definition_of_done: vec!["d".to_string()],
                in_scope: vec!["s".to_string()],
                out_of_scope: vec!["o".to_string()],
                branch: None,
                pr_url: None,
                metadata: None,
                tags: vec!["tag".to_string()],
                dependencies: vec![],
            },
        )
        .unwrap();

        delete_task(&conn, task.id()).unwrap();

        assert!(get_task(&conn, task.id()).is_err());

        let count: i32 = conn
            .query_row(
                "SELECT COUNT(*) FROM task_tags WHERE task_id = ?1",
                params![task.id()],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 0);

        let count: i32 = conn
            .query_row(
                "SELECT COUNT(*) FROM task_definition_of_done WHERE task_id = ?1",
                params![task.id()],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn delete_nonexistent_task() {
        let (_tmp, conn) = setup();
        assert!(delete_task(&conn, 99999).is_err());
    }

    #[test]
    fn list_tasks_no_filter() {
        let (_tmp, conn) = setup();
        create_task(&conn, 1, &default_create_params("a")).unwrap();
        create_task(&conn, 1, &default_create_params("b")).unwrap();

        let tasks = list_tasks(&conn, 1, &ListTasksFilter::default()).unwrap();
        assert_eq!(tasks.len(), 2);
    }

    #[test]
    fn list_tasks_filter_by_status() {
        let (_tmp, conn) = setup();
        let t1 = create_task(&conn, 1, &default_create_params("draft")).unwrap();
        let _t2 = create_task(&conn, 1, &default_create_params("todo")).unwrap();

        // Move t1 to todo
        transition_to(&conn, t1.id(), TaskStatus::Todo);

        let drafts = list_tasks(
            &conn,
            1,
            &ListTasksFilter {
                statuses: vec![TaskStatus::Draft],
                tags: vec![],
                depends_on: None,
                ready: false,
            },
        )
        .unwrap();
        assert_eq!(drafts.len(), 1);
        assert_eq!(drafts[0].title(), "todo");

        let todos = list_tasks(
            &conn,
            1,
            &ListTasksFilter {
                statuses: vec![TaskStatus::Todo],
                tags: vec![],
                depends_on: None,
                ready: false,
            },
        )
        .unwrap();
        assert_eq!(todos.len(), 1);
        assert_eq!(todos[0].title(), "draft");
    }

    #[test]
    fn list_tasks_filter_by_tag() {
        let (_tmp, conn) = setup();
        create_task(
            &conn,
            1,
            &CreateTaskParams {
                title: "tagged".to_string(),
                branch: None,
                pr_url: None,
                metadata: None,
                tags: vec!["rust".to_string()],
                ..default_create_params("tagged")
            },
        )
        .unwrap();
        create_task(&conn, 1, &default_create_params("untagged")).unwrap();

        let result = list_tasks(
            &conn,
            1,
            &ListTasksFilter {
                statuses: vec![],
                tags: vec!["rust".to_string()],
                depends_on: None,
                ready: false,
            },
        )
        .unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].title(), "tagged");
    }

    #[test]
    fn list_tasks_ready_filter() {
        let (_tmp, conn) = setup();

        // Create dep task and move to completed
        let dep = create_task(&conn, 1, &default_create_params("dep")).unwrap();
        transition_to(&conn, dep.id(), TaskStatus::Completed);

        // Create task with completed dep -> should be ready
        let ready_t = create_task(
            &conn,
            1,
            &CreateTaskParams {
                title: "ready".to_string(),
                dependencies: vec![dep.id()],
                ..default_create_params("ready")
            },
        ).unwrap();
        transition_to(&conn, ready_t.id(), TaskStatus::Todo);

        // Create another dep that is NOT completed
        let dep2 = create_task(&conn, 1, &default_create_params("dep2")).unwrap();
        let blocked_task = create_task(
            &conn,
            1,
            &CreateTaskParams {
                title: "blocked".to_string(),
                dependencies: vec![dep2.id()],
                ..default_create_params("blocked")
            },
        ).unwrap();
        transition_to(&conn, blocked_task.id(), TaskStatus::Todo);

        let result = list_tasks(
            &conn,
            1,
            &ListTasksFilter {
                statuses: vec![],
                tags: vec![],
                depends_on: None,
                ready: true,
            },
        ).unwrap();

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].title(), "ready");
    }

    #[test]
    fn unique_constraints() {
        let (_tmp, conn) = setup();
        let task = create_task(
            &conn,
            1,
            &CreateTaskParams {
                title: "t1".to_string(),
                branch: None,
                pr_url: None,
                metadata: None,
                tags: vec!["rust".to_string()],
                ..default_create_params("t1")
            },
        )
        .unwrap();

        // Duplicate tag should fail
        let result = conn.execute(
            "INSERT INTO task_tags (task_id, tag) VALUES (?1, 'rust')",
            params![task.id()],
        );
        assert!(result.is_err());
    }

    #[test]
    fn task_with_dependencies() {
        let (_tmp, conn) = setup();
        let dep1 = create_task(&conn, 1, &default_create_params("dep1")).unwrap();
        let dep2 = create_task(&conn, 1, &default_create_params("dep2")).unwrap();

        let task = create_task(
            &conn,
            1,
            &CreateTaskParams {
                title: "with deps".to_string(),
                dependencies: vec![dep1.id(), dep2.id()],
                ..default_create_params("with deps")
            },
        )
        .unwrap();

        assert_eq!(task.dependencies().len(), 2);
        assert!(task.dependencies().contains(&dep1.id()));
        assert!(task.dependencies().contains(&dep2.id()));
    }

    fn default_array_params() -> UpdateTaskArrayParams {
        UpdateTaskArrayParams {
            set_tags: None,
            add_tags: vec![],
            remove_tags: vec![],
            set_definition_of_done: None,
            add_definition_of_done: vec![],
            remove_definition_of_done: vec![],
            set_in_scope: None,
            add_in_scope: vec![],
            remove_in_scope: vec![],
            set_out_of_scope: None,
            add_out_of_scope: vec![],
            remove_out_of_scope: vec![],
        }
    }

    #[test]
    fn update_arrays_set_tags() {
        let (_tmp, conn) = setup();
        let task = create_task(
            &conn,
            1,
            &CreateTaskParams {
                branch: None,
                pr_url: None,
                metadata: None,
                tags: vec!["old".to_string()],
                ..default_create_params("t")
            },
        )
        .unwrap();

        update_task_arrays(
            &conn,
            task.id(),
            &UpdateTaskArrayParams {
                set_tags: Some(vec!["new1".to_string(), "new2".to_string()]),
                ..default_array_params()
            },
        )
        .unwrap();

        let updated = get_task(&conn, task.id()).unwrap();
        assert_eq!(updated.tags().len(), 2);
        assert!(updated.tags().contains(&"new1".to_string()));
        assert!(updated.tags().contains(&"new2".to_string()));
        assert!(!updated.tags().contains(&"old".to_string()));
    }

    #[test]
    fn update_arrays_add_tags() {
        let (_tmp, conn) = setup();
        let task = create_task(
            &conn,
            1,
            &CreateTaskParams {
                branch: None,
                pr_url: None,
                metadata: None,
                tags: vec!["existing".to_string()],
                ..default_create_params("t")
            },
        )
        .unwrap();

        update_task_arrays(
            &conn,
            task.id(),
            &UpdateTaskArrayParams {
                add_tags: vec!["new".to_string(), "existing".to_string()],
                ..default_array_params()
            },
        )
        .unwrap();

        let updated = get_task(&conn, task.id()).unwrap();
        assert_eq!(updated.tags().len(), 2);
        assert!(updated.tags().contains(&"existing".to_string()));
        assert!(updated.tags().contains(&"new".to_string()));
    }

    #[test]
    fn update_arrays_remove_tags() {
        let (_tmp, conn) = setup();
        let task = create_task(
            &conn,
            1,
            &CreateTaskParams {
                branch: None,
                pr_url: None,
                metadata: None,
                tags: vec!["keep".to_string(), "remove".to_string()],
                ..default_create_params("t")
            },
        )
        .unwrap();

        update_task_arrays(
            &conn,
            task.id(),
            &UpdateTaskArrayParams {
                remove_tags: vec!["remove".to_string()],
                ..default_array_params()
            },
        )
        .unwrap();

        let updated = get_task(&conn, task.id()).unwrap();
        assert_eq!(updated.tags(), &["keep"]);
    }

    #[test]
    fn update_arrays_set_definition_of_done() {
        let (_tmp, conn) = setup();
        let task = create_task(
            &conn,
            1,
            &CreateTaskParams {
                definition_of_done: vec!["old".to_string()],
                ..default_create_params("t")
            },
        )
        .unwrap();

        update_task_arrays(
            &conn,
            task.id(),
            &UpdateTaskArrayParams {
                set_definition_of_done: Some(vec!["new1".to_string(), "new2".to_string()]),
                ..default_array_params()
            },
        )
        .unwrap();

        let updated = get_task(&conn, task.id()).unwrap();
        assert_eq!(
            updated.definition_of_done(),
            &[
                DodItem::new("new1".to_string(), false),
                DodItem::new("new2".to_string(), false),
            ]
        );
    }

    #[test]
    fn update_arrays_add_and_remove_in_scope() {
        let (_tmp, conn) = setup();
        let task = create_task(
            &conn,
            1,
            &CreateTaskParams {
                in_scope: vec!["a".to_string(), "b".to_string()],
                ..default_create_params("t")
            },
        )
        .unwrap();

        update_task_arrays(
            &conn,
            task.id(),
            &UpdateTaskArrayParams {
                add_in_scope: vec!["c".to_string()],
                remove_in_scope: vec!["a".to_string()],
                ..default_array_params()
            },
        )
        .unwrap();

        let updated = get_task(&conn, task.id()).unwrap();
        assert_eq!(updated.in_scope(), &["b", "c"]);
    }

    fn make_todo(conn: &Connection, title: &str, priority: Option<Priority>) -> Task {
        let task = create_task(
            conn,
            1,
            &CreateTaskParams {
                priority,
                ..default_create_params(title)
            },
        )
        .unwrap();
        transition_to(conn, task.id(), TaskStatus::Todo);
        get_task(conn, task.id()).unwrap()
    }

    fn make_completed(conn: &Connection, title: &str) -> Task {
        let task = create_task(conn, 1, &default_create_params(title)).unwrap();
        transition_to(conn, task.id(), TaskStatus::Completed);
        get_task(conn, task.id()).unwrap()
    }

    #[test]
    fn next_task_returns_none_when_empty() {
        let (_tmp, conn) = setup();
        assert!(next_task(&conn, 1).unwrap().is_none());
    }

    #[test]
    fn next_task_skips_blocked() {
        let (_tmp, conn) = setup();

        // Create a dep that is NOT completed (still draft)
        let dep = create_task(&conn, 1, &default_create_params("dep")).unwrap();

        // Create a todo task that depends on dep
        let task = create_task(
            &conn,
            1,
            &CreateTaskParams {
                title: "blocked".to_string(),
                dependencies: vec![dep.id()],
                ..default_create_params("blocked")
            },
        ).unwrap();
        transition_to(&conn, task.id(), TaskStatus::Todo);

        assert!(next_task(&conn, 1).unwrap().is_none());
    }

    #[test]
    fn next_task_priority_order() {
        let (_tmp, conn) = setup();

        make_todo(&conn, "low", Some(Priority::P3));
        make_todo(&conn, "high", Some(Priority::P0));
        make_todo(&conn, "mid", Some(Priority::P1));

        let task = next_task(&conn, 1).unwrap().unwrap();
        assert_eq!(task.title(), "high");
    }

    #[test]
    fn next_task_created_at_tiebreak() {
        let (_tmp, conn) = setup();

        // Same priority, created_at order should decide
        // Since tasks are inserted sequentially, the first one has earlier created_at
        make_todo(&conn, "first", Some(Priority::P2));
        make_todo(&conn, "second", Some(Priority::P2));

        let task = next_task(&conn, 1).unwrap().unwrap();
        assert_eq!(task.title(), "first");
    }

    #[test]
    fn next_task_id_tiebreak() {
        let (_tmp, conn) = setup();

        // Insert two tasks with same priority; SQLite created_at has second-level precision
        // so they'll likely have the same created_at, making id the final tiebreaker
        let t1 = make_todo(&conn, "t1", Some(Priority::P2));
        let t2 = make_todo(&conn, "t2", Some(Priority::P2));

        let task = next_task(&conn, 1).unwrap().unwrap();
        // t1 was created first, so it has lower id
        assert!(t1.id() < t2.id());
        assert_eq!(task.id(), t1.id());
    }

    #[test]
    fn next_task_with_completed_dep() {
        let (_tmp, conn) = setup();

        let dep = make_completed(&conn, "dep");

        let task = create_task(
            &conn,
            1,
            &CreateTaskParams {
                title: "ready".to_string(),
                dependencies: vec![dep.id()],
                ..default_create_params("ready")
            },
        ).unwrap();
        transition_to(&conn, task.id(), TaskStatus::Todo);

        let result = next_task(&conn, 1).unwrap().unwrap();
        assert_eq!(result.title(), "ready");
    }

    // --- Dependency tests (via domain methods + save) ---

    #[test]
    fn save_persists_dependencies() {
        let (_tmp, conn) = setup();
        let t1 = create_task(&conn, 1, &default_create_params("t1")).unwrap();
        let t2 = create_task(&conn, 1, &default_create_params("t2")).unwrap();
        let t3 = create_task(&conn, 1, &default_create_params("t3")).unwrap();

        let (t1, _) = t1.add_dependency(t2.id(), Some("2026-01-01T00:00:00Z".into())).unwrap();
        let (t1, _) = t1.add_dependency(t3.id(), Some("2026-01-01T00:00:00Z".into())).unwrap();
        save_task(&conn, &t1).unwrap();

        let loaded = get_task(&conn, t1.id()).unwrap();
        assert_eq!(loaded.dependencies().len(), 2);
        assert!(loaded.dependencies().contains(&t2.id()));
        assert!(loaded.dependencies().contains(&t3.id()));
    }

    #[test]
    fn save_replaces_dependencies() {
        let (_tmp, conn) = setup();
        let t1 = create_task(&conn, 1, &default_create_params("t1")).unwrap();
        let t2 = create_task(&conn, 1, &default_create_params("t2")).unwrap();
        let t3 = create_task(&conn, 1, &default_create_params("t3")).unwrap();

        let (t1, _) = t1.add_dependency(t2.id(), Some("2026-01-01T00:00:00Z".into())).unwrap();
        save_task(&conn, &t1).unwrap();

        let (t1, _) = t1.set_dependencies(&[t3.id()], Some("2026-01-01T00:00:01Z".into())).unwrap();
        save_task(&conn, &t1).unwrap();

        let loaded = get_task(&conn, t1.id()).unwrap();
        assert_eq!(loaded.dependencies(), &[t3.id()]);
    }

    #[test]
    fn save_clears_dependencies() {
        let (_tmp, conn) = setup();
        let t1 = create_task(&conn, 1, &default_create_params("t1")).unwrap();
        let t2 = create_task(&conn, 1, &default_create_params("t2")).unwrap();

        let (t1, _) = t1.add_dependency(t2.id(), Some("2026-01-01T00:00:00Z".into())).unwrap();
        save_task(&conn, &t1).unwrap();

        let (t1, _) = t1.set_dependencies(&[], Some("2026-01-01T00:00:01Z".into())).unwrap();
        save_task(&conn, &t1).unwrap();

        let loaded = get_task(&conn, t1.id()).unwrap();
        assert!(loaded.dependencies().is_empty());
    }

    #[test]
    fn list_dependencies_basic() {
        let (_tmp, conn) = setup();
        let t1 = create_task(&conn, 1, &default_create_params("t1")).unwrap();
        let t2 = create_task(&conn, 1, &default_create_params("t2")).unwrap();
        let t3 = create_task(&conn, 1, &default_create_params("t3")).unwrap();

        let (t1, _) = t1.add_dependency(t2.id(), Some("2026-01-01T00:00:00Z".into())).unwrap();
        let (t1, _) = t1.add_dependency(t3.id(), Some("2026-01-01T00:00:00Z".into())).unwrap();
        save_task(&conn, &t1).unwrap();

        let deps = list_dependencies(&conn, t1.id()).unwrap();
        assert_eq!(deps.len(), 2);
        let dep_ids: Vec<i64> = deps.iter().map(|t| t.id()).collect();
        assert!(dep_ids.contains(&t2.id()));
        assert!(dep_ids.contains(&t3.id()));
    }

    #[test]
    fn list_dependencies_empty() {
        let (_tmp, conn) = setup();
        let t1 = create_task(&conn, 1, &default_create_params("t1")).unwrap();

        let deps = list_dependencies(&conn, t1.id()).unwrap();
        assert!(deps.is_empty());
    }

    #[test]
    fn clear_optional_field_with_none() {
        let (_tmp, conn) = setup();
        let task = create_task(
            &conn,
            1,
            &CreateTaskParams {
                title: "t".to_string(),
                background: Some("bg".to_string()),
                ..default_create_params("t")
            },
        )
        .unwrap();
        assert_eq!(task.background(), Some("bg"));

        let updated = update_task(
            &conn,
            task.id(),
            &UpdateTaskParams {
                title: None,
                background: Some(None), // clear it
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
                metadata: None,
            },
        )
        .unwrap();
        assert!(updated.background().is_none());
    }

    #[test]
    fn check_and_uncheck_dod_via_save() {
        let (_tmp, conn) = setup();
        let task = create_task(
            &conn,
            1,
            &CreateTaskParams {
                definition_of_done: vec!["item1".to_string(), "item2".to_string()],
                ..default_create_params("t")
            },
        )
        .unwrap();

        // Check first item via domain method + save
        let task = get_task(&conn, task.id()).unwrap();
        let (task, _) = task.check_dod(1, "2025-01-01T00:00:00Z".to_string()).unwrap();
        save_task(&conn, &task).unwrap();
        let updated = get_task(&conn, task.id()).unwrap();
        assert!(updated.definition_of_done()[0].checked());
        assert!(!updated.definition_of_done()[1].checked());

        // Check second item
        let (task, _) = updated.check_dod(2, "2025-01-01T00:00:00Z".to_string()).unwrap();
        save_task(&conn, &task).unwrap();
        let updated = get_task(&conn, task.id()).unwrap();
        assert!(updated.definition_of_done()[0].checked());
        assert!(updated.definition_of_done()[1].checked());

        // Uncheck first item
        let (task, _) = updated.uncheck_dod(1, "2025-01-01T00:00:00Z".to_string()).unwrap();
        save_task(&conn, &task).unwrap();
        let updated = get_task(&conn, task.id()).unwrap();
        assert!(!updated.definition_of_done()[0].checked());
        assert!(updated.definition_of_done()[1].checked());
    }

    // --- Migration system tests ---

    #[test]
    fn fresh_db_records_migration_version() {
        let (_tmp, conn) = setup();
        let version = current_schema_version(&conn).unwrap();
        assert_eq!(version, 5);
    }

    #[test]
    fn schema_migrations_has_initial_entry() {
        let (_tmp, conn) = setup();
        let (version, name): (i64, String) = conn
            .query_row(
                "SELECT version, name FROM schema_migrations WHERE version = 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(version, 1);
        assert_eq!(name, "initial_schema");
    }

    #[test]
    fn legacy_db_upgrade_records_version() {
        let tmp = tempfile::tempdir().unwrap();
        let senko_dir = tmp.path().join(".senko");
        std::fs::create_dir_all(&senko_dir).unwrap();
        let db_path = senko_dir.join("data.db");
        let conn = Connection::open(&db_path).unwrap();
        conn.execute_batch("PRAGMA foreign_keys=ON;").unwrap();

        // Create a legacy schema (without checked, branch, metadata, pr_url columns)
        conn.execute_batch(
            "
            CREATE TABLE tasks (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                title TEXT NOT NULL,
                background TEXT,
                details TEXT,
                status TEXT NOT NULL DEFAULT 'draft',
                priority INTEGER NOT NULL DEFAULT 2,
                assignee_session_id TEXT,
                created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
                updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
                started_at TEXT,
                completed_at TEXT,
                canceled_at TEXT,
                cancel_reason TEXT
            );
            CREATE TABLE task_definition_of_done (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                task_id INTEGER NOT NULL,
                content TEXT NOT NULL,
                FOREIGN KEY (task_id) REFERENCES tasks(id) ON DELETE CASCADE
            );
            CREATE TABLE task_in_scope (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                task_id INTEGER NOT NULL,
                content TEXT NOT NULL,
                FOREIGN KEY (task_id) REFERENCES tasks(id) ON DELETE CASCADE
            );
            CREATE TABLE task_out_of_scope (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                task_id INTEGER NOT NULL,
                content TEXT NOT NULL,
                FOREIGN KEY (task_id) REFERENCES tasks(id) ON DELETE CASCADE
            );
            CREATE TABLE task_tags (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                task_id INTEGER NOT NULL,
                tag TEXT NOT NULL,
                UNIQUE(task_id, tag),
                FOREIGN KEY (task_id) REFERENCES tasks(id) ON DELETE CASCADE
            );
            CREATE TABLE task_dependencies (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                task_id INTEGER NOT NULL,
                depends_on_task_id INTEGER NOT NULL,
                UNIQUE(task_id, depends_on_task_id),
                FOREIGN KEY (task_id) REFERENCES tasks(id) ON DELETE CASCADE,
                FOREIGN KEY (depends_on_task_id) REFERENCES tasks(id) ON DELETE CASCADE
            );
            ",
        )
        .unwrap();

        // Insert a task in the legacy schema
        conn.execute(
            "INSERT INTO tasks (title, details) VALUES ('legacy task', 'some details')",
            [],
        )
        .unwrap();

        drop(conn);

        // Open via open_db which runs migrations (using explicit path to the legacy location)
        let conn = open_db(tmp.path(), Some(db_path.as_path()), None).unwrap();

        // Version should include all migrations
        let version = current_schema_version(&conn).unwrap();
        assert_eq!(version, 5);

        // Legacy columns should have been migrated
        let has_description: bool = conn.prepare("SELECT description FROM tasks LIMIT 0").is_ok();
        assert!(has_description);
        let has_branch: bool = conn.prepare("SELECT branch FROM tasks LIMIT 0").is_ok();
        assert!(has_branch);
        let has_checked: bool = conn
            .prepare("SELECT checked FROM task_definition_of_done LIMIT 0")
            .is_ok();
        assert!(has_checked);
        let has_pr_url: bool = conn.prepare("SELECT pr_url FROM tasks LIMIT 0").is_ok();
        assert!(has_pr_url);
        let has_metadata: bool = conn.prepare("SELECT metadata FROM tasks LIMIT 0").is_ok();
        assert!(has_metadata);

        // Legacy data should be preserved (details renamed to description)
        let desc: String = conn
            .query_row(
                "SELECT description FROM tasks WHERE id = 1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(desc, "some details");
    }

    #[test]
    fn migration_idempotent() {
        let tmp = tempfile::tempdir().unwrap();
        let db_path = tmp.path().join("data.db");
        let conn1 = open_db(tmp.path(), Some(db_path.as_path()), None).unwrap();
        let v1 = current_schema_version(&conn1).unwrap();
        drop(conn1);

        let conn2 = open_db(tmp.path(), Some(db_path.as_path()), None).unwrap();
        let v2 = current_schema_version(&conn2).unwrap();
        assert_eq!(v1, v2);

        let count: i64 = conn2
            .query_row("SELECT COUNT(*) FROM schema_migrations", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(count, 5);
    }

    #[test]
    fn current_schema_version_no_table() {
        let tmp = tempfile::tempdir().unwrap();
        let senko_dir = tmp.path().join(".senko");
        std::fs::create_dir_all(&senko_dir).unwrap();
        let db_path = senko_dir.join("data.db");
        let conn = Connection::open(&db_path).unwrap();

        // No schema_migrations table at all
        let version = current_schema_version(&conn).unwrap();
        assert_eq!(version, 0);
    }

    // ---------------------------------------------------------------
    // Integration tests using in-memory SQLite
    // ---------------------------------------------------------------

    fn mem_backend() -> SqliteBackend {
        SqliteBackend::new_in_memory().unwrap()
    }

    fn params(title: &str) -> CreateTaskParams {
        CreateTaskParams {
            title: title.into(),
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
    async fn inmem_task_round_trip() {
        let backend = mem_backend();
        let task = backend
            .create_task(
                1,
                &CreateTaskParams {
                    title: "Round-trip test".into(),
                    background: Some("bg".into()),
                    description: Some("desc".into()),
                    priority: Some(Priority::P1),
                    definition_of_done: vec!["Write tests".into()],
                    in_scope: vec!["API".into()],
                    out_of_scope: vec!["UI".into()],
                    branch: Some("feat/test".into()),
                    pr_url: None,
                    metadata: Some(serde_json::json!({"key": "value"})),
                    tags: vec!["backend".into()],
                    dependencies: vec![],
                },
            )
            .await
            .unwrap();

        let got = backend.get_task(1, task.id()).await.unwrap();
        assert_eq!(got.title(), "Round-trip test");
        assert_eq!(got.background(), Some("bg"));
        assert_eq!(got.description(), Some("desc"));
        assert_eq!(got.priority(), Priority::P1);
        assert_eq!(got.definition_of_done().len(), 1);
        assert_eq!(got.definition_of_done()[0].content(), "Write tests");
        assert!(!got.definition_of_done()[0].checked());
        assert_eq!(got.in_scope(), &["API"]);
        assert_eq!(got.out_of_scope(), &["UI"]);
        assert_eq!(got.branch(), Some("feat/test"));
        assert_eq!(got.tags(), &["backend"]);
        assert_eq!(got.status(), TaskStatus::Draft);
        assert!(got.metadata().is_some());
    }

    #[tokio::test]
    async fn inmem_task_lifecycle() {
        let backend = mem_backend();
        let task = backend
            .create_task(1, &params("Lifecycle"))
            .await
            .unwrap();
        assert_eq!(task.status(), TaskStatus::Draft);

        let (task, _) = task.ready("2026-01-01T00:00:00Z".to_string()).unwrap();
        backend.save(&task).await.unwrap();
        let task_got = backend.get_task(1, task.id()).await.unwrap();
        assert_eq!(task_got.status(), TaskStatus::Todo);

        let (task, _) = task_got.start(Some("sess-1".into()), None, "2026-01-01T00:00:00Z".to_string(), None).unwrap();
        backend.save(&task).await.unwrap();
        let task_got = backend.get_task(1, task.id()).await.unwrap();
        assert_eq!(task_got.status(), TaskStatus::InProgress);
        assert_eq!(task_got.assignee_session_id(), Some("sess-1"));
        assert!(task_got.started_at().is_some());

        let (task, _) = task_got.complete("2026-01-02T00:00:00Z".to_string()).unwrap();
        backend.save(&task).await.unwrap();
        let task_got = backend.get_task(1, task.id()).await.unwrap();
        assert_eq!(task_got.status(), TaskStatus::Completed);
        assert!(task_got.completed_at().is_some());
    }

    #[tokio::test]
    async fn inmem_task_cancel() {
        let backend = mem_backend();
        let task = backend
            .create_task(1, &params("Cancel me"))
            .await
            .unwrap();
        let (task, _) = task.cancel("2026-01-01T00:00:00Z".to_string(), Some("no longer needed".into())).unwrap();
        backend.save(&task).await.unwrap();
        let task_got = backend.get_task(1, task.id()).await.unwrap();
        assert_eq!(task_got.status(), TaskStatus::Canceled);
        assert_eq!(task_got.cancel_reason(), Some("no longer needed"));
    }

    #[tokio::test]
    async fn inmem_project_crud() {
        let backend = mem_backend();
        let proj = backend
            .create_project(&CreateProjectParams {
                name: "test-project".into(),
                description: Some("A test project".into()),
            })
            .await
            .unwrap();
        assert_eq!(proj.name(), "test-project");

        let got = backend.get_project(proj.id()).await.unwrap();
        assert_eq!(got.name(), "test-project");
        assert_eq!(got.description(), Some("A test project"));

        let by_name = backend.get_project_by_name("test-project").await.unwrap();
        assert_eq!(by_name.id(), proj.id());

        let list = backend.list_projects().await.unwrap();
        assert!(list.len() >= 1);

        backend.delete_project(proj.id()).await.unwrap();
        assert!(backend.get_project(proj.id()).await.is_err());
    }

    #[tokio::test]
    async fn inmem_user_crud() {
        let backend = mem_backend();
        let user = backend
            .create_user(&CreateUserParams {
                username: "alice".into(),
                display_name: Some("Alice".into()),
                email: Some("alice@example.com".into()),
            })
            .await
            .unwrap();
        assert_eq!(user.username(), "alice");

        let got = backend.get_user(user.id()).await.unwrap();
        assert_eq!(got.display_name(), Some("Alice"));
        assert_eq!(got.email(), Some("alice@example.com"));

        let by_name = backend.get_user_by_username("alice").await.unwrap();
        assert_eq!(by_name.id(), user.id());

        let list = backend.list_users().await.unwrap();
        assert_eq!(list.len(), 2); // default user + alice

        backend.delete_user(user.id()).await.unwrap();
        assert!(backend.get_user(user.id()).await.is_err());
    }

    #[tokio::test]
    async fn inmem_project_member_management() {
        let backend = mem_backend();
        let user = backend
            .create_user(&CreateUserParams {
                username: "bob".into(),
                display_name: None,
                email: None,
            })
            .await
            .unwrap();

        let member = backend
            .add_project_member(1, &AddProjectMemberParams::new(user.id(), Some(Role::Member)))
            .await
            .unwrap();
        assert_eq!(member.role(), Role::Member);

        let got = backend.get_project_member(1, user.id()).await.unwrap();
        assert_eq!(got.user_id(), user.id());

        let updated = backend.update_member_role(1, user.id(), Role::Owner).await.unwrap();
        assert_eq!(updated.role(), Role::Owner);

        let members = backend.list_project_members(1).await.unwrap();
        assert_eq!(members.len(), 2); // default user (owner) + bob

        backend.remove_project_member(1, user.id()).await.unwrap();
        let members = backend.list_project_members(1).await.unwrap();
        assert_eq!(members.len(), 1); // only default user (owner) remains
    }

    #[tokio::test]
    async fn inmem_dependencies() {
        let backend = mem_backend();
        let t1 = backend
            .create_task(1, &params("T1"))
            .await
            .unwrap();
        let t2 = backend
            .create_task(1, &params("T2"))
            .await
            .unwrap();
        let (t1, _) = t1.ready("2026-01-01T00:00:00Z".to_string()).unwrap();
        backend.save(&t1).await.unwrap();
        let (t2, _) = t2.ready("2026-01-01T00:00:00Z".to_string()).unwrap();
        backend.save(&t2).await.unwrap();

        let (t2, _) = t2.add_dependency(t1.id(), Some("2026-01-01T00:00:01Z".into())).unwrap();
        backend.save(&t2).await.unwrap();
        let t2 = backend.get_task(1, t2.id()).await.unwrap();
        assert_eq!(t2.dependencies(), &[t1.id()]);

        let deps = backend.list_dependencies(1, t2.id()).await.unwrap();
        assert_eq!(deps.len(), 1);
        assert_eq!(deps[0].id(), t1.id());

        let next = backend.next_task(1).await.unwrap();
        assert!(next.is_none() || next.unwrap().id() == t1.id());

        let (t2, _) = t2.remove_dependency(t1.id(), Some("2026-01-01T00:00:02Z".into())).unwrap();
        backend.save(&t2).await.unwrap();
        let t2 = backend.get_task(1, t2.id()).await.unwrap();
        assert!(t2.dependencies().is_empty());
    }

    #[tokio::test]
    async fn inmem_dod_check_uncheck() {
        let backend = mem_backend();
        let mut p = params("DoD test");
        p.definition_of_done = vec!["Item A".into(), "Item B".into()];
        let task = backend
            .create_task(1, &p)
            .await
            .unwrap();
        assert!(!task.definition_of_done()[0].checked());
        assert!(!task.definition_of_done()[1].checked());

        let (task, _) = task.check_dod(1, "2026-01-01T00:00:00Z".to_string()).unwrap();
        backend.save(&task).await.unwrap();
        let task = backend.get_task(1, task.id()).await.unwrap();
        assert!(task.definition_of_done()[0].checked());
        assert!(!task.definition_of_done()[1].checked());

        let (task, _) = task.check_dod(2, "2026-01-01T00:00:00Z".to_string()).unwrap();
        backend.save(&task).await.unwrap();
        let task = backend.get_task(1, task.id()).await.unwrap();
        assert!(task.definition_of_done()[0].checked());
        assert!(task.definition_of_done()[1].checked());

        let (task, _) = task.uncheck_dod(1, "2026-01-01T00:00:00Z".to_string()).unwrap();
        backend.save(&task).await.unwrap();
        let task = backend.get_task(1, task.id()).await.unwrap();
        assert!(!task.definition_of_done()[0].checked());
        assert!(task.definition_of_done()[1].checked());
    }

    #[tokio::test]
    async fn test_sync_config_defaults_project_name() {
        let backend = SqliteBackend::new_in_memory().unwrap();
        let project = backend.get_project(1).await.unwrap();
        assert_eq!(project.name(), "default");

        let mut config = Config::default();
        config.project.name = Some("my-project".to_string());
        backend.sync_config_defaults(&config).unwrap();

        let project = backend.get_project(1).await.unwrap();
        assert_eq!(project.name(), "my-project");
    }

    #[tokio::test]
    async fn test_sync_config_defaults_user_name() {
        let backend = SqliteBackend::new_in_memory().unwrap();
        let user = backend.get_user(1).await.unwrap();
        assert_eq!(user.username(), "default");

        let mut config = Config::default();
        config.user.name = Some("alice".to_string());
        backend.sync_config_defaults(&config).unwrap();

        let user = backend.get_user(1).await.unwrap();
        assert_eq!(user.username(), "alice");
    }

    #[tokio::test]
    async fn test_sync_config_defaults_none_keeps_default() {
        let backend = SqliteBackend::new_in_memory().unwrap();
        let config = Config::default();
        backend.sync_config_defaults(&config).unwrap();

        let project = backend.get_project(1).await.unwrap();
        assert_eq!(project.name(), "default");
        let user = backend.get_user(1).await.unwrap();
        assert_eq!(user.username(), "default");
    }

    #[tokio::test]
    async fn test_sync_config_defaults_unique_conflict() {
        let backend = SqliteBackend::new_in_memory().unwrap();
        // Create a second project with name "taken"
        use crate::domain::project::CreateProjectParams;
        backend.create_project(&CreateProjectParams {
            name: "taken".to_string(),
            description: None,
        }).await.unwrap();

        let mut config = Config::default();
        config.project.name = Some("taken".to_string());
        let result = backend.sync_config_defaults(&config);
        assert!(result.is_err());
    }

    // --- SQL / domain equivalence tests ---

    #[test]
    fn sql_next_task_matches_domain_select_next() {
        let (_tmp, conn) = setup();

        make_todo(&conn, "low", Some(Priority::P3));
        make_todo(&conn, "high", Some(Priority::P0));
        make_todo(&conn, "mid", Some(Priority::P1));

        let sql_result = next_task(&conn, 1).unwrap().unwrap();

        let all_tasks = list_tasks(&conn, 1, &ListTasksFilter::default()).unwrap();
        let domain_result =
            crate::domain::task::select_next(all_tasks, &HashMap::new()).unwrap();

        assert_eq!(sql_result.id(), domain_result.id());
    }

    #[test]
    fn sql_next_task_matches_domain_with_deps() {
        let (_tmp, conn) = setup();

        let dep = create_task(&conn, 1, &default_create_params("dep")).unwrap();
        // dep stays draft (not completed) => blocks dependents

        let blocked = create_task(
            &conn,
            1,
            &CreateTaskParams {
                title: "blocked".to_string(),
                dependencies: vec![dep.id()],
                ..default_create_params("blocked")
            },
        )
        .unwrap();
        transition_to(&conn, blocked.id(), TaskStatus::Todo);

        let free = make_todo(&conn, "free", Some(Priority::P1));

        let sql_result = next_task(&conn, 1).unwrap().unwrap();

        let all_tasks = list_tasks(&conn, 1, &ListTasksFilter::default()).unwrap();
        let dep_statuses: HashMap<i64, TaskStatus> = all_tasks
            .iter()
            .map(|t| (t.id(), t.status()))
            .collect();
        let todo_tasks: Vec<Task> = all_tasks
            .into_iter()
            .filter(|t| t.status() == TaskStatus::Todo)
            .collect();
        let domain_result =
            crate::domain::task::select_next(todo_tasks, &dep_statuses).unwrap();

        assert_eq!(sql_result.id(), domain_result.id());
        assert_eq!(sql_result.id(), free.id());
    }

    #[test]
    fn sql_ready_filter_matches_domain_filter_ready() {
        let (_tmp, conn) = setup();

        let dep = create_task(&conn, 1, &default_create_params("dep")).unwrap();

        let blocked = create_task(
            &conn,
            1,
            &CreateTaskParams {
                title: "blocked".to_string(),
                dependencies: vec![dep.id()],
                ..default_create_params("blocked")
            },
        )
        .unwrap();
        transition_to(&conn, blocked.id(), TaskStatus::Todo);

        make_todo(&conn, "free1", None);
        make_todo(&conn, "free2", None);

        let sql_ready = list_ready_tasks(&conn, 1).unwrap();

        let all_tasks = list_tasks(&conn, 1, &ListTasksFilter::default()).unwrap();
        let dep_statuses: HashMap<i64, TaskStatus> = all_tasks
            .iter()
            .map(|t| (t.id(), t.status()))
            .collect();
        let todo_tasks: Vec<Task> = all_tasks
            .into_iter()
            .filter(|t| t.status() == TaskStatus::Todo)
            .collect();
        let domain_ready =
            crate::domain::task::filter_ready(todo_tasks, &dep_statuses);

        let mut sql_ids: Vec<i64> = sql_ready.iter().map(|t| t.id()).collect();
        let mut domain_ids: Vec<i64> = domain_ready.iter().map(|t| t.id()).collect();
        sql_ids.sort();
        domain_ids.sort();
        assert_eq!(sql_ids, domain_ids);
    }

    #[test]
    fn sql_ready_count_matches_domain() {
        let (_tmp, conn) = setup();

        let dep = create_task(&conn, 1, &default_create_params("dep")).unwrap();

        let blocked = create_task(
            &conn,
            1,
            &CreateTaskParams {
                title: "blocked".to_string(),
                dependencies: vec![dep.id()],
                ..default_create_params("blocked")
            },
        )
        .unwrap();
        transition_to(&conn, blocked.id(), TaskStatus::Todo);

        make_todo(&conn, "free1", None);
        make_todo(&conn, "free2", None);

        let sql_count = ready_count(&conn, 1).unwrap();

        let all_tasks = list_tasks(&conn, 1, &ListTasksFilter::default()).unwrap();
        let dep_statuses: HashMap<i64, TaskStatus> = all_tasks
            .iter()
            .map(|t| (t.id(), t.status()))
            .collect();
        let todo_tasks: Vec<Task> = all_tasks
            .into_iter()
            .filter(|t| t.status() == TaskStatus::Todo)
            .collect();
        let domain_count =
            crate::domain::task::filter_ready(todo_tasks, &dep_statuses).len() as i64;

        assert_eq!(sql_count, domain_count);
    }
}
