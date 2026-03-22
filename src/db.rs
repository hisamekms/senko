use std::collections::{HashMap, HashSet, VecDeque};
use std::path::Path;

use anyhow::{Context, Result};
use rusqlite::{params, Connection, OptionalExtension};

use crate::models::{
    CreateTaskParams, DodItem, ListTasksFilter, Priority, Task, TaskStatus, UpdateTaskArrayParams,
    UpdateTaskParams,
};

pub fn open_db(project_root: &Path) -> Result<Connection> {
    let localflow_dir = project_root.join(".localflow");
    std::fs::create_dir_all(&localflow_dir)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&localflow_dir, std::fs::Permissions::from_mode(0o700))?;
    }

    let db_path = localflow_dir.join("data.db");
    let conn = Connection::open(&db_path)?;

    conn.execute_batch("PRAGMA journal_mode=WAL;")?;
    conn.execute_batch("PRAGMA foreign_keys=ON;")?;

    create_schema(&conn)?;
    migrate_dod_checked(&conn)?;
    migrate(&conn)?;

    warn_if_not_gitignored(project_root);

    Ok(conn)
}

fn warn_if_not_gitignored(project_root: &Path) {
    let gitignore_path = project_root.join(".gitignore");
    let dominated = match std::fs::read_to_string(&gitignore_path) {
        Ok(content) => content
            .lines()
            .any(|line| {
                let trimmed = line.trim();
                trimmed == ".localflow" || trimmed == ".localflow/"
            }),
        Err(_) => false,
    };
    if !dominated {
        eprintln!(
            "warning: .localflow/ is not in .gitignore. \
             Add \".localflow/\" to your .gitignore to avoid committing local data."
        );
    }
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

fn create_schema(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "
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
            branch TEXT
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
    )?;
    Ok(())
}

fn migrate(conn: &Connection) -> Result<()> {
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

    Ok(())
}

pub fn create_task(conn: &Connection, params: &CreateTaskParams) -> Result<Task> {
    let priority: i32 = params.priority.unwrap_or(Priority::P2).into();
    conn.execute(
        "INSERT INTO tasks (title, background, description, priority, branch) VALUES (?1, ?2, ?3, ?4, ?5)",
        rusqlite::params![params.title, params.background, params.description, priority, params.branch],
    )?;
    let task_id = conn.last_insert_rowid();

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

pub fn get_task(conn: &Connection, id: i64) -> Result<Task> {
    let (title, background, description, plan, status_str, priority_val, assignee_session_id, created_at, updated_at, started_at, completed_at, canceled_at, cancel_reason, branch): (
        String, Option<String>, Option<String>, Option<String>, String, i32, Option<String>, String, String, Option<String>, Option<String>, Option<String>, Option<String>, Option<String>,
    ) = conn
        .query_row(
            "SELECT title, background, description, plan, status, priority, assignee_session_id, created_at, updated_at, started_at, completed_at, canceled_at, cancel_reason, branch FROM tasks WHERE id = ?1",
            params![id],
            |row| {
                Ok((
                    row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?,
                    row.get(4)?, row.get(5)?, row.get(6)?, row.get(7)?,
                    row.get(8)?, row.get(9)?, row.get(10)?, row.get(11)?,
                    row.get(12)?, row.get(13)?,
                ))
            },
        )
        .context("task not found")?;

    let status: TaskStatus = status_str.parse()?;
    let priority = Priority::try_from(priority_val)?;

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

    Ok(Task {
        id,
        title,
        background,
        description,
        plan,
        priority,
        status,
        assignee_session_id,
        created_at,
        updated_at,
        started_at,
        completed_at,
        canceled_at,
        cancel_reason,
        branch,
        definition_of_done,
        in_scope,
        out_of_scope,
        tags,
        dependencies,
    })
}

pub fn update_task(conn: &Connection, id: i64, params: &UpdateTaskParams) -> Result<Task> {
    // Validate status transition if status change requested
    if let Some(new_status) = params.status {
        let current: String = conn.query_row(
            "SELECT status FROM tasks WHERE id = ?1",
            rusqlite::params![id],
            |row| row.get(0),
        ).context("task not found")?;
        let current_status: TaskStatus = current.parse()?;
        current_status.transition_to(new_status)?;
    }

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
    if let Some(status) = params.status {
        columns.push(TaskColumn::Status);
        values.push(Box::new(status.to_string()));
    }
    if let Some(ref assignee) = params.assignee_session_id {
        columns.push(TaskColumn::AssigneeSessionId);
        values.push(Box::new(assignee.clone()));
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

    if !columns.is_empty() {
        let set_clause: Vec<String> = columns.iter().map(|c| format!("{} = ?", c.as_str())).collect();
        let sql = format!("UPDATE tasks SET {}, updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now') WHERE id = ?", set_clause.join(", "));
        values.push(Box::new(id));
        let param_refs: Vec<&dyn rusqlite::types::ToSql> = values.iter().map(|v| v.as_ref()).collect();
        conn.execute(&sql, param_refs.as_slice())?;
    }

    get_task(conn, id)
}

pub fn update_task_arrays(conn: &Connection, id: i64, params: &UpdateTaskArrayParams) -> Result<()> {
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

enum TaskColumn {
    Title,
    Background,
    Description,
    Plan,
    Priority,
    Status,
    AssigneeSessionId,
    StartedAt,
    CompletedAt,
    CanceledAt,
    CancelReason,
    Branch,
}

impl TaskColumn {
    fn as_str(&self) -> &'static str {
        match self {
            TaskColumn::Title => "title",
            TaskColumn::Background => "background",
            TaskColumn::Description => "description",
            TaskColumn::Plan => "plan",
            TaskColumn::Priority => "priority",
            TaskColumn::Status => "status",
            TaskColumn::AssigneeSessionId => "assignee_session_id",
            TaskColumn::StartedAt => "started_at",
            TaskColumn::CompletedAt => "completed_at",
            TaskColumn::CanceledAt => "canceled_at",
            TaskColumn::CancelReason => "cancel_reason",
            TaskColumn::Branch => "branch",
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

pub fn delete_task(conn: &Connection, id: i64) -> Result<()> {
    let affected = conn.execute("DELETE FROM tasks WHERE id = ?1", params![id])?;
    if affected == 0 {
        anyhow::bail!("task not found: {id}");
    }
    Ok(())
}

pub fn list_tasks(conn: &Connection, filter: &ListTasksFilter) -> Result<Vec<Task>> {
    let mut conditions = Vec::new();
    let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

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

pub fn next_task(conn: &Connection) -> Result<Option<Task>> {
    let sql = "
        SELECT t.id FROM tasks t
        WHERE t.status = 'todo'
          AND NOT EXISTS (
            SELECT 1 FROM task_dependencies td
            JOIN tasks dep ON dep.id = td.depends_on_task_id
            WHERE td.task_id = t.id AND dep.status != 'completed'
          )
        ORDER BY t.priority ASC, t.created_at ASC, t.id ASC
        LIMIT 1
    ";
    let id: Option<i64> = conn
        .query_row(sql, [], |row| row.get(0))
        .optional()?;
    match id {
        Some(id) => Ok(Some(get_task(conn, id)?)),
        None => Ok(None),
    }
}

pub fn task_stats(conn: &Connection) -> Result<HashMap<String, i64>> {
    let mut stmt = conn.prepare("SELECT status, COUNT(*) FROM tasks GROUP BY status")?;
    let rows = stmt.query_map([], |row| {
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

pub fn ready_count(conn: &Connection) -> Result<i64> {
    let sql = "
        SELECT COUNT(*) FROM tasks t
        WHERE t.status = 'todo'
          AND NOT EXISTS (
            SELECT 1 FROM task_dependencies td
            JOIN tasks dep ON dep.id = td.depends_on_task_id
            WHERE td.task_id = t.id AND dep.status != 'completed'
          )
    ";
    let count: i64 = conn.query_row(sql, [], |row| row.get(0))?;
    Ok(count)
}

pub fn list_ready_tasks(conn: &Connection) -> Result<Vec<Task>> {
    let filter = ListTasksFilter {
        ready: true,
        ..Default::default()
    };
    list_tasks(conn, &filter)
}

/// Check if adding dep_id as a dependency of task_id would create a cycle.
/// Performs BFS from dep_id following its dependencies; if task_id is reachable, it's a cycle.
fn has_cycle(conn: &Connection, task_id: i64, dep_id: i64) -> Result<bool> {
    let mut visited = HashSet::new();
    let mut queue = VecDeque::new();
    queue.push_back(dep_id);
    visited.insert(dep_id);

    while let Some(current) = queue.pop_front() {
        let deps = query_i64_list(
            conn,
            "SELECT depends_on_task_id FROM task_dependencies WHERE task_id = ?1",
            current,
        )?;
        for d in deps {
            if d == task_id {
                return Ok(true);
            }
            if visited.insert(d) {
                queue.push_back(d);
            }
        }
    }
    Ok(false)
}

pub fn add_dependency(conn: &Connection, task_id: i64, dep_id: i64) -> Result<Task> {
    if task_id == dep_id {
        anyhow::bail!("a task cannot depend on itself");
    }
    // Verify both tasks exist
    get_task(conn, task_id).context("task not found")?;
    get_task(conn, dep_id).context("dependency task not found")?;
    // Cycle check
    if has_cycle(conn, task_id, dep_id)? {
        anyhow::bail!("adding this dependency would create a cycle");
    }
    conn.execute(
        "INSERT OR IGNORE INTO task_dependencies (task_id, depends_on_task_id) VALUES (?1, ?2)",
        params![task_id, dep_id],
    )?;
    conn.execute(
        "UPDATE tasks SET updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now') WHERE id = ?1",
        params![task_id],
    )?;
    get_task(conn, task_id)
}

pub fn remove_dependency(conn: &Connection, task_id: i64, dep_id: i64) -> Result<Task> {
    get_task(conn, task_id).context("task not found")?;
    let affected = conn.execute(
        "DELETE FROM task_dependencies WHERE task_id = ?1 AND depends_on_task_id = ?2",
        params![task_id, dep_id],
    )?;
    if affected == 0 {
        anyhow::bail!("dependency not found: task {} does not depend on {}", task_id, dep_id);
    }
    conn.execute(
        "UPDATE tasks SET updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now') WHERE id = ?1",
        params![task_id],
    )?;
    get_task(conn, task_id)
}

pub fn set_dependencies(conn: &Connection, task_id: i64, dep_ids: &[i64]) -> Result<Task> {
    // Self-dependency check
    for &dep_id in dep_ids {
        if dep_id == task_id {
            anyhow::bail!("a task cannot depend on itself");
        }
    }
    // Verify task exists
    get_task(conn, task_id).context("task not found")?;
    // Verify all deps exist
    for &dep_id in dep_ids {
        get_task(conn, dep_id).with_context(|| format!("dependency task not found: {}", dep_id))?;
    }

    conn.execute_batch("BEGIN")?;

    // Delete all existing dependencies
    if let Err(e) = (|| -> Result<()> {
        conn.execute(
            "DELETE FROM task_dependencies WHERE task_id = ?1",
            params![task_id],
        )?;
        // Insert each new dependency with cycle check
        for &dep_id in dep_ids {
            if has_cycle(conn, task_id, dep_id)? {
                anyhow::bail!("adding dependency on {} would create a cycle", dep_id);
            }
            conn.execute(
                "INSERT INTO task_dependencies (task_id, depends_on_task_id) VALUES (?1, ?2)",
                params![task_id, dep_id],
            )?;
        }
        Ok(())
    })() {
        conn.execute_batch("ROLLBACK")?;
        return Err(e);
    }

    conn.execute(
        "UPDATE tasks SET updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now') WHERE id = ?1",
        params![task_id],
    )?;
    conn.execute_batch("COMMIT")?;
    get_task(conn, task_id)
}

pub fn list_dependencies(conn: &Connection, task_id: i64) -> Result<Vec<Task>> {
    get_task(conn, task_id).context("task not found")?;
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
            Ok(DodItem {
                content: row.get(0)?,
                checked: row.get::<_, i32>(1)? != 0,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(items)
}

pub fn check_dod(conn: &Connection, task_id: i64, index: usize) -> Result<Task> {
    set_dod_checked(conn, task_id, index, true)
}

pub fn uncheck_dod(conn: &Connection, task_id: i64, index: usize) -> Result<Task> {
    set_dod_checked(conn, task_id, index, false)
}

fn set_dod_checked(conn: &Connection, task_id: i64, index: usize, checked: bool) -> Result<Task> {
    // Verify task exists
    get_task(conn, task_id).context("task not found")?;

    let mut stmt = conn.prepare(
        "SELECT id FROM task_definition_of_done WHERE task_id = ?1 ORDER BY id",
    )?;
    let ids: Vec<i64> = stmt
        .query_map(params![task_id], |row| row.get(0))?
        .collect::<std::result::Result<Vec<_>, _>>()?;

    if index == 0 || index > ids.len() {
        anyhow::bail!(
            "DoD index {} out of range (task #{} has {} DoD item(s))",
            index,
            task_id,
            ids.len()
        );
    }

    let row_id = ids[index - 1];
    let checked_val: i32 = if checked { 1 } else { 0 };
    conn.execute(
        "UPDATE task_definition_of_done SET checked = ?1 WHERE id = ?2",
        params![checked_val, row_id],
    )?;
    conn.execute(
        "UPDATE tasks SET updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now') WHERE id = ?1",
        params![task_id],
    )?;

    get_task(conn, task_id)
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

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> (tempfile::TempDir, Connection) {
        let tmp = tempfile::tempdir().unwrap();
        let conn = open_db(tmp.path()).unwrap();
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
            tags: vec![],
            dependencies: vec![],
        }
    }

    #[test]
    fn creates_db_and_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let conn = open_db(tmp.path()).unwrap();
        assert!(tmp.path().join(".localflow/data.db").exists());
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
    fn idempotent_open() {
        let tmp = tempfile::tempdir().unwrap();
        let _conn1 = open_db(tmp.path()).unwrap();
        drop(_conn1);
        let _conn2 = open_db(tmp.path()).unwrap();
    }

    #[test]
    fn create_and_get_task() {
        let (_tmp, conn) = setup();
        let task = create_task(
            &conn,
            &CreateTaskParams {
                title: "Test task".to_string(),
                background: Some("bg".to_string()),
                description: Some("det".to_string()),
                priority: Some(Priority::P1),
                definition_of_done: vec!["done1".to_string(), "done2".to_string()],
                in_scope: vec!["scope1".to_string()],
                out_of_scope: vec!["out1".to_string()],
                branch: None,
                tags: vec!["rust".to_string(), "cli".to_string()],
                dependencies: vec![],
            },
        )
        .unwrap();

        assert_eq!(task.title, "Test task");
        assert_eq!(task.background.as_deref(), Some("bg"));
        assert_eq!(task.description.as_deref(), Some("det"));
        assert_eq!(task.priority, Priority::P1);
        assert_eq!(task.status, TaskStatus::Draft);
        assert_eq!(
            task.definition_of_done,
            vec![
                DodItem { content: "done1".to_string(), checked: false },
                DodItem { content: "done2".to_string(), checked: false },
            ]
        );
        assert_eq!(task.in_scope, vec!["scope1"]);
        assert_eq!(task.out_of_scope, vec!["out1"]);
        assert_eq!(task.tags.len(), 2);
        assert!(task.tags.contains(&"rust".to_string()));
        assert!(task.tags.contains(&"cli".to_string()));
        assert!(task.dependencies.is_empty());
        assert!(task.assignee_session_id.is_none());
        assert!(task.started_at.is_none());
        assert!(task.canceled_at.is_none());
        assert!(task.cancel_reason.is_none());

        let fetched = get_task(&conn, task.id).unwrap();
        assert_eq!(fetched.title, task.title);
        assert_eq!(fetched.tags, task.tags);
    }

    #[test]
    fn create_task_default_priority() {
        let (_tmp, conn) = setup();
        let task = create_task(&conn, &default_create_params("default prio")).unwrap();
        assert_eq!(task.priority, Priority::P2);
    }

    #[test]
    fn update_task_fields() {
        let (_tmp, conn) = setup();
        let task = create_task(&conn, &default_create_params("original")).unwrap();

        let updated = update_task(
            &conn,
            task.id,
            &UpdateTaskParams {
                title: Some("updated".to_string()),
                background: Some(Some("new bg".to_string())),
                description: Some(Some("new description".to_string())),
                plan: None,
                priority: Some(Priority::P0),
                status: Some(TaskStatus::Todo),
                assignee_session_id: Some(Some("session-1".to_string())),
                started_at: None,
                completed_at: None,
                canceled_at: None,
                cancel_reason: None,
                branch: None,
            },
        )
        .unwrap();

        assert_eq!(updated.title, "updated");
        assert_eq!(updated.background.as_deref(), Some("new bg"));
        assert_eq!(updated.description.as_deref(), Some("new description"));
        assert_eq!(updated.priority, Priority::P0);
        assert_eq!(updated.status, TaskStatus::Todo);
        assert_eq!(updated.assignee_session_id.as_deref(), Some("session-1"));
        assert!(updated.updated_at >= task.updated_at);
    }

    #[test]
    fn update_task_status_transition_validated() {
        let (_tmp, conn) = setup();
        let task = create_task(&conn, &default_create_params("t")).unwrap();
        assert_eq!(task.status, TaskStatus::Draft);

        // draft -> in_progress should fail
        let result = update_task(
            &conn,
            task.id,
            &UpdateTaskParams {
                title: None,
                background: None,
                description: None,
                plan: None,
                priority: None,
                status: Some(TaskStatus::InProgress),
                assignee_session_id: None,
                started_at: None,
                completed_at: None,
                canceled_at: None,
                cancel_reason: None,
                branch: None,
            },
        );
        assert!(result.is_err());

        // draft -> todo should succeed
        let updated = update_task(
            &conn,
            task.id,
            &UpdateTaskParams {
                title: None,
                background: None,
                description: None,
                plan: None,
                priority: None,
                status: Some(TaskStatus::Todo),
                assignee_session_id: None,
                started_at: None,
                completed_at: None,
                canceled_at: None,
                cancel_reason: None,
                branch: None,
            },
        )
        .unwrap();
        assert_eq!(updated.status, TaskStatus::Todo);
    }

    #[test]
    fn delete_task_cascade() {
        let (_tmp, conn) = setup();
        let task = create_task(
            &conn,
            &CreateTaskParams {
                title: "to delete".to_string(),
                background: None,
                description: None,
                priority: None,
                definition_of_done: vec!["d".to_string()],
                in_scope: vec!["s".to_string()],
                out_of_scope: vec!["o".to_string()],
                branch: None,
                tags: vec!["tag".to_string()],
                dependencies: vec![],
            },
        )
        .unwrap();

        delete_task(&conn, task.id).unwrap();

        assert!(get_task(&conn, task.id).is_err());

        let count: i32 = conn
            .query_row(
                "SELECT COUNT(*) FROM task_tags WHERE task_id = ?1",
                params![task.id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 0);

        let count: i32 = conn
            .query_row(
                "SELECT COUNT(*) FROM task_definition_of_done WHERE task_id = ?1",
                params![task.id],
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
        create_task(&conn, &default_create_params("a")).unwrap();
        create_task(&conn, &default_create_params("b")).unwrap();

        let tasks = list_tasks(&conn, &ListTasksFilter::default()).unwrap();
        assert_eq!(tasks.len(), 2);
    }

    #[test]
    fn list_tasks_filter_by_status() {
        let (_tmp, conn) = setup();
        let t1 = create_task(&conn, &default_create_params("draft")).unwrap();
        let _t2 = create_task(&conn, &default_create_params("todo")).unwrap();

        // Move t1 to todo
        update_task(
            &conn,
            t1.id,
            &UpdateTaskParams {
                title: None,
                background: None,
                description: None,
                plan: None,
                priority: None,
                status: Some(TaskStatus::Todo),
                assignee_session_id: None,
                started_at: None,
                completed_at: None,
                canceled_at: None,
                cancel_reason: None,
                branch: None,
            },
        )
        .unwrap();

        let drafts = list_tasks(
            &conn,
            &ListTasksFilter {
                statuses: vec![TaskStatus::Draft],
                tags: vec![],
                depends_on: None,
                ready: false,
            },
        )
        .unwrap();
        assert_eq!(drafts.len(), 1);
        assert_eq!(drafts[0].title, "todo");

        let todos = list_tasks(
            &conn,
            &ListTasksFilter {
                statuses: vec![TaskStatus::Todo],
                tags: vec![],
                depends_on: None,
                ready: false,
            },
        )
        .unwrap();
        assert_eq!(todos.len(), 1);
        assert_eq!(todos[0].title, "draft");
    }

    #[test]
    fn list_tasks_filter_by_tag() {
        let (_tmp, conn) = setup();
        create_task(
            &conn,
            &CreateTaskParams {
                title: "tagged".to_string(),
                branch: None,
                tags: vec!["rust".to_string()],
                ..default_create_params("tagged")
            },
        )
        .unwrap();
        create_task(&conn, &default_create_params("untagged")).unwrap();

        let result = list_tasks(
            &conn,
            &ListTasksFilter {
                statuses: vec![],
                tags: vec!["rust".to_string()],
                depends_on: None,
                ready: false,
            },
        )
        .unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].title, "tagged");
    }

    #[test]
    fn list_tasks_ready_filter() {
        let (_tmp, conn) = setup();

        // Create dep task and move to completed
        let dep = create_task(&conn, &default_create_params("dep")).unwrap();
        update_task(
            &conn,
            dep.id,
            &UpdateTaskParams {
                title: None, background: None, description: None, plan: None, priority: None,
                status: Some(TaskStatus::Todo),
                assignee_session_id: None, started_at: None, completed_at: None,
                canceled_at: None, cancel_reason: None,
                branch: None,
            },
        ).unwrap();
        update_task(
            &conn,
            dep.id,
            &UpdateTaskParams {
                title: None, background: None, description: None, plan: None, priority: None,
                status: Some(TaskStatus::InProgress),
                assignee_session_id: None, started_at: None, completed_at: None,
                canceled_at: None, cancel_reason: None,
                branch: None,
            },
        ).unwrap();
        update_task(
            &conn,
            dep.id,
            &UpdateTaskParams {
                title: None, background: None, description: None, plan: None, priority: None,
                status: Some(TaskStatus::Completed),
                assignee_session_id: None, started_at: None, completed_at: None,
                canceled_at: None, cancel_reason: None,
                branch: None,
            },
        ).unwrap();

        // Create task with completed dep -> should be ready
        let ready_task = create_task(
            &conn,
            &CreateTaskParams {
                title: "ready".to_string(),
                dependencies: vec![dep.id],
                ..default_create_params("ready")
            },
        ).unwrap();
        update_task(
            &conn,
            ready_task.id,
            &UpdateTaskParams {
                title: None, background: None, description: None, plan: None, priority: None,
                status: Some(TaskStatus::Todo),
                assignee_session_id: None, started_at: None, completed_at: None,
                canceled_at: None, cancel_reason: None,
                branch: None,
            },
        ).unwrap();

        // Create another dep that is NOT completed
        let dep2 = create_task(&conn, &default_create_params("dep2")).unwrap();
        let blocked_task = create_task(
            &conn,
            &CreateTaskParams {
                title: "blocked".to_string(),
                dependencies: vec![dep2.id],
                ..default_create_params("blocked")
            },
        ).unwrap();
        update_task(
            &conn,
            blocked_task.id,
            &UpdateTaskParams {
                title: None, background: None, description: None, plan: None, priority: None,
                status: Some(TaskStatus::Todo),
                assignee_session_id: None, started_at: None, completed_at: None,
                canceled_at: None, cancel_reason: None,
                branch: None,
            },
        ).unwrap();

        let result = list_tasks(
            &conn,
            &ListTasksFilter {
                statuses: vec![],
                tags: vec![],
                depends_on: None,
                ready: true,
            },
        ).unwrap();

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].title, "ready");
    }

    #[test]
    fn unique_constraints() {
        let (_tmp, conn) = setup();
        let task = create_task(
            &conn,
            &CreateTaskParams {
                title: "t1".to_string(),
                branch: None,
                tags: vec!["rust".to_string()],
                ..default_create_params("t1")
            },
        )
        .unwrap();

        // Duplicate tag should fail
        let result = conn.execute(
            "INSERT INTO task_tags (task_id, tag) VALUES (?1, 'rust')",
            params![task.id],
        );
        assert!(result.is_err());
    }

    #[test]
    fn task_with_dependencies() {
        let (_tmp, conn) = setup();
        let dep1 = create_task(&conn, &default_create_params("dep1")).unwrap();
        let dep2 = create_task(&conn, &default_create_params("dep2")).unwrap();

        let task = create_task(
            &conn,
            &CreateTaskParams {
                title: "with deps".to_string(),
                dependencies: vec![dep1.id, dep2.id],
                ..default_create_params("with deps")
            },
        )
        .unwrap();

        assert_eq!(task.dependencies.len(), 2);
        assert!(task.dependencies.contains(&dep1.id));
        assert!(task.dependencies.contains(&dep2.id));
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
            &CreateTaskParams {
                branch: None,
                tags: vec!["old".to_string()],
                ..default_create_params("t")
            },
        )
        .unwrap();

        update_task_arrays(
            &conn,
            task.id,
            &UpdateTaskArrayParams {
                set_tags: Some(vec!["new1".to_string(), "new2".to_string()]),
                ..default_array_params()
            },
        )
        .unwrap();

        let updated = get_task(&conn, task.id).unwrap();
        assert_eq!(updated.tags.len(), 2);
        assert!(updated.tags.contains(&"new1".to_string()));
        assert!(updated.tags.contains(&"new2".to_string()));
        assert!(!updated.tags.contains(&"old".to_string()));
    }

    #[test]
    fn update_arrays_add_tags() {
        let (_tmp, conn) = setup();
        let task = create_task(
            &conn,
            &CreateTaskParams {
                branch: None,
                tags: vec!["existing".to_string()],
                ..default_create_params("t")
            },
        )
        .unwrap();

        update_task_arrays(
            &conn,
            task.id,
            &UpdateTaskArrayParams {
                add_tags: vec!["new".to_string(), "existing".to_string()],
                ..default_array_params()
            },
        )
        .unwrap();

        let updated = get_task(&conn, task.id).unwrap();
        assert_eq!(updated.tags.len(), 2);
        assert!(updated.tags.contains(&"existing".to_string()));
        assert!(updated.tags.contains(&"new".to_string()));
    }

    #[test]
    fn update_arrays_remove_tags() {
        let (_tmp, conn) = setup();
        let task = create_task(
            &conn,
            &CreateTaskParams {
                branch: None,
                tags: vec!["keep".to_string(), "remove".to_string()],
                ..default_create_params("t")
            },
        )
        .unwrap();

        update_task_arrays(
            &conn,
            task.id,
            &UpdateTaskArrayParams {
                remove_tags: vec!["remove".to_string()],
                ..default_array_params()
            },
        )
        .unwrap();

        let updated = get_task(&conn, task.id).unwrap();
        assert_eq!(updated.tags, vec!["keep"]);
    }

    #[test]
    fn update_arrays_set_definition_of_done() {
        let (_tmp, conn) = setup();
        let task = create_task(
            &conn,
            &CreateTaskParams {
                definition_of_done: vec!["old".to_string()],
                ..default_create_params("t")
            },
        )
        .unwrap();

        update_task_arrays(
            &conn,
            task.id,
            &UpdateTaskArrayParams {
                set_definition_of_done: Some(vec!["new1".to_string(), "new2".to_string()]),
                ..default_array_params()
            },
        )
        .unwrap();

        let updated = get_task(&conn, task.id).unwrap();
        assert_eq!(
            updated.definition_of_done,
            vec![
                DodItem { content: "new1".to_string(), checked: false },
                DodItem { content: "new2".to_string(), checked: false },
            ]
        );
    }

    #[test]
    fn update_arrays_add_and_remove_in_scope() {
        let (_tmp, conn) = setup();
        let task = create_task(
            &conn,
            &CreateTaskParams {
                in_scope: vec!["a".to_string(), "b".to_string()],
                ..default_create_params("t")
            },
        )
        .unwrap();

        update_task_arrays(
            &conn,
            task.id,
            &UpdateTaskArrayParams {
                add_in_scope: vec!["c".to_string()],
                remove_in_scope: vec!["a".to_string()],
                ..default_array_params()
            },
        )
        .unwrap();

        let updated = get_task(&conn, task.id).unwrap();
        assert_eq!(updated.in_scope, vec!["b", "c"]);
    }

    fn make_todo(conn: &Connection, title: &str, priority: Option<Priority>) -> Task {
        let task = create_task(
            conn,
            &CreateTaskParams {
                priority,
                ..default_create_params(title)
            },
        )
        .unwrap();
        update_task(
            conn,
            task.id,
            &UpdateTaskParams {
                title: None, background: None, description: None, plan: None, priority: None,
                status: Some(TaskStatus::Todo),
                assignee_session_id: None, started_at: None, completed_at: None,
                canceled_at: None, cancel_reason: None,
                branch: None,
            },
        )
        .unwrap()
    }

    fn make_completed(conn: &Connection, title: &str) -> Task {
        let task = make_todo(conn, title, None);
        update_task(
            conn,
            task.id,
            &UpdateTaskParams {
                title: None, background: None, description: None, plan: None, priority: None,
                status: Some(TaskStatus::InProgress),
                assignee_session_id: None, started_at: None, completed_at: None,
                canceled_at: None, cancel_reason: None,
                branch: None,
            },
        ).unwrap();
        update_task(
            conn,
            task.id,
            &UpdateTaskParams {
                title: None, background: None, description: None, plan: None, priority: None,
                status: Some(TaskStatus::Completed),
                assignee_session_id: None, started_at: None, completed_at: None,
                canceled_at: None, cancel_reason: None,
                branch: None,
            },
        ).unwrap()
    }

    #[test]
    fn next_task_returns_none_when_empty() {
        let (_tmp, conn) = setup();
        assert!(next_task(&conn).unwrap().is_none());
    }

    #[test]
    fn next_task_skips_blocked() {
        let (_tmp, conn) = setup();

        // Create a dep that is NOT completed (still draft)
        let dep = create_task(&conn, &default_create_params("dep")).unwrap();

        // Create a todo task that depends on dep
        let task = create_task(
            &conn,
            &CreateTaskParams {
                title: "blocked".to_string(),
                dependencies: vec![dep.id],
                ..default_create_params("blocked")
            },
        ).unwrap();
        update_task(
            &conn,
            task.id,
            &UpdateTaskParams {
                title: None, background: None, description: None, plan: None, priority: None,
                status: Some(TaskStatus::Todo),
                assignee_session_id: None, started_at: None, completed_at: None,
                canceled_at: None, cancel_reason: None,
                branch: None,
            },
        ).unwrap();

        assert!(next_task(&conn).unwrap().is_none());
    }

    #[test]
    fn next_task_priority_order() {
        let (_tmp, conn) = setup();

        make_todo(&conn, "low", Some(Priority::P3));
        make_todo(&conn, "high", Some(Priority::P0));
        make_todo(&conn, "mid", Some(Priority::P1));

        let task = next_task(&conn).unwrap().unwrap();
        assert_eq!(task.title, "high");
    }

    #[test]
    fn next_task_created_at_tiebreak() {
        let (_tmp, conn) = setup();

        // Same priority, created_at order should decide
        // Since tasks are inserted sequentially, the first one has earlier created_at
        make_todo(&conn, "first", Some(Priority::P2));
        make_todo(&conn, "second", Some(Priority::P2));

        let task = next_task(&conn).unwrap().unwrap();
        assert_eq!(task.title, "first");
    }

    #[test]
    fn next_task_id_tiebreak() {
        let (_tmp, conn) = setup();

        // Insert two tasks with same priority; SQLite created_at has second-level precision
        // so they'll likely have the same created_at, making id the final tiebreaker
        let t1 = make_todo(&conn, "t1", Some(Priority::P2));
        let t2 = make_todo(&conn, "t2", Some(Priority::P2));

        let task = next_task(&conn).unwrap().unwrap();
        // t1 was created first, so it has lower id
        assert!(t1.id < t2.id);
        assert_eq!(task.id, t1.id);
    }

    #[test]
    fn next_task_with_completed_dep() {
        let (_tmp, conn) = setup();

        let dep = make_completed(&conn, "dep");

        let task = create_task(
            &conn,
            &CreateTaskParams {
                title: "ready".to_string(),
                dependencies: vec![dep.id],
                ..default_create_params("ready")
            },
        ).unwrap();
        update_task(
            &conn,
            task.id,
            &UpdateTaskParams {
                title: None, background: None, description: None, plan: None, priority: None,
                status: Some(TaskStatus::Todo),
                assignee_session_id: None, started_at: None, completed_at: None,
                canceled_at: None, cancel_reason: None,
                branch: None,
            },
        ).unwrap();

        let result = next_task(&conn).unwrap().unwrap();
        assert_eq!(result.title, "ready");
    }

    // --- Dependency tests ---

    #[test]
    fn add_dependency_basic() {
        let (_tmp, conn) = setup();
        let t1 = create_task(&conn, &default_create_params("task1")).unwrap();
        let t2 = create_task(&conn, &default_create_params("task2")).unwrap();

        let updated = add_dependency(&conn, t1.id, t2.id).unwrap();
        assert!(updated.dependencies.contains(&t2.id));
    }

    #[test]
    fn add_dependency_self_error() {
        let (_tmp, conn) = setup();
        let t1 = create_task(&conn, &default_create_params("task1")).unwrap();
        let result = add_dependency(&conn, t1.id, t1.id);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("itself"));
    }

    #[test]
    fn add_dependency_nonexistent_task() {
        let (_tmp, conn) = setup();
        let t1 = create_task(&conn, &default_create_params("task1")).unwrap();
        assert!(add_dependency(&conn, t1.id, 999).is_err());
        assert!(add_dependency(&conn, 999, t1.id).is_err());
    }

    #[test]
    fn add_dependency_cycle_direct() {
        let (_tmp, conn) = setup();
        let t1 = create_task(&conn, &default_create_params("t1")).unwrap();
        let t2 = create_task(&conn, &default_create_params("t2")).unwrap();

        add_dependency(&conn, t1.id, t2.id).unwrap();
        let result = add_dependency(&conn, t2.id, t1.id);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("cycle"));
    }

    #[test]
    fn add_dependency_cycle_indirect() {
        let (_tmp, conn) = setup();
        let t1 = create_task(&conn, &default_create_params("t1")).unwrap();
        let t2 = create_task(&conn, &default_create_params("t2")).unwrap();
        let t3 = create_task(&conn, &default_create_params("t3")).unwrap();

        add_dependency(&conn, t1.id, t2.id).unwrap();
        add_dependency(&conn, t2.id, t3.id).unwrap();
        let result = add_dependency(&conn, t3.id, t1.id);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("cycle"));
    }

    #[test]
    fn add_dependency_idempotent() {
        let (_tmp, conn) = setup();
        let t1 = create_task(&conn, &default_create_params("t1")).unwrap();
        let t2 = create_task(&conn, &default_create_params("t2")).unwrap();

        add_dependency(&conn, t1.id, t2.id).unwrap();
        let result = add_dependency(&conn, t1.id, t2.id);
        assert!(result.is_ok());
        let task = result.unwrap();
        assert_eq!(task.dependencies.len(), 1);
    }

    #[test]
    fn remove_dependency_basic() {
        let (_tmp, conn) = setup();
        let t1 = create_task(&conn, &default_create_params("t1")).unwrap();
        let t2 = create_task(&conn, &default_create_params("t2")).unwrap();

        add_dependency(&conn, t1.id, t2.id).unwrap();
        let updated = remove_dependency(&conn, t1.id, t2.id).unwrap();
        assert!(updated.dependencies.is_empty());
    }

    #[test]
    fn remove_dependency_nonexistent() {
        let (_tmp, conn) = setup();
        let t1 = create_task(&conn, &default_create_params("t1")).unwrap();
        let t2 = create_task(&conn, &default_create_params("t2")).unwrap();

        let result = remove_dependency(&conn, t1.id, t2.id);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("dependency not found"));
    }

    #[test]
    fn set_dependencies_basic() {
        let (_tmp, conn) = setup();
        let t1 = create_task(&conn, &default_create_params("t1")).unwrap();
        let t2 = create_task(&conn, &default_create_params("t2")).unwrap();
        let t3 = create_task(&conn, &default_create_params("t3")).unwrap();

        let updated = set_dependencies(&conn, t1.id, &[t2.id, t3.id]).unwrap();
        assert_eq!(updated.dependencies.len(), 2);
        assert!(updated.dependencies.contains(&t2.id));
        assert!(updated.dependencies.contains(&t3.id));
    }

    #[test]
    fn set_dependencies_replace() {
        let (_tmp, conn) = setup();
        let t1 = create_task(&conn, &default_create_params("t1")).unwrap();
        let t2 = create_task(&conn, &default_create_params("t2")).unwrap();
        let t3 = create_task(&conn, &default_create_params("t3")).unwrap();

        set_dependencies(&conn, t1.id, &[t2.id]).unwrap();
        let updated = set_dependencies(&conn, t1.id, &[t3.id]).unwrap();
        assert_eq!(updated.dependencies, vec![t3.id]);
    }

    #[test]
    fn set_dependencies_empty() {
        let (_tmp, conn) = setup();
        let t1 = create_task(&conn, &default_create_params("t1")).unwrap();
        let t2 = create_task(&conn, &default_create_params("t2")).unwrap();

        add_dependency(&conn, t1.id, t2.id).unwrap();
        let updated = set_dependencies(&conn, t1.id, &[]).unwrap();
        assert!(updated.dependencies.is_empty());
    }

    #[test]
    fn set_dependencies_cycle_error_rollback() {
        let (_tmp, conn) = setup();
        let t1 = create_task(&conn, &default_create_params("t1")).unwrap();
        let t2 = create_task(&conn, &default_create_params("t2")).unwrap();
        let t3 = create_task(&conn, &default_create_params("t3")).unwrap();

        // t2 -> t1
        add_dependency(&conn, t2.id, t1.id).unwrap();
        // Try to set t1 -> [t3, t2] — t1->t2 would create cycle (t2->t1->t2)
        let result = set_dependencies(&conn, t1.id, &[t3.id, t2.id]);
        assert!(result.is_err());

        // Original state should be preserved (no deps on t1)
        let task = get_task(&conn, t1.id).unwrap();
        assert!(task.dependencies.is_empty());
    }

    #[test]
    fn list_dependencies_basic() {
        let (_tmp, conn) = setup();
        let t1 = create_task(&conn, &default_create_params("t1")).unwrap();
        let t2 = create_task(&conn, &default_create_params("t2")).unwrap();
        let t3 = create_task(&conn, &default_create_params("t3")).unwrap();

        add_dependency(&conn, t1.id, t2.id).unwrap();
        add_dependency(&conn, t1.id, t3.id).unwrap();

        let deps = list_dependencies(&conn, t1.id).unwrap();
        assert_eq!(deps.len(), 2);
        let dep_ids: Vec<i64> = deps.iter().map(|t| t.id).collect();
        assert!(dep_ids.contains(&t2.id));
        assert!(dep_ids.contains(&t3.id));
    }

    #[test]
    fn list_dependencies_empty() {
        let (_tmp, conn) = setup();
        let t1 = create_task(&conn, &default_create_params("t1")).unwrap();

        let deps = list_dependencies(&conn, t1.id).unwrap();
        assert!(deps.is_empty());
    }

    #[test]
    fn clear_optional_field_with_none() {
        let (_tmp, conn) = setup();
        let task = create_task(
            &conn,
            &CreateTaskParams {
                title: "t".to_string(),
                background: Some("bg".to_string()),
                ..default_create_params("t")
            },
        )
        .unwrap();
        assert_eq!(task.background.as_deref(), Some("bg"));

        let updated = update_task(
            &conn,
            task.id,
            &UpdateTaskParams {
                title: None,
                background: Some(None), // clear it
                description: None,
                plan: None,
                priority: None,
                status: None,
                assignee_session_id: None,
                started_at: None,
                completed_at: None,
                canceled_at: None,
                cancel_reason: None,
                branch: None,
            },
        )
        .unwrap();
        assert!(updated.background.is_none());
    }

    #[test]
    fn check_and_uncheck_dod() {
        let (_tmp, conn) = setup();
        let task = create_task(
            &conn,
            &CreateTaskParams {
                definition_of_done: vec!["item1".to_string(), "item2".to_string()],
                ..default_create_params("t")
            },
        )
        .unwrap();

        // Check first item
        let updated = check_dod(&conn, task.id, 1).unwrap();
        assert!(updated.definition_of_done[0].checked);
        assert!(!updated.definition_of_done[1].checked);

        // Check second item
        let updated = check_dod(&conn, task.id, 2).unwrap();
        assert!(updated.definition_of_done[0].checked);
        assert!(updated.definition_of_done[1].checked);

        // Uncheck first item
        let updated = uncheck_dod(&conn, task.id, 1).unwrap();
        assert!(!updated.definition_of_done[0].checked);
        assert!(updated.definition_of_done[1].checked);
    }

    #[test]
    fn check_dod_index_out_of_range() {
        let (_tmp, conn) = setup();
        let task = create_task(
            &conn,
            &CreateTaskParams {
                definition_of_done: vec!["item1".to_string()],
                ..default_create_params("t")
            },
        )
        .unwrap();

        assert!(check_dod(&conn, task.id, 0).is_err());
        assert!(check_dod(&conn, task.id, 2).is_err());
    }

    #[test]
    fn check_dod_empty_list() {
        let (_tmp, conn) = setup();
        let task = create_task(&conn, &default_create_params("t")).unwrap();
        assert!(check_dod(&conn, task.id, 1).is_err());
    }
}
