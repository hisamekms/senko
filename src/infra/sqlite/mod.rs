use std::collections::HashMap;
use std::path::Path;

use crate::domain::DEFAULT_USER_ID;
use crate::domain::contract::{
    Contract, ContractId, ContractNote, ContractRepository, CreateContractParams,
    ListContractNotesFilter, ListContractsFilter, UpdateContractArrayParams, UpdateContractParams,
};
use crate::domain::error::DomainError;
use crate::domain::metadata_field::{
    CreateMetadataFieldParams, ListMetadataFieldsFilter, MetadataField, MetadataFieldType,
    UpdateMetadataFieldParams,
};
use crate::domain::pagination::{Cursor, ListPage, build_page};
use crate::domain::project::{
    CreateProjectParams, DEFAULT_PROJECT_ID, ListProjectMembersFilter, ListProjectsFilter, Project,
    ProjectId, UpdateProjectParams,
};
use crate::domain::task::{
    self, CreateTaskParams, DodItem, ListTaskDepsFilter, ListTasksFilter, ListTasksPage,
    MetadataUpdate, Priority, Task, TaskId, TaskStatus, UpdateTaskArrayParams, UpdateTaskParams,
    shallow_merge_metadata,
};
use crate::domain::user::{
    AddProjectMemberParams, ApiKey, ApiKeyWithSecret, CreateUserParams, ListSessionsFilter,
    ListUsersFilter, NewApiKey, ProjectMember, Role, UpdateUserParams, User, UserId, Username,
};
use crate::infra::TaskDbId;
use crate::infra::xdg::XdgDirs;
use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension, params};

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
    Migration {
        version: 6,
        name: "add_task_number",
        sql: "
            ALTER TABLE tasks ADD COLUMN task_number INTEGER;
            UPDATE tasks SET task_number = id;
            CREATE UNIQUE INDEX idx_tasks_project_task_number ON tasks(project_id, task_number);
        ",
    },
    Migration {
        version: 7,
        name: "add_api_key_device_name",
        sql: "
            ALTER TABLE api_keys ADD COLUMN device_name TEXT;
        ",
    },
    Migration {
        version: 8,
        name: "add_metadata_fields",
        sql: "
            CREATE TABLE metadata_fields (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                project_id INTEGER NOT NULL,
                name TEXT NOT NULL,
                field_type TEXT NOT NULL,
                required_on_complete INTEGER NOT NULL DEFAULT 0,
                description TEXT,
                created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
                UNIQUE(project_id, name),
                FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE
            );

            CREATE INDEX idx_metadata_fields_project_id ON metadata_fields(project_id);
        ",
    },
    Migration {
        version: 9,
        name: "add_user_sub",
        sql: "
            ALTER TABLE users ADD COLUMN sub TEXT;
            UPDATE users SET sub = username WHERE sub IS NULL;
            CREATE UNIQUE INDEX idx_users_sub ON users(sub);
        ",
    },
    Migration {
        version: 10,
        name: "add_contracts",
        sql: "
            CREATE TABLE contracts (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                project_id INTEGER NOT NULL,
                title TEXT NOT NULL,
                description TEXT,
                metadata TEXT,
                created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
                updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
                FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE
            );

            CREATE INDEX idx_contracts_project_id ON contracts(project_id);

            CREATE TABLE contract_definition_of_done (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                contract_id INTEGER NOT NULL,
                content TEXT NOT NULL,
                checked INTEGER NOT NULL DEFAULT 0,
                FOREIGN KEY (contract_id) REFERENCES contracts(id) ON DELETE CASCADE
            );

            CREATE TABLE contract_tags (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                contract_id INTEGER NOT NULL,
                tag TEXT NOT NULL,
                UNIQUE(contract_id, tag),
                FOREIGN KEY (contract_id) REFERENCES contracts(id) ON DELETE CASCADE
            );

            CREATE TABLE contract_notes (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                contract_id INTEGER NOT NULL,
                content TEXT NOT NULL,
                source_task_id INTEGER,
                created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
                FOREIGN KEY (contract_id) REFERENCES contracts(id) ON DELETE CASCADE,
                FOREIGN KEY (source_task_id) REFERENCES tasks(id) ON DELETE SET NULL
            );

            ALTER TABLE tasks ADD COLUMN contract_id INTEGER REFERENCES contracts(id) ON DELETE SET NULL;
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
        .query_row("SELECT MAX(version) FROM schema_migrations", [], |row| {
            row.get(0)
        })
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

    let current_version: i64 = conn.query_row(
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
        .query_row("SELECT MAX(version) FROM schema_migrations", [], |row| {
            row.get(0)
        })
        .optional()?
        .flatten();
    Ok(version.unwrap_or(0))
}

/// Resolve the XDG data directory base.
/// Returns `$XDG_DATA_HOME` or `~/.local/share`.
fn xdg_data_base(xdg: &XdgDirs) -> Option<std::path::PathBuf> {
    xdg.data_home.clone()
}

/// Compute a per-project XDG database path using the project directory name.
/// Returns `$XDG_DATA_HOME/senko/projects/<dir-name>/data.db`.
fn xdg_project_db_path(xdg: &XdgDirs, project_root: &Path) -> Option<std::path::PathBuf> {
    let data_dir = xdg_data_base(xdg)?;
    let dir_name = project_root.file_name()?.to_string_lossy();
    Some(
        data_dir
            .join("senko")
            .join("projects")
            .join(dir_name.as_ref())
            .join("data.db"),
    )
}

/// Legacy hash-based per-project XDG database path (for migration).
/// Returns `$XDG_DATA_HOME/senko/projects/<sha256-16chars>/data.db`.
fn xdg_project_db_path_legacy_hash(
    xdg: &XdgDirs,
    project_root: &Path,
) -> Option<std::path::PathBuf> {
    use sha2::{Digest, Sha256};
    let data_dir = xdg_data_base(xdg)?;
    let canonical = project_root
        .canonicalize()
        .ok()
        .unwrap_or_else(|| project_root.to_path_buf());
    let hash: String = Sha256::digest(canonical.to_string_lossy().as_bytes())
        .iter()
        .map(|b| format!("{:02x}", b))
        .collect();
    let short_hash = &hash[..16];
    Some(
        data_dir
            .join("senko")
            .join("projects")
            .join(short_hash)
            .join("data.db"),
    )
}

/// Old global XDG path (pre-per-project migration).
/// Returns `$XDG_DATA_HOME/senko/data.db`.
fn xdg_global_db_path(xdg: &XdgDirs) -> Option<std::path::PathBuf> {
    let data_dir = xdg_data_base(xdg)?;
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
    xdg: &XdgDirs,
) -> Option<std::path::PathBuf> {
    if let Some(p) = config_db_path {
        return Some(std::path::PathBuf::from(p));
    }
    xdg_project_db_path(xdg, project_root)
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
    xdg: &XdgDirs,
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
    let xdg_path = xdg_project_db_path(xdg, project_root)
        .ok_or_else(|| anyhow::anyhow!("cannot determine XDG_DATA_HOME or HOME directory"))?;

    if xdg_path.exists() {
        return Ok(xdg_path);
    }

    // 4. Migrate from hash-based XDG path → dir-name-based XDG path
    if let Some(hash_path) = xdg_project_db_path_legacy_hash(xdg, project_root)
        && hash_path.exists()
    {
        copy_db_files(&hash_path, &xdg_path)?;
        eprintln!(
            "warning: migrated database from {} to {}. \
                 The hash-based path has been kept. You can remove it after verifying the migration.",
            hash_path.display(),
            xdg_path.display()
        );
        return Ok(xdg_path);
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
    if let Some(global_path) = xdg_global_db_path(xdg)
        && global_path.exists()
    {
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

    // 7. New installation: per-project XDG default
    Ok(xdg_path)
}

fn open_db(
    project_root: &Path,
    explicit_db_path: Option<&Path>,
    config_db_path: Option<&str>,
    xdg: &XdgDirs,
) -> Result<Connection> {
    let db_path = resolve_db_path(project_root, explicit_db_path, config_db_path, xdg)?;

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
    let has_branch: bool = conn.prepare("SELECT branch FROM tasks LIMIT 0").is_ok();
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
    let has_metadata: bool = conn.prepare("SELECT metadata FROM tasks LIMIT 0").is_ok();
    if !has_metadata {
        conn.execute_batch("ALTER TABLE tasks ADD COLUMN metadata TEXT")?;
    }

    // Add pr_url column if it doesn't exist
    let has_pr_url: bool = conn.prepare("SELECT pr_url FROM tasks LIMIT 0").is_ok();
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
    let id = ProjectId(conn.last_insert_rowid());
    get_project(conn, id)
}

fn get_project(conn: &Connection, id: ProjectId) -> Result<Project> {
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
    let (id, description, created_at): (ProjectId, Option<String>, String) = conn
        .query_row(
            "SELECT id, description, created_at FROM projects WHERE name = ?1",
            params![name],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()?
        .ok_or(DomainError::ProjectNotFound)?;
    Ok(Project::new(id, name.to_string(), description, created_at))
}

fn list_projects(conn: &Connection, filter: &ListProjectsFilter) -> Result<ListPage<Project>> {
    let mut sql = String::from("SELECT id, name, description, created_at FROM projects WHERE 1=1");
    let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
    if let Some(after) = filter.after {
        sql.push_str(" AND id > ?");
        let after_i64: i64 = after.into();
        param_values.push(Box::new(after_i64));
    }
    sql.push_str(" ORDER BY id");
    if let Some(l) = filter.limit {
        sql.push_str(" LIMIT ?");
        param_values.push(Box::new(l as i64 + 1));
    }
    let param_refs: Vec<&dyn rusqlite::types::ToSql> =
        param_values.iter().map(|v| v.as_ref()).collect();
    let mut stmt = conn.prepare(&sql)?;
    let projects: Vec<Project> = stmt
        .query_map(param_refs.as_slice(), |row| {
            Ok(Project::new(
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
            ))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(build_page(projects, filter.limit, |p| {
        Cursor::encode(p.id())
    }))
}

fn delete_project(conn: &Connection, id: ProjectId) -> Result<()> {
    let affected = conn.execute("DELETE FROM projects WHERE id = ?1", params![id])?;
    if affected == 0 {
        return Err(DomainError::ProjectNotFound.into());
    }
    Ok(())
}

fn update_project(
    conn: &Connection,
    id: ProjectId,
    update: &UpdateProjectParams,
) -> Result<Project> {
    // Verify exists; bubbles up `ProjectNotFound` if missing.
    let _existing = get_project(conn, id)?;

    if let Some(ref description) = update.description {
        conn.execute(
            "UPDATE projects SET description = ?1 WHERE id = ?2",
            params![description, id],
        )?;
    }
    get_project(conn, id)
}

// --- User CRUD ---

fn create_user(conn: &Connection, params: &CreateUserParams) -> Result<User> {
    let effective_sub = params.sub.as_deref().unwrap_or(params.username.as_ref());
    conn.execute(
        "INSERT INTO users (username, sub, display_name, email) VALUES (?1, ?2, ?3, ?4)",
        rusqlite::params![
            params.username,
            effective_sub,
            params.display_name,
            params.email
        ],
    )?;
    let id = UserId(conn.last_insert_rowid());
    get_user(conn, id)
}

fn get_user(conn: &Connection, id: UserId) -> Result<User> {
    let (username, sub, display_name, email, created_at): (
        Username,
        String,
        Option<String>,
        Option<String>,
        String,
    ) = conn
        .query_row(
            "SELECT username, sub, display_name, email, created_at FROM users WHERE id = ?1",
            rusqlite::params![id],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            },
        )
        .optional()?
        .ok_or(DomainError::UserNotFound)?;
    Ok(User::new(
        id,
        username,
        sub,
        display_name,
        email,
        created_at,
    ))
}

fn get_user_by_username(conn: &Connection, username: &Username) -> Result<User> {
    let (id, sub, display_name, email, created_at): (
        UserId,
        String,
        Option<String>,
        Option<String>,
        String,
    ) = conn
        .query_row(
            "SELECT id, sub, display_name, email, created_at FROM users WHERE username = ?1",
            rusqlite::params![username],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            },
        )
        .optional()?
        .ok_or(DomainError::UserNotFound)?;
    Ok(User::new(
        id,
        username.clone(),
        sub,
        display_name,
        email,
        created_at,
    ))
}

fn get_user_by_sub(conn: &Connection, sub: &str) -> Result<User> {
    let (id, username, display_name, email, created_at): (
        UserId,
        Username,
        Option<String>,
        Option<String>,
        String,
    ) = conn
        .query_row(
            "SELECT id, username, display_name, email, created_at FROM users WHERE sub = ?1",
            rusqlite::params![sub],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            },
        )
        .optional()?
        .ok_or(DomainError::UserNotFound)?;
    Ok(User::new(
        id,
        username,
        sub.to_string(),
        display_name,
        email,
        created_at,
    ))
}

fn list_users(conn: &Connection, filter: &ListUsersFilter) -> Result<ListPage<User>> {
    let mut sql = String::from(
        "SELECT id, username, sub, display_name, email, created_at FROM users WHERE 1=1",
    );
    let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
    if let Some(after) = filter.after {
        sql.push_str(" AND id > ?");
        param_values.push(Box::new(after));
    }
    sql.push_str(" ORDER BY id");
    if let Some(l) = filter.limit {
        sql.push_str(" LIMIT ?");
        param_values.push(Box::new(l as i64 + 1));
    }
    let param_refs: Vec<&dyn rusqlite::types::ToSql> =
        param_values.iter().map(|v| v.as_ref()).collect();
    let mut stmt = conn.prepare(&sql)?;
    let users: Vec<User> = stmt
        .query_map(param_refs.as_slice(), |row| {
            Ok(User::new(
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                row.get(5)?,
            ))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(build_page(users, filter.limit, |u| Cursor::encode(u.id())))
}

fn update_user(conn: &Connection, id: UserId, params: &UpdateUserParams) -> Result<User> {
    // Verify user exists first
    get_user(conn, id)?;

    if let Some(ref username) = params.username {
        conn.execute(
            "UPDATE users SET username = ?1 WHERE id = ?2",
            rusqlite::params![username, id],
        )?;
    }
    if let Some(ref display_name) = params.display_name {
        conn.execute(
            "UPDATE users SET display_name = ?1 WHERE id = ?2",
            rusqlite::params![display_name, id],
        )?;
    }

    get_user(conn, id)
}

fn delete_user(conn: &Connection, id: UserId) -> Result<()> {
    let affected = conn.execute("DELETE FROM users WHERE id = ?1", rusqlite::params![id])?;
    if affected == 0 {
        return Err(DomainError::UserNotFound.into());
    }
    Ok(())
}

// --- API Key CRUD ---

fn create_api_key(
    conn: &Connection,
    user_id: UserId,
    name: &str,
    device_name: Option<&str>,
    new_key: &NewApiKey,
) -> Result<ApiKeyWithSecret> {
    get_user(conn, user_id)?;

    conn.execute(
        "INSERT INTO api_keys (user_id, key_hash, key_prefix, name, device_name) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![user_id, new_key.key_hash, new_key.key_prefix, name, device_name],
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
        device_name.map(String::from),
        created_at,
    ))
}

fn get_user_by_api_key(
    conn: &Connection,
    key_hash: &str,
) -> Result<crate::application::port::ApiKeyAuthResult> {
    conn.execute(
        "UPDATE api_keys SET last_used_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now') WHERE key_hash = ?1",
        params![key_hash],
    )?;

    let (user_id, key_created_at, key_last_used_at): (UserId, String, Option<String>) = conn
        .query_row(
            "SELECT user_id, created_at, last_used_at FROM api_keys WHERE key_hash = ?1",
            params![key_hash],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()?
        .ok_or(DomainError::ApiKeyNotFound)?;

    let user = get_user(conn, user_id)?;
    Ok(crate::application::port::ApiKeyAuthResult {
        user,
        key_created_at,
        key_last_used_at,
    })
}

fn list_api_keys(conn: &Connection, user_id: UserId) -> Result<Vec<ApiKey>> {
    let mut stmt = conn.prepare(
        "SELECT id, user_id, key_prefix, name, device_name, created_at, last_used_at FROM api_keys WHERE user_id = ?1 ORDER BY id",
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
                row.get(6)?,
            ))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(keys)
}

fn list_api_keys_page(
    conn: &Connection,
    user_id: UserId,
    filter: &ListSessionsFilter,
) -> Result<ListPage<ApiKey>> {
    let mut sql = String::from(
        "SELECT id, user_id, key_prefix, name, device_name, created_at, last_used_at FROM api_keys WHERE user_id = ?1",
    );
    let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
    param_values.push(Box::new(user_id));
    if let Some(after) = filter.after {
        sql.push_str(" AND id > ?");
        param_values.push(Box::new(after));
    }
    sql.push_str(" ORDER BY id");
    if let Some(l) = filter.limit {
        sql.push_str(" LIMIT ?");
        param_values.push(Box::new(l as i64 + 1));
    }
    let param_refs: Vec<&dyn rusqlite::types::ToSql> =
        param_values.iter().map(|v| v.as_ref()).collect();
    let mut stmt = conn.prepare(&sql)?;
    let keys: Vec<ApiKey> = stmt
        .query_map(param_refs.as_slice(), |row| {
            Ok(ApiKey::new(
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                row.get(5)?,
                row.get(6)?,
            ))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(build_page(keys, filter.limit, |k| Cursor::encode(k.id())))
}

fn delete_api_key(conn: &Connection, key_id: i64) -> Result<()> {
    let affected = conn.execute("DELETE FROM api_keys WHERE id = ?1", params![key_id])?;
    if affected == 0 {
        return Err(DomainError::ApiKeyNotFound.into());
    }
    Ok(())
}

fn delete_api_key_for_user(conn: &Connection, key_id: i64, user_id: UserId) -> Result<()> {
    let affected = conn.execute(
        "DELETE FROM api_keys WHERE id = ?1 AND user_id = ?2",
        params![key_id, user_id],
    )?;
    if affected == 0 {
        return Err(DomainError::ApiKeyNotFound.into());
    }
    Ok(())
}

fn delete_all_api_keys_for_user(conn: &Connection, user_id: UserId) -> Result<()> {
    conn.execute("DELETE FROM api_keys WHERE user_id = ?1", params![user_id])?;
    Ok(())
}

// --- Project Member CRUD ---

fn add_project_member(
    conn: &Connection,
    project_id: ProjectId,
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
    Ok(ProjectMember::new(
        id,
        project_id,
        params.user_id,
        params.role,
        created_at,
    ))
}

fn remove_project_member(conn: &Connection, project_id: ProjectId, user_id: UserId) -> Result<()> {
    let affected = conn.execute(
        "DELETE FROM project_members WHERE project_id = ?1 AND user_id = ?2",
        rusqlite::params![project_id, user_id],
    )?;
    if affected == 0 {
        return Err(DomainError::ProjectMemberNotFound.into());
    }
    Ok(())
}

fn list_project_members(
    conn: &Connection,
    project_id: ProjectId,
    filter: &ListProjectMembersFilter,
) -> Result<ListPage<ProjectMember>> {
    let mut sql = String::from(
        "SELECT id, user_id, role, created_at FROM project_members WHERE project_id = ?1",
    );
    let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
    param_values.push(Box::new(project_id));
    if let Some(after) = filter.after {
        sql.push_str(" AND id > ?");
        param_values.push(Box::new(after));
    }
    sql.push_str(" ORDER BY id");
    if let Some(l) = filter.limit {
        sql.push_str(" LIMIT ?");
        param_values.push(Box::new(l as i64 + 1));
    }
    let param_refs: Vec<&dyn rusqlite::types::ToSql> =
        param_values.iter().map(|v| v.as_ref()).collect();
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt
        .query_map(param_refs.as_slice(), |row| {
            let role_str: String = row.get(2)?;
            Ok((row.get(0)?, row.get(1)?, role_str, row.get(3)?))
        })?
        .collect::<std::result::Result<Vec<(i64, UserId, String, String)>, _>>()?;

    let members = rows
        .into_iter()
        .map(|(id, user_id, role_str, created_at)| {
            let role: Role = role_str
                .parse()
                .map_err(|e| anyhow::anyhow!("invalid role in database: {e}"))?;
            Ok(ProjectMember::new(
                id, project_id, user_id, role, created_at,
            ))
        })
        .collect::<Result<Vec<ProjectMember>>>()?;
    Ok(build_page(members, filter.limit, |m| {
        Cursor::encode(m.id())
    }))
}

fn get_project_member(
    conn: &Connection,
    project_id: ProjectId,
    user_id: UserId,
) -> Result<ProjectMember> {
    let (id, role_str, created_at): (i64, String, String) = conn
        .query_row(
            "SELECT id, role, created_at FROM project_members WHERE project_id = ?1 AND user_id = ?2",
            rusqlite::params![project_id, user_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()?
        .ok_or(DomainError::ProjectMemberNotFound)?;
    let role: Role = role_str.parse()?;
    Ok(ProjectMember::new(
        id, project_id, user_id, role, created_at,
    ))
}

fn update_member_role(
    conn: &Connection,
    project_id: ProjectId,
    user_id: UserId,
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
/// Resolve a user-facing task_number to internal id, verifying project ownership.
fn resolve_task_number(
    conn: &Connection,
    project_id: ProjectId,
    task_id: TaskId,
) -> Result<TaskDbId> {
    let task_number: i64 = task_id.into();
    conn.query_row(
        "SELECT id FROM tasks WHERE project_id = ?1 AND task_number = ?2",
        params![project_id, task_number],
        |row| row.get::<_, i64>(0).map(TaskDbId),
    )
    .optional()?
    .ok_or_else(|| DomainError::TaskNotFound.into())
}

// --- Task functions ---

fn create_task(
    conn: &Connection,
    project_id: ProjectId,
    params: &CreateTaskParams,
) -> Result<Task> {
    // Verify project exists
    get_project(conn, project_id)?;
    let priority: i32 = params.priority.unwrap_or(Priority::P2).into();
    let metadata_str = params
        .metadata
        .as_ref()
        .map(serde_json::to_string)
        .transpose()?;

    // Assign next task_number for this project
    let task_number: i64 = conn.query_row(
        "SELECT COALESCE(MAX(task_number), 0) + 1 FROM tasks WHERE project_id = ?1",
        params![project_id],
        |row| row.get(0),
    )?;

    conn.execute(
        "INSERT INTO tasks (title, background, description, priority, branch, pr_url, metadata, project_id, task_number, assignee_user_id, contract_id) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        rusqlite::params![params.title, params.background, params.description, priority, params.branch, params.pr_url, metadata_str, project_id, task_number, params.assignee_user_id.as_ref().and_then(|a| a.as_id()), params.contract_id],
    )?;
    let task_id = TaskDbId(conn.last_insert_rowid());

    if let Some(ref branch) = params.branch
        && branch.contains("${task_id}")
    {
        let expanded = task::expand_branch_template(branch, TaskId(task_number));
        conn.execute(
            "UPDATE tasks SET branch = ?1 WHERE id = ?2",
            params![expanded, task_id],
        )?;
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
    for &dep_task_number in &params.dependencies {
        let dep_internal_id = resolve_task_number(conn, project_id, dep_task_number)?;
        conn.execute(
            "INSERT INTO task_dependencies (task_id, depends_on_task_id) VALUES (?1, ?2)",
            params![task_id, dep_internal_id],
        )?;
    }

    get_task(conn, task_id)
}

type TaskRow = (
    ProjectId,
    i64,
    String,
    Option<String>,
    Option<String>,
    Option<String>,
    String,
    i32,
    Option<String>,
    String,
    String,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<UserId>,
    Option<ContractId>,
);

fn get_task(conn: &Connection, id: TaskDbId) -> Result<Task> {
    let (project_id, task_number, title, background, description, plan, status_str, priority_val, assignee_session_id, created_at, updated_at, started_at, completed_at, canceled_at, cancel_reason, branch, pr_url, metadata_str, assignee_user_id, contract_id): TaskRow = conn
        .query_row(
            "SELECT project_id, task_number, title, background, description, plan, status, priority, assignee_session_id, created_at, updated_at, started_at, completed_at, canceled_at, cancel_reason, branch, pr_url, metadata, assignee_user_id, contract_id FROM tasks WHERE id = ?1",
            params![id],
            |row| {
                Ok((
                    row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?,
                    row.get(4)?, row.get(5)?, row.get(6)?, row.get(7)?,
                    row.get(8)?, row.get(9)?, row.get(10)?, row.get(11)?,
                    row.get(12)?, row.get(13)?, row.get(14)?, row.get(15)?,
                    row.get(16)?, row.get(17)?, row.get(18)?, row.get(19)?,
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
    // Fetch dependency task_numbers (not internal IDs)
    let dependencies: Vec<TaskId> = query_i64_list(
        conn,
        "SELECT t.task_number FROM task_dependencies td JOIN tasks t ON t.id = td.depends_on_task_id WHERE td.task_id = ?1",
        id,
    )?
    .into_iter()
    .map(TaskId::from)
    .collect();

    Ok(Task::new(
        TaskId(task_number),
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
        contract_id,
        metadata,
        definition_of_done,
        in_scope,
        out_of_scope,
        tags,
        dependencies,
    ))
}

fn update_task(conn: &Connection, id: TaskDbId, params: &UpdateTaskParams) -> Result<Task> {
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
        values.push(Box::new(assignee_user_id.as_ref().and_then(|a| a.as_id())));
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
    if let Some(ref contract_id) = params.contract_id {
        columns.push(TaskColumn::ContractId);
        values.push(Box::new(*contract_id));
    }
    if let Some(ref meta_update) = params.metadata {
        columns.push(TaskColumn::Metadata);
        let resolved: Option<serde_json::Value> = match meta_update {
            MetadataUpdate::Clear => None,
            MetadataUpdate::Replace(v) => Some(v.clone()),
            MetadataUpdate::Merge(patch) => {
                let existing_str: Option<String> = conn.query_row(
                    "SELECT metadata FROM tasks WHERE id = ?",
                    params![id],
                    |row| row.get(0),
                )?;
                let existing: Option<serde_json::Value> = existing_str
                    .as_deref()
                    .map(serde_json::from_str)
                    .transpose()
                    .map_err(|e| anyhow::anyhow!("failed to parse existing metadata: {e}"))?;
                shallow_merge_metadata(existing.as_ref(), patch)
            }
        };
        let metadata_str: Option<String> = resolved
            .as_ref()
            .map(serde_json::to_string)
            .transpose()
            .map_err(|e| anyhow::anyhow!("failed to serialize metadata: {e}"))?;
        values.push(Box::new(metadata_str));
    }

    if !columns.is_empty() {
        let set_clause: Vec<String> = columns
            .iter()
            .map(|c| format!("{} = ?", c.as_str()))
            .collect();
        let sql = format!(
            "UPDATE tasks SET {}, updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now') WHERE id = ?",
            set_clause.join(", ")
        );
        values.push(Box::new(id));
        let param_refs: Vec<&dyn rusqlite::types::ToSql> =
            values.iter().map(|v| v.as_ref()).collect();
        conn.execute(&sql, param_refs.as_slice())?;
    }

    get_task(conn, id)
}

fn update_task_arrays(
    conn: &Connection,
    id: TaskDbId,
    params: &UpdateTaskArrayParams,
) -> Result<()> {
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
    update_content_array(
        conn,
        id,
        ContentTable::DefinitionOfDone,
        &params.set_definition_of_done,
        &params.add_definition_of_done,
        &params.remove_definition_of_done,
    )?;
    // in_scope
    update_content_array(
        conn,
        id,
        ContentTable::InScope,
        &params.set_in_scope,
        &params.add_in_scope,
        &params.remove_in_scope,
    )?;
    // out_of_scope
    update_content_array(
        conn,
        id,
        ContentTable::OutOfScope,
        &params.set_out_of_scope,
        &params.add_out_of_scope,
        &params.remove_out_of_scope,
    )?;

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
    let internal_id = resolve_task_number(conn, task.project_id(), task.id())?;
    let metadata_str: Option<String> = task
        .metadata()
        .map(serde_json::to_string)
        .transpose()
        .map_err(|e| anyhow::anyhow!("failed to serialize metadata: {e}"))?;

    conn.execute(
        "UPDATE tasks SET
            title = ?2, background = ?3, description = ?4, plan = ?5,
            priority = ?6, status = ?7,
            assignee_session_id = ?8, assignee_user_id = ?9,
            started_at = ?10, completed_at = ?11, canceled_at = ?12, cancel_reason = ?13,
            branch = ?14, pr_url = ?15, metadata = ?16, contract_id = ?17,
            updated_at = ?18
        WHERE id = ?1",
        params![
            internal_id,
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
            task.contract_id(),
            task.updated_at(),
        ],
    )?;

    // Sync definition_of_done
    conn.execute(
        "DELETE FROM task_definition_of_done WHERE task_id = ?1",
        params![internal_id],
    )?;
    for dod in task.definition_of_done() {
        let checked_val: i32 = if dod.checked() { 1 } else { 0 };
        conn.execute(
            "INSERT INTO task_definition_of_done (task_id, content, checked) VALUES (?1, ?2, ?3)",
            params![internal_id, dod.content(), checked_val],
        )?;
    }

    // Sync dependencies (task.dependencies() contains task_numbers, resolve to internal IDs)
    conn.execute(
        "DELETE FROM task_dependencies WHERE task_id = ?1",
        params![internal_id],
    )?;
    for &dep_task_number in task.dependencies() {
        let dep_internal_id = resolve_task_number(conn, task.project_id(), dep_task_number)?;
        conn.execute(
            "INSERT INTO task_dependencies (task_id, depends_on_task_id) VALUES (?1, ?2)",
            params![internal_id, dep_internal_id],
        )?;
    }

    Ok(())
}

enum TaskColumn {
    Title,
    Background,
    Description,
    ContractId,
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
            TaskColumn::ContractId => "contract_id",
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
    task_id: TaskDbId,
    table: ContentTable,
    set: &Option<Vec<String>>,
    add: &[String],
    remove: &[String],
) -> Result<()> {
    let table = table.as_str();
    if let Some(values) = set {
        conn.execute(
            &format!("DELETE FROM {table} WHERE task_id = ?1"),
            params![task_id],
        )?;
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

fn delete_task(conn: &Connection, id: TaskDbId) -> Result<()> {
    let affected = conn.execute("DELETE FROM tasks WHERE id = ?1", params![id])?;
    if affected == 0 {
        return Err(DomainError::TaskNotFound.into());
    }
    Ok(())
}

fn list_tasks(
    conn: &Connection,
    project_id: ProjectId,
    filter: &ListTasksFilter,
) -> Result<ListTasksPage> {
    let mut conditions = Vec::new();
    let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

    conditions.push("t.project_id = ?".to_string());
    param_values.push(Box::new(project_id));

    if let Some(after) = filter.after {
        conditions.push("t.id > ?".to_string());
        let after_i64: i64 = after.into();
        param_values.push(Box::new(after_i64));
    }

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
        let dep_id_i64: i64 = dep_id.into();
        param_values.push(Box::new(dep_id_i64));
    }

    if let Some(contract_id) = filter.contract_id {
        conditions.push("t.contract_id = ?".to_string());
        param_values.push(Box::new(contract_id));
    }

    if let Some(id_min) = filter.id_min {
        conditions.push("t.id >= ?".to_string());
        let id_min_i64: i64 = id_min.into();
        param_values.push(Box::new(id_min_i64));
    }

    if let Some(id_max) = filter.id_max {
        conditions.push("t.id <= ?".to_string());
        let id_max_i64: i64 = id_max.into();
        param_values.push(Box::new(id_max_i64));
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

    if let Some(uid) = filter.assignee_user_id {
        if filter.include_unassigned {
            conditions.push("(t.assignee_user_id = ? OR t.assignee_user_id IS NULL)".to_string());
        } else {
            conditions.push("t.assignee_user_id = ?".to_string());
        }
        param_values.push(Box::new(uid));
    }

    for (key, value) in &filter.metadata {
        let json_path = format!("$.{key}");
        match value {
            serde_json::Value::Number(n) => {
                conditions.push("json_extract(t.metadata, ?) = ?".to_string());
                param_values.push(Box::new(json_path));
                if let Some(i) = n.as_i64() {
                    param_values.push(Box::new(i));
                } else if let Some(f) = n.as_f64() {
                    param_values.push(Box::new(f));
                }
            }
            serde_json::Value::Bool(b) => {
                // SQLite json_extract returns 1/0 for JSON booleans
                conditions.push("json_extract(t.metadata, ?) = ?".to_string());
                param_values.push(Box::new(json_path));
                param_values.push(Box::new(if *b { 1i64 } else { 0i64 }));
            }
            serde_json::Value::String(s) => {
                conditions.push("json_extract(t.metadata, ?) = ?".to_string());
                param_values.push(Box::new(json_path));
                param_values.push(Box::new(s.clone()));
            }
            other => {
                conditions.push("json_extract(t.metadata, ?) = json(?)".to_string());
                param_values.push(Box::new(json_path));
                param_values.push(Box::new(serde_json::to_string(other).unwrap_or_default()));
            }
        }
    }

    let where_clause = if conditions.is_empty() {
        String::new()
    } else {
        format!(" WHERE {}", conditions.join(" AND "))
    };

    let mut sql = format!("SELECT t.id FROM tasks t{} ORDER BY t.id", where_clause);
    // peek-ahead: fetch limit+1 rows to detect whether more exist
    if let Some(l) = filter.limit {
        sql.push_str(" LIMIT ?");
        param_values.push(Box::new(l as i64 + 1));
    }
    let param_refs: Vec<&dyn rusqlite::types::ToSql> =
        param_values.iter().map(|v| v.as_ref()).collect();

    let mut stmt = conn.prepare(&sql)?;
    let ids: Vec<TaskDbId> = stmt
        .query_map(param_refs.as_slice(), |row| row.get::<_, TaskDbId>(0))?
        .collect::<std::result::Result<Vec<_>, _>>()?;

    let mut items = Vec::with_capacity(ids.len());
    for id in ids {
        items.push(get_task(conn, id)?);
    }

    Ok(crate::domain::pagination::build_page(
        items,
        filter.limit,
        |t| Cursor::encode(t.id()),
    ))
}

/// SQL-optimized implementation of [`crate::domain::task::select_next`].
/// Equivalence with domain logic is verified by integration tests.
fn next_task(
    conn: &Connection,
    project_id: ProjectId,
    user_id: Option<UserId>,
    include_unassigned: bool,
) -> Result<Option<Task>> {
    let assignee_clause = match user_id {
        Some(_) if include_unassigned => {
            " AND (t.assignee_user_id = ?2 OR t.assignee_user_id IS NULL)"
        }
        Some(_) => " AND t.assignee_user_id = ?2",
        None => "",
    };
    let sql = format!(
        "SELECT t.id FROM tasks t
         WHERE t.project_id = ?1
           AND t.status = 'todo'
           AND NOT EXISTS (
             SELECT 1 FROM task_dependencies td
             JOIN tasks dep ON dep.id = td.depends_on_task_id
             WHERE td.task_id = t.id AND dep.status != 'completed'
           ){assignee_clause}
         ORDER BY t.priority ASC, t.created_at ASC, t.id ASC
         LIMIT 1"
    );
    let id: Option<TaskDbId> = if let Some(uid) = user_id {
        conn.query_row(&sql, params![project_id, uid], |row| {
            row.get::<_, TaskDbId>(0)
        })
        .optional()?
    } else {
        conn.query_row(&sql, params![project_id], |row| row.get::<_, TaskDbId>(0))
            .optional()?
    };
    match id {
        Some(id) => Ok(Some(get_task(conn, id)?)),
        None => Ok(None),
    }
}

fn task_stats(conn: &Connection, project_id: ProjectId) -> Result<HashMap<String, i64>> {
    let mut stmt =
        conn.prepare("SELECT status, COUNT(*) FROM tasks WHERE project_id = ?1 GROUP BY status")?;
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
fn ready_count(conn: &Connection, project_id: ProjectId) -> Result<i64> {
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

fn list_ready_tasks(conn: &Connection, project_id: ProjectId) -> Result<Vec<Task>> {
    let filter = ListTasksFilter {
        ready: true,
        ..Default::default()
    };
    Ok(list_tasks(conn, project_id, &filter)?.items)
}

/// SQL-optimized equivalent of `Task::is_ready`: true iff the task is in the
/// given project, has status `todo`, and every dependency is `completed`.
/// Missing tasks or tasks from other projects return `false`.
///
/// The second argument is the public `task_number`, not the internal DB id —
/// matching the identity exposed via HTTP / CLI / hook payloads.
fn is_task_ready(conn: &Connection, project_id: ProjectId, task_number: TaskId) -> Result<bool> {
    let task_number_i64: i64 = task_number.into();
    let sql = "
        SELECT 1 FROM tasks t
        WHERE t.project_id = ?1
          AND t.task_number = ?2
          AND t.status = 'todo'
          AND NOT EXISTS (
            SELECT 1 FROM task_dependencies td
            JOIN tasks dep ON dep.id = td.depends_on_task_id
            WHERE td.task_id = t.id AND dep.status != 'completed'
          )
        LIMIT 1
    ";
    let found: Option<i64> = conn
        .query_row(sql, params![project_id, task_number_i64], |row| row.get(0))
        .optional()?;
    Ok(found.is_some())
}

fn list_dependencies(
    conn: &Connection,
    task_id: TaskDbId,
    filter: &ListTaskDepsFilter,
) -> Result<ListPage<Task>> {
    get_task(conn, task_id)?;

    let mut sql =
        String::from("SELECT depends_on_task_id FROM task_dependencies WHERE task_id = ?1");
    let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
    param_values.push(Box::new(task_id));
    if let Some(after) = filter.after {
        sql.push_str(" AND depends_on_task_id > ?");
        let after_i64: i64 = after.into();
        param_values.push(Box::new(after_i64));
    }
    sql.push_str(" ORDER BY depends_on_task_id");
    if let Some(l) = filter.limit {
        sql.push_str(" LIMIT ?");
        param_values.push(Box::new(l as i64 + 1));
    }
    let param_refs: Vec<&dyn rusqlite::types::ToSql> =
        param_values.iter().map(|v| v.as_ref()).collect();
    let mut stmt = conn.prepare(&sql)?;
    let dep_ids: Vec<TaskDbId> = stmt
        .query_map(param_refs.as_slice(), |row| row.get::<_, TaskDbId>(0))?
        .collect::<std::result::Result<Vec<_>, _>>()?;

    let mut tasks = Vec::with_capacity(dep_ids.len());
    for id in dep_ids {
        tasks.push(get_task(conn, id)?);
    }
    Ok(build_page(tasks, filter.limit, |t| Cursor::encode(t.id())))
}

fn query_dod_list(conn: &Connection, task_id: TaskDbId) -> Result<Vec<DodItem>> {
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

fn query_string_list(conn: &Connection, sql: &str, task_id: TaskDbId) -> Result<Vec<String>> {
    let mut stmt = conn.prepare(sql)?;
    let items: Vec<String> = stmt
        .query_map(params![task_id], |row| row.get(0))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(items)
}

fn query_i64_list(conn: &Connection, sql: &str, task_id: TaskDbId) -> Result<Vec<i64>> {
    let mut stmt = conn.prepare(sql)?;
    let items: Vec<i64> = stmt
        .query_map(params![task_id], |row| row.get(0))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(items)
}

// --- Default record sync ---

fn update_project_name(conn: &Connection, id: ProjectId, name: &str) -> Result<()> {
    conn.execute(
        "UPDATE projects SET name = ?1 WHERE id = ?2",
        params![name, id],
    )?;
    Ok(())
}

fn update_user_username(conn: &Connection, id: UserId, username: &Username) -> Result<()> {
    conn.execute(
        "UPDATE users SET username = ?1 WHERE id = ?2",
        params![username, id],
    )?;
    Ok(())
}

// --- MetadataField CRUD ---

fn create_metadata_field(
    conn: &Connection,
    project_id: ProjectId,
    params: &CreateMetadataFieldParams,
) -> Result<MetadataField> {
    let result = conn.execute(
        "INSERT INTO metadata_fields (project_id, name, field_type, required_on_complete, description)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        rusqlite::params![
            project_id,
            params.name,
            params.field_type.to_string(),
            params.required_on_complete as i32,
            params.description,
        ],
    );
    match result {
        Ok(_) => {}
        Err(rusqlite::Error::SqliteFailure(err, _))
            if err.code == rusqlite::ErrorCode::ConstraintViolation =>
        {
            return Err(DomainError::MetadataFieldNameConflict {
                name: params.name.clone(),
            }
            .into());
        }
        Err(e) => return Err(e.into()),
    }
    let id = conn.last_insert_rowid();
    get_metadata_field(conn, project_id, id)
}

fn get_metadata_field(
    conn: &Connection,
    project_id: ProjectId,
    field_id: i64,
) -> Result<MetadataField> {
    let row: (i64, ProjectId, String, String, i32, Option<String>, String) = conn
        .query_row(
            "SELECT id, project_id, name, field_type, required_on_complete, description, created_at
             FROM metadata_fields WHERE id = ?1 AND project_id = ?2",
            params![field_id, project_id],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                ))
            },
        )
        .optional()?
        .ok_or(DomainError::MetadataFieldNotFound)?;
    let field_type: MetadataFieldType = row.3.parse()?;
    Ok(MetadataField::new(
        row.0,
        row.1,
        row.2,
        field_type,
        row.4 != 0,
        row.5,
        row.6,
    ))
}

fn list_metadata_fields(
    conn: &Connection,
    project_id: ProjectId,
    filter: &ListMetadataFieldsFilter,
) -> Result<ListPage<MetadataField>> {
    let mut sql = String::from(
        "SELECT id, project_id, name, field_type, required_on_complete, description, created_at
         FROM metadata_fields WHERE project_id = ?1",
    );
    let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
    param_values.push(Box::new(project_id));
    if let Some(after) = filter.after {
        sql.push_str(" AND id > ?");
        param_values.push(Box::new(after));
    }
    sql.push_str(" ORDER BY id");
    if let Some(l) = filter.limit {
        sql.push_str(" LIMIT ?");
        param_values.push(Box::new(l as i64 + 1));
    }
    let param_refs: Vec<&dyn rusqlite::types::ToSql> =
        param_values.iter().map(|v| v.as_ref()).collect();
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt
        .query_map(param_refs.as_slice(), |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, ProjectId>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, i32>(4)?,
                row.get::<_, Option<String>>(5)?,
                row.get::<_, String>(6)?,
            ))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let items: Vec<MetadataField> = rows
        .into_iter()
        .map(|r| {
            let field_type: MetadataFieldType = r.3.parse()?;
            Ok(MetadataField::new(
                r.0,
                r.1,
                r.2,
                field_type,
                r.4 != 0,
                r.5,
                r.6,
            ))
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(build_page(items, filter.limit, |f| Cursor::encode(f.id())))
}

fn update_metadata_field(
    conn: &Connection,
    project_id: ProjectId,
    field_id: i64,
    params: &UpdateMetadataFieldParams,
) -> Result<MetadataField> {
    // Verify it exists
    let _existing = get_metadata_field(conn, project_id, field_id)?;

    let mut sets = Vec::new();
    let mut values: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

    if let Some(req) = params.required_on_complete {
        sets.push("required_on_complete = ?");
        values.push(Box::new(req as i32));
    }
    if let Some(ref desc) = params.description {
        sets.push("description = ?");
        values.push(Box::new(desc.clone()));
    }

    if sets.is_empty() {
        return get_metadata_field(conn, project_id, field_id);
    }

    let set_clause = sets.join(", ");
    let sql = format!(
        "UPDATE metadata_fields SET {} WHERE id = ? AND project_id = ?",
        set_clause
    );
    values.push(Box::new(field_id));
    values.push(Box::new(project_id));

    let param_refs: Vec<&dyn rusqlite::ToSql> = values.iter().map(|v| v.as_ref()).collect();
    conn.execute(&sql, param_refs.as_slice())?;
    get_metadata_field(conn, project_id, field_id)
}

fn delete_metadata_field(conn: &Connection, project_id: ProjectId, field_id: i64) -> Result<()> {
    let affected = conn.execute(
        "DELETE FROM metadata_fields WHERE id = ?1 AND project_id = ?2",
        params![field_id, project_id],
    )?;
    if affected == 0 {
        return Err(DomainError::MetadataFieldNotFound.into());
    }
    Ok(())
}

// --- Contract CRUD (sync helpers) ---

fn get_contract(conn: &Connection, id: ContractId) -> Result<Contract> {
    let (project_id, title, description, metadata_str, created_at, updated_at): (
        ProjectId,
        String,
        Option<String>,
        Option<String>,
        String,
        String,
    ) = conn
        .query_row(
            "SELECT project_id, title, description, metadata, created_at, updated_at \
             FROM contracts WHERE id = ?1",
            params![id],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                ))
            },
        )
        .optional()?
        .ok_or(DomainError::ContractNotFound)?;

    let metadata: Option<serde_json::Value> = metadata_str
        .map(|s| serde_json::from_str(&s))
        .transpose()
        .context("invalid contract metadata JSON in database")?;

    let definition_of_done = {
        let mut stmt = conn.prepare(
            "SELECT content, checked FROM contract_definition_of_done WHERE contract_id = ?1 ORDER BY id",
        )?;
        stmt.query_map(params![id], |row| {
            Ok(DodItem::new(row.get(0)?, row.get::<_, i32>(1)? != 0))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?
    };

    let tags: Vec<String> = {
        let mut stmt =
            conn.prepare("SELECT tag FROM contract_tags WHERE contract_id = ?1 ORDER BY id")?;
        stmt.query_map(params![id], |row| row.get(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?
    };

    let notes: Vec<ContractNote> = {
        let mut stmt = conn.prepare(
            "SELECT content, source_task_id, created_at FROM contract_notes WHERE contract_id = ?1 ORDER BY id",
        )?;
        stmt.query_map(params![id], |row| {
            Ok(ContractNote::new(
                row.get::<_, String>(0)?,
                row.get::<_, Option<i64>>(1)?.map(TaskId),
                row.get::<_, String>(2)?,
            ))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?
    };

    Ok(Contract::new(
        id,
        project_id,
        title,
        description,
        definition_of_done,
        tags,
        metadata,
        notes,
        created_at,
        updated_at,
    ))
}

fn create_contract(
    conn: &Connection,
    project_id: ProjectId,
    params: &CreateContractParams,
) -> Result<Contract> {
    get_project(conn, project_id)?;

    let metadata_str = params
        .metadata
        .as_ref()
        .map(serde_json::to_string)
        .transpose()?;

    conn.execute(
        "INSERT INTO contracts (project_id, title, description, metadata) VALUES (?1, ?2, ?3, ?4)",
        rusqlite::params![project_id, params.title, params.description, metadata_str],
    )?;
    let contract_id = ContractId(conn.last_insert_rowid());

    for content in &params.definition_of_done {
        conn.execute(
            "INSERT INTO contract_definition_of_done (contract_id, content) VALUES (?1, ?2)",
            params![contract_id, content],
        )?;
    }
    for tag in &params.tags {
        conn.execute(
            "INSERT INTO contract_tags (contract_id, tag) VALUES (?1, ?2)",
            params![contract_id, tag],
        )?;
    }

    get_contract(conn, contract_id)
}

fn list_contracts(
    conn: &Connection,
    project_id: ProjectId,
    filter: &ListContractsFilter,
) -> Result<ListPage<Contract>> {
    let mut sql = String::from("SELECT c.id FROM contracts c WHERE c.project_id = ?1");
    let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
    param_values.push(Box::new(project_id));
    if let Some(after) = filter.after {
        sql.push_str(" AND c.id > ?");
        param_values.push(Box::new(after));
    }
    // AND-semantic tag filter: every requested tag must be present on the contract.
    for tag in &filter.tags {
        sql.push_str(
            " AND EXISTS (SELECT 1 FROM contract_tags ct WHERE ct.contract_id = c.id AND ct.tag = ?)",
        );
        param_values.push(Box::new(tag.clone()));
    }
    sql.push_str(" ORDER BY c.id");
    if let Some(l) = filter.limit {
        sql.push_str(" LIMIT ?");
        param_values.push(Box::new(l as i64 + 1));
    }
    let param_refs: Vec<&dyn rusqlite::types::ToSql> =
        param_values.iter().map(|v| v.as_ref()).collect();
    let mut stmt = conn.prepare(&sql)?;
    let ids: Vec<ContractId> = stmt
        .query_map(param_refs.as_slice(), |row| row.get(0))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let mut items = Vec::with_capacity(ids.len());
    for id in ids {
        items.push(get_contract(conn, id)?);
    }
    Ok(build_page(items, filter.limit, |c| Cursor::encode(c.id())))
}

fn list_contract_notes(
    conn: &Connection,
    contract_id: ContractId,
    filter: &ListContractNotesFilter,
) -> Result<ListPage<ContractNote>> {
    let mut sql = String::from(
        "SELECT id, content, source_task_id, created_at FROM contract_notes WHERE contract_id = ?1",
    );
    let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
    param_values.push(Box::new(contract_id));
    if let Some(after) = filter.after {
        sql.push_str(" AND id > ?");
        param_values.push(Box::new(after));
    }
    sql.push_str(" ORDER BY id");
    if let Some(l) = filter.limit {
        sql.push_str(" LIMIT ?");
        param_values.push(Box::new(l as i64 + 1));
    }
    let param_refs: Vec<&dyn rusqlite::types::ToSql> =
        param_values.iter().map(|v| v.as_ref()).collect();
    let mut stmt = conn.prepare(&sql)?;
    // We read id only for cursor computation; ContractNote doesn't expose it.
    let rows: Vec<(i64, ContractNote)> = stmt
        .query_map(param_refs.as_slice(), |row| {
            let id: i64 = row.get(0)?;
            let content: String = row.get(1)?;
            let source: Option<i64> = row.get(2)?;
            let created_at: String = row.get(3)?;
            Ok((
                id,
                ContractNote::new(content, source.map(TaskId::from), created_at),
            ))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    // Build a parallel Vec of the id so we can encode the trailing cursor after
    // build_page trims the peeked row.
    let ids: Vec<i64> = rows.iter().map(|(id, _)| *id).collect();
    let notes: Vec<ContractNote> = rows.into_iter().map(|(_, n)| n).collect();
    let page = match filter.limit {
        Some(l) if notes.len() as u32 > l => {
            let mut notes = notes;
            notes.truncate(l as usize);
            let cursor_id = ids[l as usize - 1];
            ListPage {
                items: notes,
                next_cursor: Some(Cursor::encode(cursor_id)),
            }
        }
        _ => ListPage {
            items: notes,
            next_cursor: None,
        },
    };
    Ok(page)
}

fn update_contract(
    conn: &Connection,
    id: ContractId,
    update: &UpdateContractParams,
    array_update: &UpdateContractArrayParams,
) -> Result<Contract> {
    // Verify exists
    let _existing = get_contract(conn, id)?;

    let mut touched = false;

    if let Some(ref title) = update.title {
        conn.execute(
            "UPDATE contracts SET title = ?1 WHERE id = ?2",
            params![title, id],
        )?;
        touched = true;
    }
    if let Some(ref description) = update.description {
        conn.execute(
            "UPDATE contracts SET description = ?1 WHERE id = ?2",
            params![description, id],
        )?;
        touched = true;
    }
    if let Some(ref meta_update) = update.metadata {
        let resolved: Option<serde_json::Value> = match meta_update {
            MetadataUpdate::Clear => None,
            MetadataUpdate::Replace(v) => Some(v.clone()),
            MetadataUpdate::Merge(patch) => {
                let existing_str: Option<String> = conn.query_row(
                    "SELECT metadata FROM contracts WHERE id = ?1",
                    params![id],
                    |row| row.get(0),
                )?;
                let existing: Option<serde_json::Value> = existing_str
                    .as_deref()
                    .map(serde_json::from_str)
                    .transpose()?;
                shallow_merge_metadata(existing.as_ref(), patch)
            }
        };
        let metadata_str: Option<String> =
            resolved.as_ref().map(serde_json::to_string).transpose()?;
        conn.execute(
            "UPDATE contracts SET metadata = ?1 WHERE id = ?2",
            params![metadata_str, id],
        )?;
        touched = true;
    }

    // Tags
    if let Some(ref set) = array_update.set_tags {
        conn.execute(
            "DELETE FROM contract_tags WHERE contract_id = ?1",
            params![id],
        )?;
        for tag in set {
            conn.execute(
                "INSERT INTO contract_tags (contract_id, tag) VALUES (?1, ?2)",
                params![id, tag],
            )?;
        }
        touched = true;
    }
    for tag in &array_update.add_tags {
        conn.execute(
            "INSERT OR IGNORE INTO contract_tags (contract_id, tag) VALUES (?1, ?2)",
            params![id, tag],
        )?;
        touched = true;
    }
    for tag in &array_update.remove_tags {
        conn.execute(
            "DELETE FROM contract_tags WHERE contract_id = ?1 AND tag = ?2",
            params![id, tag],
        )?;
        touched = true;
    }

    // Definition of done (reset checked on `set`)
    if let Some(ref set) = array_update.set_definition_of_done {
        conn.execute(
            "DELETE FROM contract_definition_of_done WHERE contract_id = ?1",
            params![id],
        )?;
        for content in set {
            conn.execute(
                "INSERT INTO contract_definition_of_done (contract_id, content) VALUES (?1, ?2)",
                params![id, content],
            )?;
        }
        touched = true;
    }
    for content in &array_update.add_definition_of_done {
        conn.execute(
            "INSERT INTO contract_definition_of_done (contract_id, content) VALUES (?1, ?2)",
            params![id, content],
        )?;
        touched = true;
    }
    for content in &array_update.remove_definition_of_done {
        conn.execute(
            "DELETE FROM contract_definition_of_done WHERE contract_id = ?1 AND content = ?2",
            params![id, content],
        )?;
        touched = true;
    }

    if touched {
        conn.execute(
            "UPDATE contracts SET updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now') WHERE id = ?1",
            params![id],
        )?;
    }

    get_contract(conn, id)
}

fn delete_contract(conn: &Connection, id: ContractId) -> Result<()> {
    let affected = conn.execute("DELETE FROM contracts WHERE id = ?1", params![id])?;
    if affected == 0 {
        return Err(DomainError::ContractNotFound.into());
    }
    Ok(())
}

fn add_contract_note(
    conn: &Connection,
    contract_id: ContractId,
    note: &ContractNote,
) -> Result<ContractNote> {
    // Verify exists
    let _existing = get_contract(conn, contract_id)?;

    conn.execute(
        "INSERT INTO contract_notes (contract_id, content, source_task_id, created_at) \
         VALUES (?1, ?2, ?3, ?4)",
        params![
            contract_id,
            note.content(),
            note.source_task_id().map(i64::from),
            note.created_at(),
        ],
    )?;
    conn.execute(
        "UPDATE contracts SET updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now') WHERE id = ?1",
        params![contract_id],
    )?;

    Ok(ContractNote::new(
        note.content().to_string(),
        note.source_task_id(),
        note.created_at().to_string(),
    ))
}

fn set_contract_dod_checked(
    conn: &Connection,
    contract_id: ContractId,
    index: usize,
    checked: bool,
) -> Result<Contract> {
    // 1-based; verify exists
    let contract = get_contract(conn, contract_id)?;
    let dod_len = contract.definition_of_done().len();
    if index == 0 || index > dod_len {
        return Err(DomainError::DodIndexOutOfRange {
            index,
            task_id: contract_id.into(),
            count: dod_len,
        }
        .into());
    }

    let dod_row_id: i64 = conn.query_row(
        "SELECT id FROM contract_definition_of_done WHERE contract_id = ?1 ORDER BY id LIMIT 1 OFFSET ?2",
        params![contract_id, (index - 1) as i64],
        |row| row.get(0),
    )?;

    conn.execute(
        "UPDATE contract_definition_of_done SET checked = ?1 WHERE id = ?2",
        params![if checked { 1i32 } else { 0i32 }, dod_row_id],
    )?;
    conn.execute(
        "UPDATE contracts SET updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now') WHERE id = ?1",
        params![contract_id],
    )?;

    get_contract(conn, contract_id)
}

// --- SqliteBackend implementation ---

use std::sync::Arc;

use async_trait::async_trait;

use crate::application::port::{
    AuthenticationPort, ProjectQueryPort, TaskQueryPort, UserQueryPort,
};
use crate::domain::{
    ApiKeyRepository, MetadataFieldRepository, ProjectMemberRepository, ProjectRepository,
    TaskRepository, UserRepository,
};
use crate::infra::config::Config;

pub struct SqliteBackend {
    conn: Arc<std::sync::Mutex<Connection>>,
}

impl SqliteBackend {
    pub fn new(
        project_root: &Path,
        explicit_db_path: Option<&Path>,
        config_db_path: Option<&str>,
        xdg: &XdgDirs,
    ) -> Result<Self> {
        let conn = open_db(project_root, explicit_db_path, config_db_path, xdg)?;
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
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("mutex lock failed: {e}"))?;
        if let Some(ref name) = config.project.name {
            update_project_name(&conn, DEFAULT_PROJECT_ID, name)
                .with_context(|| format!(
                    "failed to sync project name '{name}' to default project (id=1): name may already be used by another project"
                ))?;
        }
        if let Some(ref name) = config.user.name {
            let username = Username::try_from(name.clone())
                .with_context(|| format!("invalid user name '{name}' in config"))?;
            update_user_username(&conn, DEFAULT_USER_ID, &username)
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
            let conn = conn
                .lock()
                .map_err(|e| anyhow::anyhow!("mutex lock failed: {e}"))?;
            $body(&conn)
        })
        .await?
    }};
}

#[async_trait]
impl ProjectRepository for SqliteBackend {
    async fn create_project(&self, params: &CreateProjectParams) -> Result<Project> {
        let params = params.clone();
        blocking!(self, |conn: &Connection| create_project(conn, &params))
    }

    async fn get_project(&self, id: ProjectId) -> Result<Project> {
        blocking!(self, |conn: &Connection| get_project(conn, id))
    }

    async fn get_project_by_name(&self, name: &str) -> Result<Project> {
        let name = name.to_owned();
        blocking!(self, |conn: &Connection| get_project_by_name(conn, &name))
    }

    async fn update_project(&self, id: ProjectId, params: &UpdateProjectParams) -> Result<Project> {
        let params = params.clone();
        blocking!(self, |conn: &Connection| update_project(conn, id, &params))
    }

    async fn delete_project(&self, id: ProjectId) -> Result<()> {
        blocking!(self, |conn: &Connection| delete_project(conn, id))
    }
}

#[async_trait]
impl ProjectMemberRepository for SqliteBackend {
    async fn add_project_member(
        &self,
        project_id: ProjectId,
        params: &AddProjectMemberParams,
    ) -> Result<ProjectMember> {
        let params = params.clone();
        blocking!(self, |conn: &Connection| add_project_member(
            conn, project_id, &params
        ))
    }

    async fn remove_project_member(&self, project_id: ProjectId, user_id: UserId) -> Result<()> {
        blocking!(self, |conn: &Connection| remove_project_member(
            conn, project_id, user_id
        ))
    }

    async fn list_project_members(
        &self,
        project_id: ProjectId,
        filter: &ListProjectMembersFilter,
    ) -> Result<ListPage<ProjectMember>> {
        let filter = filter.clone();
        blocking!(self, |conn: &Connection| list_project_members(
            conn, project_id, &filter
        ))
    }

    async fn get_project_member(
        &self,
        project_id: ProjectId,
        user_id: UserId,
    ) -> Result<ProjectMember> {
        blocking!(self, |conn: &Connection| get_project_member(
            conn, project_id, user_id
        ))
    }

    async fn update_member_role(
        &self,
        project_id: ProjectId,
        user_id: UserId,
        role: Role,
    ) -> Result<ProjectMember> {
        blocking!(self, |conn: &Connection| update_member_role(
            conn, project_id, user_id, role
        ))
    }
}

#[async_trait]
impl UserRepository for SqliteBackend {
    async fn create_user(&self, params: &CreateUserParams) -> Result<User> {
        let params = params.clone();
        blocking!(self, |conn: &Connection| create_user(conn, &params))
    }

    async fn get_user(&self, id: UserId) -> Result<User> {
        blocking!(self, |conn: &Connection| get_user(conn, id))
    }

    async fn get_user_by_username(&self, username: &Username) -> Result<User> {
        let username = username.clone();
        blocking!(self, |conn: &Connection| get_user_by_username(
            conn, &username
        ))
    }

    async fn get_user_by_sub(&self, sub: &str) -> Result<User> {
        let sub = sub.to_owned();
        blocking!(self, |conn: &Connection| get_user_by_sub(conn, &sub))
    }

    async fn update_user(&self, id: UserId, params: &UpdateUserParams) -> Result<User> {
        let params = params.clone();
        blocking!(self, |conn: &Connection| update_user(conn, id, &params))
    }

    async fn delete_user(&self, id: UserId) -> Result<()> {
        blocking!(self, |conn: &Connection| delete_user(conn, id))
    }
}

#[async_trait]
impl AuthenticationPort for SqliteBackend {
    async fn get_user_by_api_key(
        &self,
        key_hash: &str,
    ) -> Result<crate::application::port::ApiKeyAuthResult> {
        let key_hash = key_hash.to_owned();
        blocking!(self, |conn: &Connection| get_user_by_api_key(
            conn, &key_hash
        ))
    }
}

#[async_trait]
impl ApiKeyRepository for SqliteBackend {
    async fn create_api_key(
        &self,
        user_id: UserId,
        name: &str,
        device_name: Option<&str>,
        new_key: &NewApiKey,
    ) -> Result<ApiKeyWithSecret> {
        let name = name.to_owned();
        let device_name = device_name.map(String::from);
        let new_key = new_key.clone();
        blocking!(self, |conn: &Connection| create_api_key(
            conn,
            user_id,
            &name,
            device_name.as_deref(),
            &new_key
        ))
    }

    async fn delete_api_key(&self, key_id: i64) -> Result<()> {
        blocking!(self, |conn: &Connection| delete_api_key(conn, key_id))
    }

    async fn delete_api_key_for_user(&self, key_id: i64, user_id: UserId) -> Result<()> {
        blocking!(self, |conn: &Connection| delete_api_key_for_user(
            conn, key_id, user_id
        ))
    }

    async fn delete_all_api_keys_for_user(&self, user_id: UserId) -> Result<()> {
        blocking!(self, |conn: &Connection| delete_all_api_keys_for_user(
            conn, user_id
        ))
    }
}

#[async_trait]
impl ProjectQueryPort for SqliteBackend {
    async fn list_projects(&self, filter: &ListProjectsFilter) -> Result<ListPage<Project>> {
        let filter = filter.clone();
        blocking!(self, |conn: &Connection| list_projects(conn, &filter))
    }
}

#[async_trait]
impl UserQueryPort for SqliteBackend {
    async fn list_users(&self, filter: &ListUsersFilter) -> Result<ListPage<User>> {
        let filter = filter.clone();
        blocking!(self, |conn: &Connection| list_users(conn, &filter))
    }

    async fn list_api_keys(&self, user_id: UserId) -> Result<Vec<ApiKey>> {
        blocking!(self, |conn: &Connection| list_api_keys(conn, user_id))
    }

    async fn list_api_keys_page(
        &self,
        user_id: UserId,
        filter: &ListSessionsFilter,
    ) -> Result<ListPage<ApiKey>> {
        let filter = filter.clone();
        blocking!(self, |conn: &Connection| list_api_keys_page(
            conn, user_id, &filter
        ))
    }
}

#[async_trait]
impl TaskRepository for SqliteBackend {
    async fn create_task(&self, project_id: ProjectId, params: &CreateTaskParams) -> Result<Task> {
        let params = params.clone();
        blocking!(self, |conn: &Connection| create_task(
            conn, project_id, &params
        ))
    }

    async fn get_task(&self, project_id: ProjectId, id: TaskId) -> Result<Task> {
        blocking!(self, |conn: &Connection| {
            let internal_id = resolve_task_number(conn, project_id, id)?;
            get_task(conn, internal_id)
        })
    }

    async fn update_task(
        &self,
        project_id: ProjectId,
        id: TaskId,
        params: &UpdateTaskParams,
    ) -> Result<Task> {
        let params = params.clone();
        blocking!(self, |conn: &Connection| {
            let internal_id = resolve_task_number(conn, project_id, id)?;
            update_task(conn, internal_id, &params)
        })
    }

    async fn update_task_arrays(
        &self,
        project_id: ProjectId,
        id: TaskId,
        params: &UpdateTaskArrayParams,
    ) -> Result<()> {
        let params = params.clone();
        blocking!(self, |conn: &Connection| {
            let internal_id = resolve_task_number(conn, project_id, id)?;
            update_task_arrays(conn, internal_id, &params)
        })
    }

    async fn delete_task(&self, project_id: ProjectId, id: TaskId) -> Result<()> {
        blocking!(self, |conn: &Connection| {
            let internal_id = resolve_task_number(conn, project_id, id)?;
            delete_task(conn, internal_id)
        })
    }

    async fn list_dependencies(
        &self,
        project_id: ProjectId,
        task_id: TaskId,
        filter: &ListTaskDepsFilter,
    ) -> Result<ListPage<Task>> {
        let filter = filter.clone();
        blocking!(self, |conn: &Connection| {
            let internal_id = resolve_task_number(conn, project_id, task_id)?;
            list_dependencies(conn, internal_id, &filter)
        })
    }

    async fn save(&self, task: &Task) -> Result<()> {
        let task = task.clone();
        blocking!(self, |conn: &Connection| save_task(conn, &task))
    }
}

#[async_trait]
impl TaskQueryPort for SqliteBackend {
    async fn list_tasks(
        &self,
        project_id: ProjectId,
        filter: &ListTasksFilter,
    ) -> Result<ListTasksPage> {
        let filter = filter.clone();
        blocking!(self, |conn: &Connection| list_tasks(
            conn, project_id, &filter
        ))
    }

    async fn next_task(
        &self,
        project_id: ProjectId,
        user_id: Option<UserId>,
        include_unassigned: bool,
    ) -> Result<Option<Task>> {
        blocking!(self, |conn: &Connection| next_task(
            conn,
            project_id,
            user_id,
            include_unassigned
        ))
    }

    async fn task_stats(&self, project_id: ProjectId) -> Result<HashMap<String, i64>> {
        blocking!(self, |conn: &Connection| task_stats(conn, project_id))
    }

    async fn ready_count(&self, project_id: ProjectId) -> Result<i64> {
        blocking!(self, |conn: &Connection| ready_count(conn, project_id))
    }

    async fn list_ready_tasks(&self, project_id: ProjectId) -> Result<Vec<Task>> {
        blocking!(self, |conn: &Connection| list_ready_tasks(conn, project_id))
    }

    async fn is_task_ready(&self, project_id: ProjectId, id: TaskId) -> Result<bool> {
        blocking!(self, |conn: &Connection| is_task_ready(
            conn, project_id, id
        ))
    }
}

#[async_trait]
impl MetadataFieldRepository for SqliteBackend {
    async fn create_metadata_field(
        &self,
        project_id: ProjectId,
        params: &CreateMetadataFieldParams,
    ) -> Result<MetadataField> {
        let params = params.clone();
        blocking!(self, |conn: &Connection| create_metadata_field(
            conn, project_id, &params
        ))
    }

    async fn get_metadata_field(
        &self,
        project_id: ProjectId,
        field_id: i64,
    ) -> Result<MetadataField> {
        blocking!(self, |conn: &Connection| get_metadata_field(
            conn, project_id, field_id
        ))
    }

    async fn list_metadata_fields(
        &self,
        project_id: ProjectId,
        filter: &ListMetadataFieldsFilter,
    ) -> Result<ListPage<MetadataField>> {
        let filter = filter.clone();
        blocking!(self, |conn: &Connection| list_metadata_fields(
            conn, project_id, &filter
        ))
    }

    async fn update_metadata_field(
        &self,
        project_id: ProjectId,
        field_id: i64,
        params: &UpdateMetadataFieldParams,
    ) -> Result<MetadataField> {
        let params = params.clone();
        blocking!(self, |conn: &Connection| update_metadata_field(
            conn, project_id, field_id, &params
        ))
    }

    async fn delete_metadata_field(&self, project_id: ProjectId, field_id: i64) -> Result<()> {
        blocking!(self, |conn: &Connection| delete_metadata_field(
            conn, project_id, field_id
        ))
    }
}

#[async_trait]
impl ContractRepository for SqliteBackend {
    async fn create_contract(
        &self,
        project_id: ProjectId,
        params: &CreateContractParams,
    ) -> Result<Contract> {
        let params = params.clone();
        blocking!(self, |conn: &Connection| create_contract(
            conn, project_id, &params
        ))
    }

    async fn get_contract(&self, id: ContractId) -> Result<Contract> {
        blocking!(self, |conn: &Connection| get_contract(conn, id))
    }

    async fn list_contracts(
        &self,
        project_id: ProjectId,
        filter: &ListContractsFilter,
    ) -> Result<ListPage<Contract>> {
        let filter = filter.clone();
        blocking!(self, |conn: &Connection| list_contracts(
            conn, project_id, &filter
        ))
    }

    async fn list_contract_notes(
        &self,
        contract_id: ContractId,
        filter: &ListContractNotesFilter,
    ) -> Result<ListPage<ContractNote>> {
        let filter = filter.clone();
        blocking!(self, |conn: &Connection| list_contract_notes(
            conn,
            contract_id,
            &filter
        ))
    }

    async fn update_contract(
        &self,
        id: ContractId,
        update: &UpdateContractParams,
        array_update: &UpdateContractArrayParams,
    ) -> Result<Contract> {
        let update = update.clone();
        let array_update = array_update.clone();
        blocking!(self, |conn: &Connection| update_contract(
            conn,
            id,
            &update,
            &array_update
        ))
    }

    async fn delete_contract(&self, id: ContractId) -> Result<()> {
        blocking!(self, |conn: &Connection| delete_contract(conn, id))
    }

    async fn add_note(&self, contract_id: ContractId, note: &ContractNote) -> Result<ContractNote> {
        let note = note.clone();
        blocking!(self, |conn: &Connection| add_contract_note(
            conn,
            contract_id,
            &note
        ))
    }

    async fn check_dod(&self, contract_id: ContractId, index: usize) -> Result<Contract> {
        blocking!(self, |conn: &Connection| set_contract_dod_checked(
            conn,
            contract_id,
            index,
            true
        ))
    }

    async fn uncheck_dod(&self, contract_id: ContractId, index: usize) -> Result<Contract> {
        blocking!(self, |conn: &Connection| set_contract_dod_checked(
            conn,
            contract_id,
            index,
            false
        ))
    }
}

crate::impl_task_transition_default!(SqliteBackend);

#[cfg(feature = "dev-tools")]
#[async_trait]
impl crate::application::port::SeederPort for SqliteBackend {
    async fn wipe_for_seed(&self) -> Result<()> {
        blocking!(self, |conn: &Connection| {
            // Order matters: child rows first, then parents. The bootstrap rows
            // (project id=1, user id=1, the matching project_member) must
            // survive so subsequent senko operations keep working.
            let stmts = [
                "DELETE FROM task_dependencies",
                "DELETE FROM task_definition_of_done",
                "DELETE FROM task_in_scope",
                "DELETE FROM task_out_of_scope",
                "DELETE FROM task_tags",
                "DELETE FROM contract_definition_of_done",
                "DELETE FROM contract_tags",
                "DELETE FROM contract_notes",
                "DELETE FROM tasks",
                "DELETE FROM contracts",
                "DELETE FROM metadata_fields",
                "DELETE FROM api_keys",
                "DELETE FROM project_members WHERE NOT (project_id = 1 AND user_id = 1)",
                "DELETE FROM users WHERE id != 1",
                "DELETE FROM projects WHERE id != 1",
                // Reset AUTOINCREMENT counters for tables we just emptied so a
                // fresh seed always produces the same id sequence.
                "DELETE FROM sqlite_sequence WHERE name IN ('tasks','contracts','contract_notes','metadata_fields','api_keys')",
            ];
            conn.execute_batch("BEGIN")?;
            for s in stmts {
                conn.execute(s, [])?;
            }
            conn.execute_batch("COMMIT")?;
            Ok(())
        })
    }

    async fn has_seeded_data(&self) -> Result<bool> {
        blocking!(self, |conn: &Connection| {
            let n: i64 = conn.query_row(
                "SELECT COUNT(*) FROM task_tags WHERE tag = 'seed'",
                [],
                |r| r.get(0),
            )?;
            Ok(n > 0)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::task::AssigneeUserId;

    fn setup() -> (tempfile::TempDir, Connection) {
        let tmp = tempfile::tempdir().unwrap();
        let db_path = tmp.path().join("data.db");
        let conn = open_db(
            tmp.path(),
            Some(db_path.as_path()),
            None,
            &XdgDirs::default(),
        )
        .unwrap();
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
            assignee_user_id: None,
            contract_id: None,
        }
    }

    /// Helper to transition a task through statuses using domain methods + save
    fn transition_to(conn: &Connection, id: TaskDbId, target: TaskStatus) {
        let task = get_task(conn, id).unwrap();
        match target {
            TaskStatus::Draft => {} // already draft
            TaskStatus::Todo => {
                let (task, _) = task.publish("2025-01-01T00:00:00Z".to_string()).unwrap();
                save_task(conn, &task).unwrap();
            }
            TaskStatus::InProgress => {
                let (task, _) = task.publish("2025-01-01T00:00:00Z".to_string()).unwrap();
                let (task, _) = task
                    .start(None, None, "2025-01-01T00:00:00Z".to_string(), None)
                    .unwrap();
                save_task(conn, &task).unwrap();
            }
            TaskStatus::Completed => {
                let (task, _) = task.publish("2025-01-01T00:00:00Z".to_string()).unwrap();
                let (task, _) = task
                    .start(None, None, "2025-01-01T00:00:00Z".to_string(), None)
                    .unwrap();
                let (task, _) = task.complete("2025-01-01T00:00:00Z".to_string()).unwrap();
                save_task(conn, &task).unwrap();
            }
            TaskStatus::Canceled => {
                let (task, _) = task
                    .cancel("2025-01-01T00:00:00Z".to_string(), None)
                    .unwrap();
                save_task(conn, &task).unwrap();
            }
        }
    }

    #[test]
    fn creates_db_at_explicit_path() {
        let tmp = tempfile::tempdir().unwrap();
        let db_path = tmp.path().join("custom.db");
        let conn = open_db(
            tmp.path(),
            Some(db_path.as_path()),
            None,
            &XdgDirs::default(),
        )
        .unwrap();
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
        let _conn1 = open_db(
            tmp.path(),
            Some(db_path.as_path()),
            None,
            &XdgDirs::default(),
        )
        .unwrap();
        drop(_conn1);
        let _conn2 = open_db(
            tmp.path(),
            Some(db_path.as_path()),
            None,
            &XdgDirs::default(),
        )
        .unwrap();
    }

    #[test]
    fn create_and_get_task() {
        let (_tmp, conn) = setup();
        let task = create_task(
            &conn,
            ProjectId(1),
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
                assignee_user_id: None,
                contract_id: None,
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

        let fetched = get_task(&conn, TaskDbId(task.id().into())).unwrap();
        assert_eq!(fetched.title(), task.title());
        assert_eq!(fetched.tags(), task.tags());
    }

    #[test]
    fn create_task_default_priority() {
        let (_tmp, conn) = setup();
        let task =
            create_task(&conn, ProjectId(1), &default_create_params("default prio")).unwrap();
        assert_eq!(task.priority(), Priority::P2);
    }

    #[test]
    fn update_task_fields() {
        let (_tmp, conn) = setup();
        let task = create_task(&conn, ProjectId(1), &default_create_params("original")).unwrap();

        let updated = update_task(
            &conn,
            TaskDbId(task.id().into()),
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
                contract_id: None,
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
        let task = create_task(&conn, ProjectId(1), &default_create_params("t")).unwrap();
        assert_eq!(task.status(), TaskStatus::Draft);

        // draft -> todo via domain method + save
        let task = get_task(&conn, TaskDbId(task.id().into())).unwrap();
        let (task, _) = task.publish("2025-01-01T00:00:00Z".to_string()).unwrap();
        save_task(&conn, &task).unwrap();
        let updated = get_task(&conn, TaskDbId(task.id().into())).unwrap();
        assert_eq!(updated.status(), TaskStatus::Todo);

        // todo -> in_progress
        let (task, _) = updated
            .start(
                Some("session-1".into()),
                None,
                "2025-01-01T00:00:00Z".to_string(),
                None,
            )
            .unwrap();
        save_task(&conn, &task).unwrap();
        let updated = get_task(&conn, TaskDbId(task.id().into())).unwrap();
        assert_eq!(updated.status(), TaskStatus::InProgress);
        assert_eq!(updated.assignee_session_id(), Some("session-1"));
        assert_eq!(updated.started_at(), Some("2025-01-01T00:00:00Z"));

        // in_progress -> completed
        let (task, _) = updated
            .complete("2025-01-01T01:00:00Z".to_string())
            .unwrap();
        save_task(&conn, &task).unwrap();
        let updated = get_task(&conn, TaskDbId(task.id().into())).unwrap();
        assert_eq!(updated.status(), TaskStatus::Completed);
        assert_eq!(updated.completed_at(), Some("2025-01-01T01:00:00Z"));
    }

    #[test]
    fn cancel_task_from_any_active_status() {
        let (_tmp, conn) = setup();

        // cancel from draft
        let t1 = create_task(&conn, ProjectId(1), &default_create_params("t1")).unwrap();
        let task = get_task(&conn, TaskDbId(t1.id().into())).unwrap();
        let (task, _) = task
            .cancel("2025-01-01T00:00:00Z".to_string(), Some("reason1".into()))
            .unwrap();
        save_task(&conn, &task).unwrap();
        let canceled = get_task(&conn, TaskDbId(t1.id().into())).unwrap();
        assert_eq!(canceled.status(), TaskStatus::Canceled);
        assert_eq!(canceled.cancel_reason(), Some("reason1"));

        // cancel from todo
        let t2 = create_task(&conn, ProjectId(1), &default_create_params("t2")).unwrap();
        transition_to(&conn, TaskDbId(t2.id().into()), TaskStatus::Todo);
        let task = get_task(&conn, TaskDbId(t2.id().into())).unwrap();
        let (task, _) = task
            .cancel("2025-01-01T00:00:00Z".to_string(), None)
            .unwrap();
        save_task(&conn, &task).unwrap();
        let canceled = get_task(&conn, TaskDbId(t2.id().into())).unwrap();
        assert_eq!(canceled.status(), TaskStatus::Canceled);

        // cancel from in_progress
        let t3 = create_task(&conn, ProjectId(1), &default_create_params("t3")).unwrap();
        transition_to(&conn, TaskDbId(t3.id().into()), TaskStatus::InProgress);
        let task = get_task(&conn, TaskDbId(t3.id().into())).unwrap();
        let (task, _) = task
            .cancel("2025-01-01T00:00:00Z".to_string(), None)
            .unwrap();
        save_task(&conn, &task).unwrap();
        let canceled = get_task(&conn, TaskDbId(t3.id().into())).unwrap();
        assert_eq!(canceled.status(), TaskStatus::Canceled);
    }

    #[test]
    fn delete_task_cascade() {
        let (_tmp, conn) = setup();
        let task = create_task(
            &conn,
            ProjectId(1),
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
                assignee_user_id: None,
                contract_id: None,
            },
        )
        .unwrap();

        delete_task(&conn, TaskDbId(task.id().into())).unwrap();

        assert!(get_task(&conn, TaskDbId(task.id().into())).is_err());

        let count: i32 = conn
            .query_row(
                "SELECT COUNT(*) FROM task_tags WHERE task_id = ?1",
                params![i64::from(task.id())],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 0);

        let count: i32 = conn
            .query_row(
                "SELECT COUNT(*) FROM task_definition_of_done WHERE task_id = ?1",
                params![i64::from(task.id())],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn delete_nonexistent_task() {
        let (_tmp, conn) = setup();
        assert!(delete_task(&conn, TaskDbId(99999)).is_err());
    }

    #[test]
    fn list_tasks_no_filter() {
        let (_tmp, conn) = setup();
        create_task(&conn, ProjectId(1), &default_create_params("a")).unwrap();
        create_task(&conn, ProjectId(1), &default_create_params("b")).unwrap();

        let tasks = list_tasks(&conn, ProjectId(1), &ListTasksFilter::default())
            .unwrap()
            .items;
        assert_eq!(tasks.len(), 2);
    }

    #[test]
    fn list_tasks_filter_by_status() {
        let (_tmp, conn) = setup();
        let t1 = create_task(&conn, ProjectId(1), &default_create_params("draft")).unwrap();
        let _t2 = create_task(&conn, ProjectId(1), &default_create_params("todo")).unwrap();

        // Move t1 to todo
        transition_to(&conn, TaskDbId(t1.id().into()), TaskStatus::Todo);

        let drafts = list_tasks(
            &conn,
            ProjectId(1),
            &ListTasksFilter {
                statuses: vec![TaskStatus::Draft],
                ..Default::default()
            },
        )
        .unwrap()
        .items;
        assert_eq!(drafts.len(), 1);
        assert_eq!(drafts[0].title(), "todo");

        let todos = list_tasks(
            &conn,
            ProjectId(1),
            &ListTasksFilter {
                statuses: vec![TaskStatus::Todo],
                ..Default::default()
            },
        )
        .unwrap()
        .items;
        assert_eq!(todos.len(), 1);
        assert_eq!(todos[0].title(), "draft");
    }

    #[test]
    fn list_tasks_filter_by_tag() {
        let (_tmp, conn) = setup();
        create_task(
            &conn,
            ProjectId(1),
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
        create_task(&conn, ProjectId(1), &default_create_params("untagged")).unwrap();

        let result = list_tasks(
            &conn,
            ProjectId(1),
            &ListTasksFilter {
                tags: vec!["rust".to_string()],
                ..Default::default()
            },
        )
        .unwrap()
        .items;
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].title(), "tagged");
    }

    #[test]
    fn list_tasks_ready_filter() {
        let (_tmp, conn) = setup();

        // Create dep task and move to completed
        let dep = create_task(&conn, ProjectId(1), &default_create_params("dep")).unwrap();
        transition_to(&conn, TaskDbId(dep.id().into()), TaskStatus::Completed);

        // Create task with completed dep -> should be ready
        let ready_t = create_task(
            &conn,
            ProjectId(1),
            &CreateTaskParams {
                title: "ready".to_string(),
                dependencies: vec![dep.id()],
                ..default_create_params("ready")
            },
        )
        .unwrap();
        transition_to(&conn, TaskDbId(ready_t.id().into()), TaskStatus::Todo);

        // Create another dep that is NOT completed
        let dep2 = create_task(&conn, ProjectId(1), &default_create_params("dep2")).unwrap();
        let blocked_task = create_task(
            &conn,
            ProjectId(1),
            &CreateTaskParams {
                title: "blocked".to_string(),
                dependencies: vec![dep2.id()],
                ..default_create_params("blocked")
            },
        )
        .unwrap();
        transition_to(&conn, TaskDbId(blocked_task.id().into()), TaskStatus::Todo);

        let result = list_tasks(
            &conn,
            ProjectId(1),
            &ListTasksFilter {
                ready: true,
                ..Default::default()
            },
        )
        .unwrap()
        .items;

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].title(), "ready");
    }

    #[test]
    fn list_tasks_filter_by_contract_id() {
        let (_tmp, conn) = setup();
        let contract = create_contract(
            &conn,
            ProjectId(1),
            &CreateContractParams {
                title: "c".to_string(),
                description: None,
                definition_of_done: vec![],
                tags: vec![],
                metadata: None,
            },
        )
        .unwrap();

        let mut with_contract = default_create_params("linked");
        with_contract.contract_id = Some(contract.id());
        create_task(&conn, ProjectId(1), &with_contract).unwrap();
        create_task(&conn, ProjectId(1), &default_create_params("unlinked")).unwrap();

        let matched = list_tasks(
            &conn,
            ProjectId(1),
            &ListTasksFilter {
                contract_id: Some(contract.id()),
                ..Default::default()
            },
        )
        .unwrap()
        .items;
        assert_eq!(matched.len(), 1);
        assert_eq!(matched[0].title(), "linked");

        let nomatch = list_tasks(
            &conn,
            ProjectId(1),
            &ListTasksFilter {
                contract_id: Some(ContractId(contract.id().0 + 9999)),
                ..Default::default()
            },
        )
        .unwrap()
        .items;
        assert!(nomatch.is_empty());
    }

    #[test]
    fn list_tasks_filter_by_id_range() {
        let (_tmp, conn) = setup();
        let a = create_task(&conn, ProjectId(1), &default_create_params("a")).unwrap();
        let b = create_task(&conn, ProjectId(1), &default_create_params("b")).unwrap();
        let c = create_task(&conn, ProjectId(1), &default_create_params("c")).unwrap();

        let ge_b = list_tasks(
            &conn,
            ProjectId(1),
            &ListTasksFilter {
                id_min: Some(b.id()),
                ..Default::default()
            },
        )
        .unwrap()
        .items;
        assert_eq!(ge_b.len(), 2);

        let le_b = list_tasks(
            &conn,
            ProjectId(1),
            &ListTasksFilter {
                id_max: Some(b.id()),
                ..Default::default()
            },
        )
        .unwrap()
        .items;
        assert_eq!(le_b.len(), 2);

        let just_b = list_tasks(
            &conn,
            ProjectId(1),
            &ListTasksFilter {
                id_min: Some(b.id()),
                id_max: Some(b.id()),
                ..Default::default()
            },
        )
        .unwrap()
        .items;
        assert_eq!(just_b.len(), 1);
        assert_eq!(just_b[0].id(), b.id());

        // Sanity: a < b < c
        assert!(a.id() < b.id() && b.id() < c.id());
    }

    #[test]
    fn list_tasks_pagination_cursor() {
        let (_tmp, conn) = setup();
        for i in 0..5 {
            create_task(
                &conn,
                ProjectId(1),
                &default_create_params(&format!("t{i}")),
            )
            .unwrap();
        }

        let all = list_tasks(&conn, ProjectId(1), &ListTasksFilter::default()).unwrap();
        assert_eq!(all.items.len(), 5);
        assert!(all.next_cursor.is_none());

        // First page: limit 2 → 2 items + next_cursor pointing to the 2nd task.
        let page1 = list_tasks(
            &conn,
            ProjectId(1),
            &ListTasksFilter {
                limit: Some(2),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(page1.items.len(), 2);
        assert_eq!(page1.items[0].id(), all.items[0].id());
        let cursor1 = page1.next_cursor.expect("next_cursor for first page");

        // Second page: decode cursor → take next 2 items, cursor still points to more.
        let after1 = Cursor::decode(&cursor1).unwrap();
        let page2 = list_tasks(
            &conn,
            ProjectId(1),
            &ListTasksFilter {
                limit: Some(2),
                after: Some(after1),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(page2.items.len(), 2);
        assert_eq!(page2.items[0].id(), all.items[2].id());
        assert_eq!(page2.items[1].id(), all.items[3].id());
        assert!(page2.next_cursor.is_some());

        // Third page: exactly 1 remaining, next_cursor is None (end).
        let after2 = Cursor::decode(&page2.next_cursor.unwrap()).unwrap();
        let page3 = list_tasks(
            &conn,
            ProjectId(1),
            &ListTasksFilter {
                limit: Some(2),
                after: Some(after2),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(page3.items.len(), 1);
        assert_eq!(page3.items[0].id(), all.items[4].id());
        assert!(page3.next_cursor.is_none());
    }

    #[test]
    fn list_tasks_cursor_exact_boundary() {
        // When total count is an exact multiple of limit, the last page
        // must report next_cursor == None (no dangling "more" signal).
        let (_tmp, conn) = setup();
        for i in 0..4 {
            create_task(
                &conn,
                ProjectId(1),
                &default_create_params(&format!("t{i}")),
            )
            .unwrap();
        }

        let page1 = list_tasks(
            &conn,
            ProjectId(1),
            &ListTasksFilter {
                limit: Some(2),
                ..Default::default()
            },
        )
        .unwrap();
        let cursor1 = page1.next_cursor.expect("first page has more");

        let after1 = Cursor::decode(&cursor1).unwrap();
        let page2 = list_tasks(
            &conn,
            ProjectId(1),
            &ListTasksFilter {
                limit: Some(2),
                after: Some(after1),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(page2.items.len(), 2);
        assert!(page2.next_cursor.is_none());
    }

    // --- Cursor pagination smoke tests for each list (task #337) ---

    #[test]
    fn list_projects_cursor_roundtrip() {
        let (_tmp, conn) = setup();
        for name in &["alpha", "bravo", "charlie"] {
            create_project(
                &conn,
                &CreateProjectParams {
                    name: name.to_string(),
                    description: None,
                },
            )
            .unwrap();
        }
        // 1 existing default + 3 newly created = 4 projects.
        let page1 = list_projects(
            &conn,
            &ListProjectsFilter {
                limit: Some(2),
                after: None,
            },
        )
        .unwrap();
        assert_eq!(page1.items.len(), 2);
        let cursor1 = page1.next_cursor.expect("more pages");
        let after1: ProjectId = Cursor::decode(&cursor1).unwrap();
        let page2 = list_projects(
            &conn,
            &ListProjectsFilter {
                limit: Some(2),
                after: Some(after1),
            },
        )
        .unwrap();
        assert_eq!(page2.items.len(), 2);
        assert!(page2.next_cursor.is_none());
    }

    #[test]
    fn list_users_cursor_roundtrip() {
        let (_tmp, conn) = setup();
        for n in &["a", "b", "c"] {
            create_user(
                &conn,
                &CreateUserParams {
                    username: Username(n.to_string()),
                    sub: None,
                    display_name: None,
                    email: None,
                },
            )
            .unwrap();
        }
        let page1 = list_users(
            &conn,
            &ListUsersFilter {
                limit: Some(2),
                after: None,
            },
        )
        .unwrap();
        assert_eq!(page1.items.len(), 2);
        let cursor1 = page1.next_cursor.expect("more pages");
        let after1: i64 = Cursor::decode(&cursor1).unwrap();
        let page2 = list_users(
            &conn,
            &ListUsersFilter {
                limit: Some(2),
                after: Some(after1),
            },
        )
        .unwrap();
        assert_eq!(page2.items.len(), 2);
        assert!(page2.next_cursor.is_none());
    }

    #[test]
    fn list_contracts_cursor_and_tag_filter() {
        let (_tmp, conn) = setup();
        create_contract(
            &conn,
            ProjectId(1),
            &CreateContractParams {
                title: "A".into(),
                description: None,
                definition_of_done: vec![],
                tags: vec!["t1".into()],
                metadata: None,
            },
        )
        .unwrap();
        create_contract(
            &conn,
            ProjectId(1),
            &CreateContractParams {
                title: "B".into(),
                description: None,
                definition_of_done: vec![],
                tags: vec!["t1".into(), "t2".into()],
                metadata: None,
            },
        )
        .unwrap();
        create_contract(
            &conn,
            ProjectId(1),
            &CreateContractParams {
                title: "C".into(),
                description: None,
                definition_of_done: vec![],
                tags: vec!["t2".into()],
                metadata: None,
            },
        )
        .unwrap();

        // Tag filter t1 → contracts A and B. limit 1 → first, cursor to next.
        let page1 = list_contracts(
            &conn,
            ProjectId(1),
            &ListContractsFilter {
                tags: vec!["t1".into()],
                limit: Some(1),
                after: None,
            },
        )
        .unwrap();
        assert_eq!(page1.items.len(), 1);
        assert_eq!(page1.items[0].title(), "A");
        let cursor = page1.next_cursor.expect("more");
        let after: ContractId = Cursor::decode(&cursor).unwrap();
        let page2 = list_contracts(
            &conn,
            ProjectId(1),
            &ListContractsFilter {
                tags: vec!["t1".into()],
                limit: Some(1),
                after: Some(after),
            },
        )
        .unwrap();
        assert_eq!(page2.items.len(), 1);
        assert_eq!(page2.items[0].title(), "B");
        assert!(page2.next_cursor.is_none());
    }

    #[test]
    fn list_dependencies_cursor_roundtrip() {
        let (_tmp, conn) = setup();
        // Parent task depending on 3 others.
        let deps: Vec<_> = (0..3)
            .map(|i| {
                create_task(
                    &conn,
                    ProjectId(1),
                    &default_create_params(&format!("d{i}")),
                )
                .unwrap()
            })
            .collect();
        let parent = create_task(&conn, ProjectId(1), &default_create_params("p")).unwrap();

        // Wire up dependencies by updating parent's dep list.
        let (parent, _) = parent
            .set_dependencies(
                deps.iter().map(|t| t.id()).collect::<Vec<_>>().as_slice(),
                Some("2026-04-22T00:00:00Z".into()),
            )
            .unwrap();
        save_task(&conn, &parent).unwrap();

        let parent_db = TaskDbId(parent.id().into());
        let page1 = list_dependencies(
            &conn,
            parent_db,
            &ListTaskDepsFilter {
                limit: Some(2),
                after: None,
            },
        )
        .unwrap();
        assert_eq!(page1.items.len(), 2);
        let cursor1 = page1.next_cursor.expect("more");
        let after1: TaskId = Cursor::decode(&cursor1).unwrap();
        let page2 = list_dependencies(
            &conn,
            parent_db,
            &ListTaskDepsFilter {
                limit: Some(2),
                after: Some(after1),
            },
        )
        .unwrap();
        assert_eq!(page2.items.len(), 1);
        assert!(page2.next_cursor.is_none());
    }

    #[test]
    fn list_contract_notes_cursor_roundtrip() {
        let (_tmp, conn) = setup();
        let contract = create_contract(
            &conn,
            ProjectId(1),
            &CreateContractParams {
                title: "T".into(),
                description: None,
                definition_of_done: vec![],
                tags: vec![],
                metadata: None,
            },
        )
        .unwrap();
        for i in 0..4 {
            let note = ContractNote::new(format!("n{i}"), None, "2026-04-22T00:00:00Z".into());
            add_contract_note(&conn, contract.id(), &note).unwrap();
        }

        let page1 = list_contract_notes(
            &conn,
            contract.id(),
            &ListContractNotesFilter {
                limit: Some(2),
                after: None,
            },
        )
        .unwrap();
        assert_eq!(page1.items.len(), 2);
        let cursor1 = page1.next_cursor.expect("more");
        let after1: i64 = Cursor::decode(&cursor1).unwrap();
        let page2 = list_contract_notes(
            &conn,
            contract.id(),
            &ListContractNotesFilter {
                limit: Some(2),
                after: Some(after1),
            },
        )
        .unwrap();
        assert_eq!(page2.items.len(), 2);
        assert!(page2.next_cursor.is_none());
    }

    #[test]
    fn cursor_encode_decode_roundtrip() {
        let id = TaskId(42);
        let encoded = Cursor::encode(id);
        let decoded: TaskId = Cursor::decode(&encoded).unwrap();
        assert_eq!(decoded, id);
    }

    #[test]
    fn cursor_decode_invalid() {
        assert!(Cursor::decode::<TaskId>("not!valid").is_err());
        assert!(Cursor::decode::<TaskId>("").is_err());
        // valid base64 but invalid JSON
        assert!(Cursor::decode::<TaskId>("aGVsbG8").is_err());
    }

    #[test]
    fn unique_constraints() {
        let (_tmp, conn) = setup();
        let task = create_task(
            &conn,
            ProjectId(1),
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
            params![i64::from(task.id())],
        );
        assert!(result.is_err());
    }

    #[test]
    fn task_with_dependencies() {
        let (_tmp, conn) = setup();
        let dep1 = create_task(&conn, ProjectId(1), &default_create_params("dep1")).unwrap();
        let dep2 = create_task(&conn, ProjectId(1), &default_create_params("dep2")).unwrap();

        let task = create_task(
            &conn,
            ProjectId(1),
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
            ProjectId(1),
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
            TaskDbId(task.id().into()),
            &UpdateTaskArrayParams {
                set_tags: Some(vec!["new1".to_string(), "new2".to_string()]),
                ..default_array_params()
            },
        )
        .unwrap();

        let updated = get_task(&conn, TaskDbId(task.id().into())).unwrap();
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
            ProjectId(1),
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
            TaskDbId(task.id().into()),
            &UpdateTaskArrayParams {
                add_tags: vec!["new".to_string(), "existing".to_string()],
                ..default_array_params()
            },
        )
        .unwrap();

        let updated = get_task(&conn, TaskDbId(task.id().into())).unwrap();
        assert_eq!(updated.tags().len(), 2);
        assert!(updated.tags().contains(&"existing".to_string()));
        assert!(updated.tags().contains(&"new".to_string()));
    }

    #[test]
    fn update_arrays_remove_tags() {
        let (_tmp, conn) = setup();
        let task = create_task(
            &conn,
            ProjectId(1),
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
            TaskDbId(task.id().into()),
            &UpdateTaskArrayParams {
                remove_tags: vec!["remove".to_string()],
                ..default_array_params()
            },
        )
        .unwrap();

        let updated = get_task(&conn, TaskDbId(task.id().into())).unwrap();
        assert_eq!(updated.tags(), &["keep"]);
    }

    #[test]
    fn update_arrays_set_definition_of_done() {
        let (_tmp, conn) = setup();
        let task = create_task(
            &conn,
            ProjectId(1),
            &CreateTaskParams {
                definition_of_done: vec!["old".to_string()],
                ..default_create_params("t")
            },
        )
        .unwrap();

        update_task_arrays(
            &conn,
            TaskDbId(task.id().into()),
            &UpdateTaskArrayParams {
                set_definition_of_done: Some(vec!["new1".to_string(), "new2".to_string()]),
                ..default_array_params()
            },
        )
        .unwrap();

        let updated = get_task(&conn, TaskDbId(task.id().into())).unwrap();
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
            ProjectId(1),
            &CreateTaskParams {
                in_scope: vec!["a".to_string(), "b".to_string()],
                ..default_create_params("t")
            },
        )
        .unwrap();

        update_task_arrays(
            &conn,
            TaskDbId(task.id().into()),
            &UpdateTaskArrayParams {
                add_in_scope: vec!["c".to_string()],
                remove_in_scope: vec!["a".to_string()],
                ..default_array_params()
            },
        )
        .unwrap();

        let updated = get_task(&conn, TaskDbId(task.id().into())).unwrap();
        assert_eq!(updated.in_scope(), &["b", "c"]);
    }

    fn make_todo(conn: &Connection, title: &str, priority: Option<Priority>) -> Task {
        let task = create_task(
            conn,
            ProjectId(1),
            &CreateTaskParams {
                priority,
                ..default_create_params(title)
            },
        )
        .unwrap();
        transition_to(conn, TaskDbId(task.id().into()), TaskStatus::Todo);
        get_task(conn, TaskDbId(task.id().into())).unwrap()
    }

    fn make_completed(conn: &Connection, title: &str) -> Task {
        let task = create_task(conn, ProjectId(1), &default_create_params(title)).unwrap();
        transition_to(conn, TaskDbId(task.id().into()), TaskStatus::Completed);
        get_task(conn, TaskDbId(task.id().into())).unwrap()
    }

    #[test]
    fn next_task_returns_none_when_empty() {
        let (_tmp, conn) = setup();
        assert!(
            next_task(&conn, ProjectId(1), None, false)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn next_task_skips_blocked() {
        let (_tmp, conn) = setup();

        // Create a dep that is NOT completed (still draft)
        let dep = create_task(&conn, ProjectId(1), &default_create_params("dep")).unwrap();

        // Create a todo task that depends on dep
        let task = create_task(
            &conn,
            ProjectId(1),
            &CreateTaskParams {
                title: "blocked".to_string(),
                dependencies: vec![dep.id()],
                ..default_create_params("blocked")
            },
        )
        .unwrap();
        transition_to(&conn, TaskDbId(task.id().into()), TaskStatus::Todo);

        assert!(
            next_task(&conn, ProjectId(1), None, false)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn next_task_priority_order() {
        let (_tmp, conn) = setup();

        make_todo(&conn, "low", Some(Priority::P3));
        make_todo(&conn, "high", Some(Priority::P0));
        make_todo(&conn, "mid", Some(Priority::P1));

        let task = next_task(&conn, ProjectId(1), None, false)
            .unwrap()
            .unwrap();
        assert_eq!(task.title(), "high");
    }

    #[test]
    fn next_task_created_at_tiebreak() {
        let (_tmp, conn) = setup();

        // Same priority, created_at order should decide
        // Since tasks are inserted sequentially, the first one has earlier created_at
        make_todo(&conn, "first", Some(Priority::P2));
        make_todo(&conn, "second", Some(Priority::P2));

        let task = next_task(&conn, ProjectId(1), None, false)
            .unwrap()
            .unwrap();
        assert_eq!(task.title(), "first");
    }

    #[test]
    fn next_task_id_tiebreak() {
        let (_tmp, conn) = setup();

        // Insert two tasks with same priority; SQLite created_at has second-level precision
        // so they'll likely have the same created_at, making id the final tiebreaker
        let t1 = make_todo(&conn, "t1", Some(Priority::P2));
        let t2 = make_todo(&conn, "t2", Some(Priority::P2));

        let task = next_task(&conn, ProjectId(1), None, false)
            .unwrap()
            .unwrap();
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
            ProjectId(1),
            &CreateTaskParams {
                title: "ready".to_string(),
                dependencies: vec![dep.id()],
                ..default_create_params("ready")
            },
        )
        .unwrap();
        transition_to(&conn, TaskDbId(task.id().into()), TaskStatus::Todo);

        let result = next_task(&conn, ProjectId(1), None, false)
            .unwrap()
            .unwrap();
        assert_eq!(result.title(), "ready");
    }

    #[test]
    fn next_task_filters_by_user_id() {
        let (_tmp, conn) = setup();
        let user2 = create_user(
            &conn,
            &CreateUserParams {
                username: Username("user2".to_string()),
                sub: None,
                display_name: None,
                email: None,
            },
        )
        .unwrap();
        let t1 = create_task(
            &conn,
            ProjectId(1),
            &CreateTaskParams {
                assignee_user_id: Some(AssigneeUserId::Id(UserId(1))),
                ..default_create_params("user1-task")
            },
        )
        .unwrap();
        let t2 = create_task(
            &conn,
            ProjectId(1),
            &CreateTaskParams {
                assignee_user_id: Some(AssigneeUserId::Id(user2.id())),
                ..default_create_params("user2-task")
            },
        )
        .unwrap();
        transition_to(&conn, TaskDbId(t1.id().into()), TaskStatus::Todo);
        transition_to(&conn, TaskDbId(t2.id().into()), TaskStatus::Todo);

        let result = next_task(&conn, ProjectId(1), Some(UserId(1)), false)
            .unwrap()
            .unwrap();
        assert_eq!(result.title(), "user1-task");
    }

    #[test]
    fn next_task_includes_unassigned_when_flag_set() {
        let (_tmp, conn) = setup();
        // Lower priority (P2) assigned task
        let t1 = create_task(
            &conn,
            ProjectId(1),
            &CreateTaskParams {
                assignee_user_id: Some(AssigneeUserId::Id(UserId(1))),
                priority: Some(Priority::P2),
                ..default_create_params("assigned")
            },
        )
        .unwrap();
        // Higher priority (P1) unassigned task
        let t2 = create_task(
            &conn,
            ProjectId(1),
            &CreateTaskParams {
                assignee_user_id: None,
                priority: Some(Priority::P1),
                ..default_create_params("unassigned")
            },
        )
        .unwrap();
        transition_to(&conn, TaskDbId(t1.id().into()), TaskStatus::Todo);
        transition_to(&conn, TaskDbId(t2.id().into()), TaskStatus::Todo);

        let result = next_task(&conn, ProjectId(1), Some(UserId(1)), true)
            .unwrap()
            .unwrap();
        assert_eq!(result.title(), "unassigned");
    }

    #[test]
    fn next_task_excludes_unassigned_when_flag_unset() {
        let (_tmp, conn) = setup();
        let t1 = create_task(
            &conn,
            ProjectId(1),
            &CreateTaskParams {
                assignee_user_id: Some(AssigneeUserId::Id(UserId(1))),
                priority: Some(Priority::P2),
                ..default_create_params("assigned")
            },
        )
        .unwrap();
        let t2 = create_task(
            &conn,
            ProjectId(1),
            &CreateTaskParams {
                assignee_user_id: None,
                priority: Some(Priority::P1),
                ..default_create_params("unassigned")
            },
        )
        .unwrap();
        transition_to(&conn, TaskDbId(t1.id().into()), TaskStatus::Todo);
        transition_to(&conn, TaskDbId(t2.id().into()), TaskStatus::Todo);

        let result = next_task(&conn, ProjectId(1), Some(UserId(1)), false)
            .unwrap()
            .unwrap();
        assert_eq!(result.title(), "assigned");
    }

    #[test]
    fn next_task_no_filter_when_user_id_none() {
        let (_tmp, conn) = setup();
        let t1 = create_task(
            &conn,
            ProjectId(1),
            &CreateTaskParams {
                assignee_user_id: Some(AssigneeUserId::Id(UserId(1))),
                priority: Some(Priority::P2),
                ..default_create_params("assigned")
            },
        )
        .unwrap();
        let t2 = create_task(
            &conn,
            ProjectId(1),
            &CreateTaskParams {
                assignee_user_id: None,
                priority: Some(Priority::P1),
                ..default_create_params("unassigned")
            },
        )
        .unwrap();
        transition_to(&conn, TaskDbId(t1.id().into()), TaskStatus::Todo);
        transition_to(&conn, TaskDbId(t2.id().into()), TaskStatus::Todo);

        let result = next_task(&conn, ProjectId(1), None, false)
            .unwrap()
            .unwrap();
        assert_eq!(result.title(), "unassigned");
    }

    #[test]
    fn create_task_sets_assignee_user_id() {
        let (_tmp, conn) = setup();
        let task = create_task(
            &conn,
            ProjectId(1),
            &CreateTaskParams {
                assignee_user_id: Some(AssigneeUserId::Id(UserId(1))),
                ..default_create_params("with-assignee")
            },
        )
        .unwrap();
        assert_eq!(task.assignee_user_id(), Some(UserId(1)));
    }

    #[test]
    fn list_tasks_filters_by_assignee() {
        let (_tmp, conn) = setup();
        let user2 = create_user(
            &conn,
            &CreateUserParams {
                username: Username("user2".to_string()),
                sub: None,
                display_name: None,
                email: None,
            },
        )
        .unwrap();
        let _t1 = create_task(
            &conn,
            ProjectId(1),
            &CreateTaskParams {
                assignee_user_id: Some(AssigneeUserId::Id(UserId(1))),
                ..default_create_params("user1-task")
            },
        )
        .unwrap();
        let _t2 = create_task(
            &conn,
            ProjectId(1),
            &CreateTaskParams {
                assignee_user_id: Some(AssigneeUserId::Id(user2.id())),
                ..default_create_params("user2-task")
            },
        )
        .unwrap();
        let _t3 = create_task(
            &conn,
            ProjectId(1),
            &CreateTaskParams {
                assignee_user_id: None,
                ..default_create_params("unassigned-task")
            },
        )
        .unwrap();

        // Exact match (no unassigned)
        let filter = ListTasksFilter {
            assignee_user_id: Some(UserId(1)),
            include_unassigned: false,
            ..Default::default()
        };
        let tasks = list_tasks(&conn, ProjectId(1), &filter).unwrap().items;
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].title(), "user1-task");

        // With unassigned included
        let filter = ListTasksFilter {
            assignee_user_id: Some(UserId(1)),
            include_unassigned: true,
            ..Default::default()
        };
        let tasks = list_tasks(&conn, ProjectId(1), &filter).unwrap().items;
        assert_eq!(tasks.len(), 2);
        let titles: Vec<&str> = tasks.iter().map(|t| t.title()).collect();
        assert!(titles.contains(&"user1-task"));
        assert!(titles.contains(&"unassigned-task"));
    }

    // --- Dependency tests (via domain methods + save) ---

    #[test]
    fn save_persists_dependencies() {
        let (_tmp, conn) = setup();
        let t1 = create_task(&conn, ProjectId(1), &default_create_params("t1")).unwrap();
        let t2 = create_task(&conn, ProjectId(1), &default_create_params("t2")).unwrap();
        let t3 = create_task(&conn, ProjectId(1), &default_create_params("t3")).unwrap();

        let (t1, _) = t1
            .add_dependency(t2.id(), Some("2026-01-01T00:00:00Z".into()))
            .unwrap();
        let (t1, _) = t1
            .add_dependency(t3.id(), Some("2026-01-01T00:00:00Z".into()))
            .unwrap();
        save_task(&conn, &t1).unwrap();

        let loaded = get_task(&conn, TaskDbId(t1.id().into())).unwrap();
        assert_eq!(loaded.dependencies().len(), 2);
        assert!(loaded.dependencies().contains(&t2.id()));
        assert!(loaded.dependencies().contains(&t3.id()));
    }

    #[test]
    fn save_replaces_dependencies() {
        let (_tmp, conn) = setup();
        let t1 = create_task(&conn, ProjectId(1), &default_create_params("t1")).unwrap();
        let t2 = create_task(&conn, ProjectId(1), &default_create_params("t2")).unwrap();
        let t3 = create_task(&conn, ProjectId(1), &default_create_params("t3")).unwrap();

        let (t1, _) = t1
            .add_dependency(t2.id(), Some("2026-01-01T00:00:00Z".into()))
            .unwrap();
        save_task(&conn, &t1).unwrap();

        let (t1, _) = t1
            .set_dependencies(&[t3.id()], Some("2026-01-01T00:00:01Z".into()))
            .unwrap();
        save_task(&conn, &t1).unwrap();

        let loaded = get_task(&conn, TaskDbId(t1.id().into())).unwrap();
        assert_eq!(loaded.dependencies(), &[t3.id()]);
    }

    #[test]
    fn save_clears_dependencies() {
        let (_tmp, conn) = setup();
        let t1 = create_task(&conn, ProjectId(1), &default_create_params("t1")).unwrap();
        let t2 = create_task(&conn, ProjectId(1), &default_create_params("t2")).unwrap();

        let (t1, _) = t1
            .add_dependency(t2.id(), Some("2026-01-01T00:00:00Z".into()))
            .unwrap();
        save_task(&conn, &t1).unwrap();

        let (t1, _) = t1
            .set_dependencies(&[], Some("2026-01-01T00:00:01Z".into()))
            .unwrap();
        save_task(&conn, &t1).unwrap();

        let loaded = get_task(&conn, TaskDbId(t1.id().into())).unwrap();
        assert!(loaded.dependencies().is_empty());
    }

    #[test]
    fn list_dependencies_basic() {
        let (_tmp, conn) = setup();
        let t1 = create_task(&conn, ProjectId(1), &default_create_params("t1")).unwrap();
        let t2 = create_task(&conn, ProjectId(1), &default_create_params("t2")).unwrap();
        let t3 = create_task(&conn, ProjectId(1), &default_create_params("t3")).unwrap();

        let (t1, _) = t1
            .add_dependency(t2.id(), Some("2026-01-01T00:00:00Z".into()))
            .unwrap();
        let (t1, _) = t1
            .add_dependency(t3.id(), Some("2026-01-01T00:00:00Z".into()))
            .unwrap();
        save_task(&conn, &t1).unwrap();

        let deps = list_dependencies(
            &conn,
            TaskDbId(t1.id().into()),
            &ListTaskDepsFilter::default(),
        )
        .unwrap();
        assert_eq!(deps.items.len(), 2);
        let dep_ids: Vec<TaskId> = deps.items.iter().map(|t| t.id()).collect();
        assert!(dep_ids.contains(&t2.id()));
        assert!(dep_ids.contains(&t3.id()));
    }

    #[test]
    fn list_dependencies_empty() {
        let (_tmp, conn) = setup();
        let t1 = create_task(&conn, ProjectId(1), &default_create_params("t1")).unwrap();

        let deps = list_dependencies(
            &conn,
            TaskDbId(t1.id().into()),
            &ListTaskDepsFilter::default(),
        )
        .unwrap();
        assert!(deps.items.is_empty());
    }

    #[test]
    fn clear_optional_field_with_none() {
        let (_tmp, conn) = setup();
        let task = create_task(
            &conn,
            ProjectId(1),
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
            TaskDbId(task.id().into()),
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
                contract_id: None,
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
            ProjectId(1),
            &CreateTaskParams {
                definition_of_done: vec!["item1".to_string(), "item2".to_string()],
                ..default_create_params("t")
            },
        )
        .unwrap();

        // Check first item via domain method + save
        let task = get_task(&conn, TaskDbId(task.id().into())).unwrap();
        let (task, _) = task
            .check_dod(1, "2025-01-01T00:00:00Z".to_string())
            .unwrap();
        save_task(&conn, &task).unwrap();
        let updated = get_task(&conn, TaskDbId(task.id().into())).unwrap();
        assert!(updated.definition_of_done()[0].checked());
        assert!(!updated.definition_of_done()[1].checked());

        // Check second item
        let (task, _) = updated
            .check_dod(2, "2025-01-01T00:00:00Z".to_string())
            .unwrap();
        save_task(&conn, &task).unwrap();
        let updated = get_task(&conn, TaskDbId(task.id().into())).unwrap();
        assert!(updated.definition_of_done()[0].checked());
        assert!(updated.definition_of_done()[1].checked());

        // Uncheck first item
        let (task, _) = updated
            .uncheck_dod(1, "2025-01-01T00:00:00Z".to_string())
            .unwrap();
        save_task(&conn, &task).unwrap();
        let updated = get_task(&conn, TaskDbId(task.id().into())).unwrap();
        assert!(!updated.definition_of_done()[0].checked());
        assert!(updated.definition_of_done()[1].checked());
    }

    // --- Migration system tests ---

    #[test]
    fn fresh_db_records_migration_version() {
        let (_tmp, conn) = setup();
        let version = current_schema_version(&conn).unwrap();
        assert_eq!(version, 10);
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
        let conn = open_db(
            tmp.path(),
            Some(db_path.as_path()),
            None,
            &XdgDirs::default(),
        )
        .unwrap();

        // Version should include all migrations
        let version = current_schema_version(&conn).unwrap();
        assert_eq!(version, 10);

        // Legacy columns should have been migrated
        let has_description: bool = conn
            .prepare("SELECT description FROM tasks LIMIT 0")
            .is_ok();
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
            .query_row("SELECT description FROM tasks WHERE id = 1", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(desc, "some details");
    }

    #[test]
    fn migration_idempotent() {
        let tmp = tempfile::tempdir().unwrap();
        let db_path = tmp.path().join("data.db");
        let conn1 = open_db(
            tmp.path(),
            Some(db_path.as_path()),
            None,
            &XdgDirs::default(),
        )
        .unwrap();
        let v1 = current_schema_version(&conn1).unwrap();
        drop(conn1);

        let conn2 = open_db(
            tmp.path(),
            Some(db_path.as_path()),
            None,
            &XdgDirs::default(),
        )
        .unwrap();
        let v2 = current_schema_version(&conn2).unwrap();
        assert_eq!(v1, v2);

        let count: i64 = conn2
            .query_row("SELECT COUNT(*) FROM schema_migrations", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(count, 10);
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
            assignee_user_id: None,
            contract_id: None,
        }
    }

    #[tokio::test]
    async fn inmem_task_round_trip() {
        let backend = mem_backend();
        let task = backend
            .create_task(
                ProjectId(1),
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
                    assignee_user_id: None,
                    contract_id: None,
                },
            )
            .await
            .unwrap();

        let got = backend.get_task(ProjectId(1), task.id()).await.unwrap();
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
            .create_task(ProjectId(1), &params("Lifecycle"))
            .await
            .unwrap();
        assert_eq!(task.status(), TaskStatus::Draft);

        let (task, _) = task.publish("2026-01-01T00:00:00Z".to_string()).unwrap();
        backend.save(&task).await.unwrap();
        let task_got = backend.get_task(ProjectId(1), task.id()).await.unwrap();
        assert_eq!(task_got.status(), TaskStatus::Todo);

        let (task, _) = task_got
            .start(
                Some("sess-1".into()),
                None,
                "2026-01-01T00:00:00Z".to_string(),
                None,
            )
            .unwrap();
        backend.save(&task).await.unwrap();
        let task_got = backend.get_task(ProjectId(1), task.id()).await.unwrap();
        assert_eq!(task_got.status(), TaskStatus::InProgress);
        assert_eq!(task_got.assignee_session_id(), Some("sess-1"));
        assert!(task_got.started_at().is_some());

        let (task, _) = task_got
            .complete("2026-01-02T00:00:00Z".to_string())
            .unwrap();
        backend.save(&task).await.unwrap();
        let task_got = backend.get_task(ProjectId(1), task.id()).await.unwrap();
        assert_eq!(task_got.status(), TaskStatus::Completed);
        assert!(task_got.completed_at().is_some());
    }

    #[tokio::test]
    async fn inmem_task_cancel() {
        let backend = mem_backend();
        let task = backend
            .create_task(ProjectId(1), &params("Cancel me"))
            .await
            .unwrap();
        let (task, _) = task
            .cancel(
                "2026-01-01T00:00:00Z".to_string(),
                Some("no longer needed".into()),
            )
            .unwrap();
        backend.save(&task).await.unwrap();
        let task_got = backend.get_task(ProjectId(1), task.id()).await.unwrap();
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

        let list = backend
            .list_projects(&ListProjectsFilter::default())
            .await
            .unwrap();
        assert!(!list.items.is_empty());

        backend.delete_project(proj.id()).await.unwrap();
        assert!(backend.get_project(proj.id()).await.is_err());
    }

    #[tokio::test]
    async fn inmem_user_crud() {
        let backend = mem_backend();
        let user = backend
            .create_user(&CreateUserParams {
                username: Username("alice".into()),
                sub: None,
                display_name: Some("Alice".into()),
                email: Some("alice@example.com".into()),
            })
            .await
            .unwrap();
        assert_eq!(user.username(), "alice");
        assert_eq!(user.sub(), "alice"); // sub defaults to username

        let got = backend.get_user(user.id()).await.unwrap();
        assert_eq!(got.display_name(), Some("Alice"));
        assert_eq!(got.email(), Some("alice@example.com"));
        assert_eq!(got.sub(), "alice");

        let by_name = backend
            .get_user_by_username(&Username("alice".into()))
            .await
            .unwrap();
        assert_eq!(by_name.id(), user.id());

        let by_sub = backend.get_user_by_sub("alice").await.unwrap();
        assert_eq!(by_sub.id(), user.id());

        let list = backend
            .list_users(&ListUsersFilter::default())
            .await
            .unwrap();
        assert_eq!(list.items.len(), 2); // default user + alice

        backend.delete_user(user.id()).await.unwrap();
        assert!(backend.get_user(user.id()).await.is_err());
    }

    #[tokio::test]
    async fn inmem_project_member_management() {
        let backend = mem_backend();
        let user = backend
            .create_user(&CreateUserParams {
                username: Username("bob".into()),
                sub: None,
                display_name: None,
                email: None,
            })
            .await
            .unwrap();

        let member = backend
            .add_project_member(
                ProjectId(1),
                &AddProjectMemberParams::new(user.id(), Some(Role::Member)),
            )
            .await
            .unwrap();
        assert_eq!(member.role(), Role::Member);

        let got = backend
            .get_project_member(ProjectId(1), user.id())
            .await
            .unwrap();
        assert_eq!(got.user_id(), user.id());

        let updated = backend
            .update_member_role(ProjectId(1), user.id(), Role::Owner)
            .await
            .unwrap();
        assert_eq!(updated.role(), Role::Owner);

        let members = backend
            .list_project_members(ProjectId(1), &ListProjectMembersFilter::default())
            .await
            .unwrap();
        assert_eq!(members.items.len(), 2); // default user (owner) + bob

        backend
            .remove_project_member(ProjectId(1), user.id())
            .await
            .unwrap();
        let members = backend
            .list_project_members(ProjectId(1), &ListProjectMembersFilter::default())
            .await
            .unwrap();
        assert_eq!(members.items.len(), 1); // only default user (owner) remains
    }

    #[tokio::test]
    async fn inmem_dependencies() {
        let backend = mem_backend();
        let t1 = backend
            .create_task(ProjectId(1), &params("T1"))
            .await
            .unwrap();
        let t2 = backend
            .create_task(ProjectId(1), &params("T2"))
            .await
            .unwrap();
        let (t1, _) = t1.publish("2026-01-01T00:00:00Z".to_string()).unwrap();
        backend.save(&t1).await.unwrap();
        let (t2, _) = t2.publish("2026-01-01T00:00:00Z".to_string()).unwrap();
        backend.save(&t2).await.unwrap();

        let (t2, _) = t2
            .add_dependency(t1.id(), Some("2026-01-01T00:00:01Z".into()))
            .unwrap();
        backend.save(&t2).await.unwrap();
        let t2 = backend.get_task(ProjectId(1), t2.id()).await.unwrap();
        assert_eq!(t2.dependencies(), &[t1.id()]);

        let deps = backend
            .list_dependencies(ProjectId(1), t2.id(), &ListTaskDepsFilter::default())
            .await
            .unwrap();
        assert_eq!(deps.items.len(), 1);
        assert_eq!(deps.items[0].id(), t1.id());

        let next = backend.next_task(ProjectId(1), None, false).await.unwrap();
        assert!(next.is_none() || next.unwrap().id() == t1.id());

        let (t2, _) = t2
            .remove_dependency(t1.id(), Some("2026-01-01T00:00:02Z".into()))
            .unwrap();
        backend.save(&t2).await.unwrap();
        let t2 = backend.get_task(ProjectId(1), t2.id()).await.unwrap();
        assert!(t2.dependencies().is_empty());
    }

    #[tokio::test]
    async fn inmem_dod_check_uncheck() {
        let backend = mem_backend();
        let mut p = params("DoD test");
        p.definition_of_done = vec!["Item A".into(), "Item B".into()];
        let task = backend.create_task(ProjectId(1), &p).await.unwrap();
        assert!(!task.definition_of_done()[0].checked());
        assert!(!task.definition_of_done()[1].checked());

        let (task, _) = task
            .check_dod(1, "2026-01-01T00:00:00Z".to_string())
            .unwrap();
        backend.save(&task).await.unwrap();
        let task = backend.get_task(ProjectId(1), task.id()).await.unwrap();
        assert!(task.definition_of_done()[0].checked());
        assert!(!task.definition_of_done()[1].checked());

        let (task, _) = task
            .check_dod(2, "2026-01-01T00:00:00Z".to_string())
            .unwrap();
        backend.save(&task).await.unwrap();
        let task = backend.get_task(ProjectId(1), task.id()).await.unwrap();
        assert!(task.definition_of_done()[0].checked());
        assert!(task.definition_of_done()[1].checked());

        let (task, _) = task
            .uncheck_dod(1, "2026-01-01T00:00:00Z".to_string())
            .unwrap();
        backend.save(&task).await.unwrap();
        let task = backend.get_task(ProjectId(1), task.id()).await.unwrap();
        assert!(!task.definition_of_done()[0].checked());
        assert!(task.definition_of_done()[1].checked());
    }

    #[tokio::test]
    async fn test_sync_config_defaults_project_name() {
        let backend = SqliteBackend::new_in_memory().unwrap();
        let project = backend.get_project(ProjectId(1)).await.unwrap();
        assert_eq!(project.name(), "default");

        let mut config = Config::default();
        config.project.name = Some("my-project".to_string());
        backend.sync_config_defaults(&config).unwrap();

        let project = backend.get_project(ProjectId(1)).await.unwrap();
        assert_eq!(project.name(), "my-project");
    }

    #[tokio::test]
    async fn test_sync_config_defaults_user_name() {
        let backend = SqliteBackend::new_in_memory().unwrap();
        let user = backend.get_user(UserId(1)).await.unwrap();
        assert_eq!(user.username(), "default");

        let mut config = Config::default();
        config.user.name = Some("alice".to_string());
        backend.sync_config_defaults(&config).unwrap();

        let user = backend.get_user(UserId(1)).await.unwrap();
        assert_eq!(user.username(), "alice");
    }

    #[tokio::test]
    async fn test_sync_config_defaults_none_keeps_default() {
        let backend = SqliteBackend::new_in_memory().unwrap();
        let config = Config::default();
        backend.sync_config_defaults(&config).unwrap();

        let project = backend.get_project(ProjectId(1)).await.unwrap();
        assert_eq!(project.name(), "default");
        let user = backend.get_user(UserId(1)).await.unwrap();
        assert_eq!(user.username(), "default");
    }

    #[tokio::test]
    async fn test_sync_config_defaults_unique_conflict() {
        let backend = SqliteBackend::new_in_memory().unwrap();
        // Create a second project with name "taken"
        use crate::domain::project::CreateProjectParams;
        backend
            .create_project(&CreateProjectParams {
                name: "taken".to_string(),
                description: None,
            })
            .await
            .unwrap();

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

        let sql_result = next_task(&conn, ProjectId(1), None, false)
            .unwrap()
            .unwrap();

        let all_tasks = list_tasks(&conn, ProjectId(1), &ListTasksFilter::default())
            .unwrap()
            .items;
        let domain_result = crate::domain::task::select_next(all_tasks, &HashMap::new()).unwrap();

        assert_eq!(sql_result.id(), domain_result.id());
    }

    #[test]
    fn sql_next_task_matches_domain_with_deps() {
        let (_tmp, conn) = setup();

        let dep = create_task(&conn, ProjectId(1), &default_create_params("dep")).unwrap();
        // dep stays draft (not completed) => blocks dependents

        let blocked = create_task(
            &conn,
            ProjectId(1),
            &CreateTaskParams {
                title: "blocked".to_string(),
                dependencies: vec![dep.id()],
                ..default_create_params("blocked")
            },
        )
        .unwrap();
        transition_to(&conn, TaskDbId(blocked.id().into()), TaskStatus::Todo);

        let free = make_todo(&conn, "free", Some(Priority::P1));

        let sql_result = next_task(&conn, ProjectId(1), None, false)
            .unwrap()
            .unwrap();

        let all_tasks = list_tasks(&conn, ProjectId(1), &ListTasksFilter::default())
            .unwrap()
            .items;
        let dep_statuses: HashMap<TaskId, TaskStatus> =
            all_tasks.iter().map(|t| (t.id(), t.status())).collect();
        let todo_tasks: Vec<Task> = all_tasks
            .into_iter()
            .filter(|t| t.status() == TaskStatus::Todo)
            .collect();
        let domain_result = crate::domain::task::select_next(todo_tasks, &dep_statuses).unwrap();

        assert_eq!(sql_result.id(), domain_result.id());
        assert_eq!(sql_result.id(), free.id());
    }

    #[test]
    fn sql_ready_filter_matches_domain_filter_ready() {
        let (_tmp, conn) = setup();

        let dep = create_task(&conn, ProjectId(1), &default_create_params("dep")).unwrap();

        let blocked = create_task(
            &conn,
            ProjectId(1),
            &CreateTaskParams {
                title: "blocked".to_string(),
                dependencies: vec![dep.id()],
                ..default_create_params("blocked")
            },
        )
        .unwrap();
        transition_to(&conn, TaskDbId(blocked.id().into()), TaskStatus::Todo);

        make_todo(&conn, "free1", None);
        make_todo(&conn, "free2", None);

        let sql_ready = list_ready_tasks(&conn, ProjectId(1)).unwrap();

        let all_tasks = list_tasks(&conn, ProjectId(1), &ListTasksFilter::default())
            .unwrap()
            .items;
        let dep_statuses: HashMap<TaskId, TaskStatus> =
            all_tasks.iter().map(|t| (t.id(), t.status())).collect();
        let todo_tasks: Vec<Task> = all_tasks
            .into_iter()
            .filter(|t| t.status() == TaskStatus::Todo)
            .collect();
        let domain_ready = crate::domain::task::filter_ready(todo_tasks, &dep_statuses);

        let mut sql_ids: Vec<TaskId> = sql_ready.iter().map(|t| t.id()).collect();
        let mut domain_ids: Vec<TaskId> = domain_ready.iter().map(|t| t.id()).collect();
        sql_ids.sort();
        domain_ids.sort();
        assert_eq!(sql_ids, domain_ids);
    }

    #[test]
    fn sql_ready_count_matches_domain() {
        let (_tmp, conn) = setup();

        let dep = create_task(&conn, ProjectId(1), &default_create_params("dep")).unwrap();

        let blocked = create_task(
            &conn,
            ProjectId(1),
            &CreateTaskParams {
                title: "blocked".to_string(),
                dependencies: vec![dep.id()],
                ..default_create_params("blocked")
            },
        )
        .unwrap();
        transition_to(&conn, TaskDbId(blocked.id().into()), TaskStatus::Todo);

        make_todo(&conn, "free1", None);
        make_todo(&conn, "free2", None);

        let sql_count = ready_count(&conn, ProjectId(1)).unwrap();

        let all_tasks = list_tasks(&conn, ProjectId(1), &ListTasksFilter::default())
            .unwrap()
            .items;
        let dep_statuses: HashMap<TaskId, TaskStatus> =
            all_tasks.iter().map(|t| (t.id(), t.status())).collect();
        let todo_tasks: Vec<Task> = all_tasks
            .into_iter()
            .filter(|t| t.status() == TaskStatus::Todo)
            .collect();
        let domain_count =
            crate::domain::task::filter_ready(todo_tasks, &dep_statuses).len() as i64;

        assert_eq!(sql_count, domain_count);
    }

    #[test]
    fn is_task_ready_true_for_todo_with_no_deps() {
        let (_tmp, conn) = setup();
        let t = make_todo(&conn, "free", None);
        assert!(is_task_ready(&conn, ProjectId(1), t.id()).unwrap());
    }

    #[test]
    fn is_task_ready_false_for_draft() {
        let (_tmp, conn) = setup();
        let t = create_task(&conn, ProjectId(1), &default_create_params("draft")).unwrap();
        assert!(!is_task_ready(&conn, ProjectId(1), t.id()).unwrap());
    }

    #[test]
    fn is_task_ready_false_for_in_progress() {
        let (_tmp, conn) = setup();
        let t = create_task(&conn, ProjectId(1), &default_create_params("wip")).unwrap();
        transition_to(&conn, TaskDbId(t.id().into()), TaskStatus::InProgress);
        assert!(!is_task_ready(&conn, ProjectId(1), t.id()).unwrap());
    }

    #[test]
    fn is_task_ready_false_for_completed() {
        let (_tmp, conn) = setup();
        let t = make_completed(&conn, "done");
        assert!(!is_task_ready(&conn, ProjectId(1), t.id()).unwrap());
    }

    #[test]
    fn is_task_ready_false_when_blocked_by_incomplete_dep() {
        let (_tmp, conn) = setup();
        let dep = create_task(&conn, ProjectId(1), &default_create_params("dep")).unwrap();
        let blocked = create_task(
            &conn,
            ProjectId(1),
            &CreateTaskParams {
                title: "blocked".to_string(),
                dependencies: vec![dep.id()],
                ..default_create_params("blocked")
            },
        )
        .unwrap();
        transition_to(&conn, TaskDbId(blocked.id().into()), TaskStatus::Todo);
        assert!(!is_task_ready(&conn, ProjectId(1), blocked.id()).unwrap());
    }

    #[test]
    fn is_task_ready_true_when_deps_completed() {
        let (_tmp, conn) = setup();
        let dep = make_completed(&conn, "dep");
        let unblocked = create_task(
            &conn,
            ProjectId(1),
            &CreateTaskParams {
                title: "unblocked".to_string(),
                dependencies: vec![dep.id()],
                ..default_create_params("unblocked")
            },
        )
        .unwrap();
        transition_to(&conn, TaskDbId(unblocked.id().into()), TaskStatus::Todo);
        assert!(is_task_ready(&conn, ProjectId(1), unblocked.id()).unwrap());
    }

    #[test]
    fn is_task_ready_false_for_missing_task() {
        let (_tmp, conn) = setup();
        assert!(!is_task_ready(&conn, ProjectId(1), TaskId(9_999)).unwrap());
    }

    // --- MetadataField tests ---

    #[test]
    fn create_and_get_metadata_field() {
        let (_tmp, conn) = setup();
        let params = CreateMetadataFieldParams {
            name: "sprint".to_string(),
            field_type: MetadataFieldType::String,
            required_on_complete: false,
            description: Some("Sprint name".to_string()),
        };
        let field = create_metadata_field(&conn, ProjectId(1), &params).unwrap();
        assert_eq!(field.name(), "sprint");
        assert_eq!(field.field_type(), MetadataFieldType::String);
        assert!(!field.required_on_complete());
        assert_eq!(field.description(), Some("Sprint name"));
        assert_eq!(field.project_id(), ProjectId(1));

        let fetched = get_metadata_field(&conn, ProjectId(1), field.id()).unwrap();
        assert_eq!(fetched.id(), field.id());
        assert_eq!(fetched.name(), "sprint");
    }

    #[test]
    fn list_metadata_fields_by_project() {
        let (_tmp, conn) = setup();
        create_metadata_field(
            &conn,
            ProjectId(1),
            &CreateMetadataFieldParams {
                name: "sprint".to_string(),
                field_type: MetadataFieldType::String,
                required_on_complete: false,
                description: None,
            },
        )
        .unwrap();
        create_metadata_field(
            &conn,
            ProjectId(1),
            &CreateMetadataFieldParams {
                name: "points".to_string(),
                field_type: MetadataFieldType::Number,
                required_on_complete: true,
                description: None,
            },
        )
        .unwrap();

        let fields =
            list_metadata_fields(&conn, ProjectId(1), &ListMetadataFieldsFilter::default())
                .unwrap();
        assert_eq!(fields.items.len(), 2);

        // Different project should be empty
        create_project(
            &conn,
            &CreateProjectParams {
                name: "other".to_string(),
                description: None,
            },
        )
        .unwrap();
        let other_fields =
            list_metadata_fields(&conn, ProjectId(2), &ListMetadataFieldsFilter::default())
                .unwrap();
        assert!(other_fields.items.is_empty());
    }

    #[test]
    fn update_metadata_field_required_only() {
        let (_tmp, conn) = setup();
        let field = create_metadata_field(
            &conn,
            ProjectId(1),
            &CreateMetadataFieldParams {
                name: "sprint".to_string(),
                field_type: MetadataFieldType::String,
                required_on_complete: false,
                description: Some("original".to_string()),
            },
        )
        .unwrap();

        let updated = update_metadata_field(
            &conn,
            ProjectId(1),
            field.id(),
            &UpdateMetadataFieldParams {
                required_on_complete: Some(true),
                description: None,
            },
        )
        .unwrap();
        assert!(updated.required_on_complete());
        assert_eq!(updated.description(), Some("original"));
    }

    #[test]
    fn update_metadata_field_clear_description() {
        let (_tmp, conn) = setup();
        let field = create_metadata_field(
            &conn,
            ProjectId(1),
            &CreateMetadataFieldParams {
                name: "sprint".to_string(),
                field_type: MetadataFieldType::String,
                required_on_complete: false,
                description: Some("has desc".to_string()),
            },
        )
        .unwrap();

        let updated = update_metadata_field(
            &conn,
            ProjectId(1),
            field.id(),
            &UpdateMetadataFieldParams {
                required_on_complete: None,
                description: Some(None),
            },
        )
        .unwrap();
        assert_eq!(updated.description(), None);
    }

    #[test]
    fn update_metadata_field_set_description() {
        let (_tmp, conn) = setup();
        let field = create_metadata_field(
            &conn,
            ProjectId(1),
            &CreateMetadataFieldParams {
                name: "sprint".to_string(),
                field_type: MetadataFieldType::String,
                required_on_complete: false,
                description: None,
            },
        )
        .unwrap();

        let updated = update_metadata_field(
            &conn,
            ProjectId(1),
            field.id(),
            &UpdateMetadataFieldParams {
                required_on_complete: None,
                description: Some(Some("new desc".to_string())),
            },
        )
        .unwrap();
        assert_eq!(updated.description(), Some("new desc"));
    }

    #[test]
    fn delete_metadata_field_success() {
        let (_tmp, conn) = setup();
        let field = create_metadata_field(
            &conn,
            ProjectId(1),
            &CreateMetadataFieldParams {
                name: "sprint".to_string(),
                field_type: MetadataFieldType::String,
                required_on_complete: false,
                description: None,
            },
        )
        .unwrap();

        delete_metadata_field(&conn, ProjectId(1), field.id()).unwrap();
        let result = get_metadata_field(&conn, ProjectId(1), field.id());
        assert!(result.is_err());
    }

    #[test]
    fn delete_metadata_field_not_found() {
        let (_tmp, conn) = setup();
        let result = delete_metadata_field(&conn, ProjectId(1), 999);
        assert!(result.is_err());
    }

    #[test]
    fn create_metadata_field_name_conflict() {
        let (_tmp, conn) = setup();
        let params = CreateMetadataFieldParams {
            name: "sprint".to_string(),
            field_type: MetadataFieldType::String,
            required_on_complete: false,
            description: None,
        };
        create_metadata_field(&conn, ProjectId(1), &params).unwrap();
        let result = create_metadata_field(&conn, ProjectId(1), &params);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.downcast_ref::<DomainError>()
                .is_some_and(|e| matches!(e, DomainError::MetadataFieldNameConflict { .. }))
        );
    }

    #[test]
    fn create_metadata_field_same_name_different_project() {
        let (_tmp, conn) = setup();
        create_project(
            &conn,
            &CreateProjectParams {
                name: "other".to_string(),
                description: None,
            },
        )
        .unwrap();
        let params = CreateMetadataFieldParams {
            name: "sprint".to_string(),
            field_type: MetadataFieldType::String,
            required_on_complete: false,
            description: None,
        };
        create_metadata_field(&conn, ProjectId(1), &params).unwrap();
        let result = create_metadata_field(&conn, ProjectId(2), &params);
        assert!(result.is_ok());
    }

    #[test]
    fn get_metadata_field_wrong_project() {
        let (_tmp, conn) = setup();
        let field = create_metadata_field(
            &conn,
            ProjectId(1),
            &CreateMetadataFieldParams {
                name: "sprint".to_string(),
                field_type: MetadataFieldType::String,
                required_on_complete: false,
                description: None,
            },
        )
        .unwrap();

        create_project(
            &conn,
            &CreateProjectParams {
                name: "other".to_string(),
                description: None,
            },
        )
        .unwrap();
        let result = get_metadata_field(&conn, ProjectId(2), field.id());
        assert!(result.is_err());
    }

    #[test]
    fn metadata_fields_table_exists() {
        let (_tmp, conn) = setup();
        let tables: Vec<String> = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        assert!(tables.contains(&"metadata_fields".to_string()));
    }

    #[test]
    fn test_update_user() {
        let (_tmp, conn) = setup();

        let user = create_user(
            &conn,
            &CreateUserParams {
                username: Username("alice".to_string()),
                sub: None,
                display_name: Some("Alice".to_string()),
                email: Some("alice@example.com".to_string()),
            },
        )
        .unwrap();
        assert_eq!(user.username(), "alice");
        assert_eq!(user.display_name(), Some("Alice"));

        // Update username only
        let updated = update_user(
            &conn,
            user.id(),
            &UpdateUserParams {
                username: Some(Username("alice2".to_string())),
                display_name: None,
            },
        )
        .unwrap();
        assert_eq!(updated.username(), "alice2");
        assert_eq!(updated.display_name(), Some("Alice"));

        // Update display_name only
        let updated = update_user(
            &conn,
            user.id(),
            &UpdateUserParams {
                username: None,
                display_name: Some(Some("Alice Updated".to_string())),
            },
        )
        .unwrap();
        assert_eq!(updated.username(), "alice2");
        assert_eq!(updated.display_name(), Some("Alice Updated"));

        // Clear display_name
        let updated = update_user(
            &conn,
            user.id(),
            &UpdateUserParams {
                username: None,
                display_name: Some(None),
            },
        )
        .unwrap();
        assert_eq!(updated.username(), "alice2");
        assert_eq!(updated.display_name(), None);

        // Update non-existent user
        let err = update_user(
            &conn,
            UserId(9999),
            &UpdateUserParams {
                username: Some(Username("ghost".to_string())),
                display_name: None,
            },
        );
        assert!(err.is_err());
    }

    // ---------------------------------------------------------------
    // Contract tests
    // ---------------------------------------------------------------

    fn make_contract_params(title: &str) -> CreateContractParams {
        CreateContractParams {
            title: title.to_string(),
            description: Some("spec".to_string()),
            definition_of_done: vec!["item1".to_string(), "item2".to_string()],
            tags: vec!["api".to_string()],
            metadata: Some(serde_json::json!({"owner": "team-a"})),
        }
    }

    #[tokio::test]
    async fn inmem_contract_create_and_get() {
        let backend = mem_backend();
        let created = backend
            .create_contract(ProjectId(1), &make_contract_params("Contract A"))
            .await
            .unwrap();
        assert_eq!(created.title(), "Contract A");
        assert_eq!(created.project_id(), ProjectId(1));
        assert_eq!(created.definition_of_done().len(), 2);
        assert_eq!(created.tags(), &["api".to_string()]);
        assert_eq!(
            created.metadata(),
            Some(&serde_json::json!({"owner": "team-a"}))
        );

        let got = backend.get_contract(created.id()).await.unwrap();
        assert_eq!(got.id(), created.id());
        assert_eq!(got.title(), "Contract A");
        assert_eq!(got.definition_of_done()[0].content(), "item1");
        assert!(!got.definition_of_done()[0].checked());
    }

    #[tokio::test]
    async fn inmem_contract_list_ordered() {
        let backend = mem_backend();
        let a = backend
            .create_contract(ProjectId(1), &make_contract_params("A"))
            .await
            .unwrap();
        let b = backend
            .create_contract(ProjectId(1), &make_contract_params("B"))
            .await
            .unwrap();

        let list = backend
            .list_contracts(ProjectId(1), &ListContractsFilter::default())
            .await
            .unwrap();
        assert_eq!(list.items.len(), 2);
        assert_eq!(list.items[0].id(), a.id());
        assert_eq!(list.items[1].id(), b.id());
    }

    #[tokio::test]
    async fn inmem_contract_update_scalar_and_arrays() {
        let backend = mem_backend();
        let c = backend
            .create_contract(ProjectId(1), &make_contract_params("Spec"))
            .await
            .unwrap();

        let updated = backend
            .update_contract(
                c.id(),
                &UpdateContractParams {
                    title: Some("Spec v2".to_string()),
                    description: Some(None),
                    metadata: Some(MetadataUpdate::Merge(
                        serde_json::json!({"stage": "review"}),
                    )),
                },
                &UpdateContractArrayParams {
                    add_tags: vec!["backend".to_string()],
                    set_definition_of_done: Some(vec!["done-a".to_string()]),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(updated.title(), "Spec v2");
        assert_eq!(updated.description(), None);
        assert_eq!(
            updated.metadata(),
            Some(&serde_json::json!({"owner": "team-a", "stage": "review"}))
        );
        assert!(updated.tags().contains(&"backend".to_string()));
        assert_eq!(updated.definition_of_done().len(), 1);
        assert_eq!(updated.definition_of_done()[0].content(), "done-a");
        assert!(!updated.definition_of_done()[0].checked());
    }

    #[tokio::test]
    async fn inmem_contract_check_and_uncheck_dod() {
        let backend = mem_backend();
        let c = backend
            .create_contract(ProjectId(1), &make_contract_params("DoD"))
            .await
            .unwrap();
        let checked = backend.check_dod(c.id(), 1).await.unwrap();
        assert!(checked.definition_of_done()[0].checked());
        assert!(!checked.definition_of_done()[1].checked());

        let unchecked = backend.uncheck_dod(c.id(), 1).await.unwrap();
        assert!(!unchecked.definition_of_done()[0].checked());

        // Out-of-range returns an error
        assert!(backend.check_dod(c.id(), 0).await.is_err());
        assert!(backend.check_dod(c.id(), 99).await.is_err());
    }

    #[tokio::test]
    async fn inmem_contract_add_note_preserves_source_task() {
        let backend = mem_backend();
        let c = backend
            .create_contract(ProjectId(1), &make_contract_params("With note"))
            .await
            .unwrap();
        let task = backend
            .create_task(ProjectId(1), &params("source"))
            .await
            .unwrap();
        let note = ContractNote::new(
            "first observation".to_string(),
            Some(task.id()),
            "2026-04-17T00:00:00Z".to_string(),
        );
        backend.add_note(c.id(), &note).await.unwrap();

        let refreshed = backend.get_contract(c.id()).await.unwrap();
        assert_eq!(refreshed.notes().len(), 1);
        assert_eq!(refreshed.notes()[0].content(), "first observation");
        assert_eq!(refreshed.notes()[0].source_task_id(), Some(task.id()));

        // ON DELETE SET NULL: deleting the source task nullifies the reference
        backend.delete_task(ProjectId(1), task.id()).await.unwrap();
        let refreshed = backend.get_contract(c.id()).await.unwrap();
        assert_eq!(refreshed.notes().len(), 1);
        assert_eq!(refreshed.notes()[0].source_task_id(), None);
    }

    #[tokio::test]
    async fn inmem_contract_delete_cascades_children() {
        let backend = mem_backend();
        let c = backend
            .create_contract(ProjectId(1), &make_contract_params("Delete me"))
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

        // Child rows must be gone (SQLite FK cascade)
        assert!(backend.get_contract(c.id()).await.is_err());
        let list = backend
            .list_contracts(ProjectId(1), &ListContractsFilter::default())
            .await
            .unwrap();
        assert!(list.items.is_empty());
    }

    #[tokio::test]
    async fn inmem_create_task_with_contract_id() {
        let backend = mem_backend();
        let c = backend
            .create_contract(ProjectId(1), &make_contract_params("linked at create"))
            .await
            .unwrap();
        let mut p = params("task with contract");
        p.contract_id = Some(c.id());
        let task = backend.create_task(ProjectId(1), &p).await.unwrap();
        assert_eq!(task.contract_id(), Some(c.id()));

        let got = backend.get_task(ProjectId(1), task.id()).await.unwrap();
        assert_eq!(got.contract_id(), Some(c.id()));
    }

    #[tokio::test]
    async fn inmem_task_contract_id_roundtrip() {
        let backend = mem_backend();
        let c = backend
            .create_contract(ProjectId(1), &make_contract_params("linked"))
            .await
            .unwrap();
        let task = backend
            .create_task(ProjectId(1), &params("linked task"))
            .await
            .unwrap();

        // Initially NULL
        assert_eq!(task.contract_id(), None);

        let updated = backend
            .update_task(
                ProjectId(1),
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

        // Clearing back to None
        let cleared = backend
            .update_task(
                ProjectId(1),
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
                    contract_id: Some(None),
                    metadata: None,
                },
            )
            .await
            .unwrap();
        assert_eq!(cleared.contract_id(), None);
    }
}
