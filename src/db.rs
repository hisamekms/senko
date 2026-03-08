use std::path::Path;

use anyhow::{Context, Result};
use rusqlite::{params, Connection, OptionalExtension};

use crate::models::{
    CreateTaskParams, ListTasksFilter, Priority, Task, TaskStatus, UpdateTaskParams,
};

pub fn open_db(project_root: &Path) -> Result<Connection> {
    let localflow_dir = project_root.join(".localflow");
    std::fs::create_dir_all(&localflow_dir)?;

    let db_path = localflow_dir.join("data.db");
    let conn = Connection::open(&db_path)?;

    conn.execute_batch("PRAGMA journal_mode=WAL;")?;
    conn.execute_batch("PRAGMA foreign_keys=ON;")?;

    create_schema(&conn)?;

    Ok(conn)
}

fn create_schema(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS tasks (
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

        CREATE TABLE IF NOT EXISTS task_definition_of_done (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            task_id INTEGER NOT NULL,
            content TEXT NOT NULL,
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

pub fn create_task(conn: &Connection, params: &CreateTaskParams) -> Result<Task> {
    let priority: i32 = params.priority.unwrap_or(Priority::P2).into();
    conn.execute(
        "INSERT INTO tasks (title, background, details, priority) VALUES (?1, ?2, ?3, ?4)",
        rusqlite::params![params.title, params.background, params.details, priority],
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
    let (title, background, details, status_str, priority_val, assignee_session_id, created_at, updated_at, started_at, completed_at, canceled_at, cancel_reason): (
        String, Option<String>, Option<String>, String, i32, Option<String>, String, String, Option<String>, Option<String>, Option<String>, Option<String>,
    ) = conn
        .query_row(
            "SELECT title, background, details, status, priority, assignee_session_id, created_at, updated_at, started_at, completed_at, canceled_at, cancel_reason FROM tasks WHERE id = ?1",
            params![id],
            |row| {
                Ok((
                    row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?,
                    row.get(4)?, row.get(5)?, row.get(6)?, row.get(7)?,
                    row.get(8)?, row.get(9)?, row.get(10)?, row.get(11)?,
                ))
            },
        )
        .context("task not found")?;

    let status: TaskStatus = status_str.parse()?;
    let priority = Priority::try_from(priority_val)?;

    let definition_of_done = query_string_list(
        conn,
        "SELECT content FROM task_definition_of_done WHERE task_id = ?1",
        id,
    )?;
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
        details,
        priority,
        status,
        assignee_session_id,
        created_at,
        updated_at,
        started_at,
        completed_at,
        canceled_at,
        cancel_reason,
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

    let mut sets = Vec::new();
    let mut values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

    if let Some(ref title) = params.title {
        sets.push("title = ?");
        values.push(Box::new(title.clone()));
    }
    if let Some(ref background) = params.background {
        sets.push("background = ?");
        values.push(Box::new(background.clone()));
    }
    if let Some(ref details) = params.details {
        sets.push("details = ?");
        values.push(Box::new(details.clone()));
    }
    if let Some(priority) = params.priority {
        sets.push("priority = ?");
        values.push(Box::new(i32::from(priority)));
    }
    if let Some(status) = params.status {
        sets.push("status = ?");
        values.push(Box::new(status.to_string()));
    }
    if let Some(ref assignee) = params.assignee_session_id {
        sets.push("assignee_session_id = ?");
        values.push(Box::new(assignee.clone()));
    }
    if let Some(ref started_at) = params.started_at {
        sets.push("started_at = ?");
        values.push(Box::new(started_at.clone()));
    }
    if let Some(ref completed_at) = params.completed_at {
        sets.push("completed_at = ?");
        values.push(Box::new(completed_at.clone()));
    }
    if let Some(ref canceled_at) = params.canceled_at {
        sets.push("canceled_at = ?");
        values.push(Box::new(canceled_at.clone()));
    }
    if let Some(ref cancel_reason) = params.cancel_reason {
        sets.push("cancel_reason = ?");
        values.push(Box::new(cancel_reason.clone()));
    }

    if !sets.is_empty() {
        sets.push("updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')");
        let sql = format!("UPDATE tasks SET {} WHERE id = ?", sets.join(", "));
        values.push(Box::new(id));
        let param_refs: Vec<&dyn rusqlite::types::ToSql> = values.iter().map(|v| v.as_ref()).collect();
        conn.execute(&sql, param_refs.as_slice())?;
    }

    get_task(conn, id)
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

    if let Some(status) = filter.status {
        conditions.push("t.status = ?".to_string());
        param_values.push(Box::new(status.to_string()));
    }

    if let Some(ref tag) = filter.tag {
        conditions.push(
            "EXISTS (SELECT 1 FROM task_tags tt WHERE tt.task_id = t.id AND tt.tag = ?)".to_string(),
        );
        param_values.push(Box::new(tag.clone()));
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
            details: None,
            priority: None,
            definition_of_done: vec![],
            in_scope: vec![],
            out_of_scope: vec![],
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
                details: Some("det".to_string()),
                priority: Some(Priority::P1),
                definition_of_done: vec!["done1".to_string(), "done2".to_string()],
                in_scope: vec!["scope1".to_string()],
                out_of_scope: vec!["out1".to_string()],
                tags: vec!["rust".to_string(), "cli".to_string()],
                dependencies: vec![],
            },
        )
        .unwrap();

        assert_eq!(task.title, "Test task");
        assert_eq!(task.background.as_deref(), Some("bg"));
        assert_eq!(task.details.as_deref(), Some("det"));
        assert_eq!(task.priority, Priority::P1);
        assert_eq!(task.status, TaskStatus::Draft);
        assert_eq!(task.definition_of_done, vec!["done1", "done2"]);
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
                details: Some(Some("new details".to_string())),
                priority: Some(Priority::P0),
                status: Some(TaskStatus::Todo),
                assignee_session_id: Some(Some("session-1".to_string())),
                started_at: None,
                completed_at: None,
                canceled_at: None,
                cancel_reason: None,
            },
        )
        .unwrap();

        assert_eq!(updated.title, "updated");
        assert_eq!(updated.background.as_deref(), Some("new bg"));
        assert_eq!(updated.details.as_deref(), Some("new details"));
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
                details: None,
                priority: None,
                status: Some(TaskStatus::InProgress),
                assignee_session_id: None,
                started_at: None,
                completed_at: None,
                canceled_at: None,
                cancel_reason: None,
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
                details: None,
                priority: None,
                status: Some(TaskStatus::Todo),
                assignee_session_id: None,
                started_at: None,
                completed_at: None,
                canceled_at: None,
                cancel_reason: None,
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
                details: None,
                priority: None,
                definition_of_done: vec!["d".to_string()],
                in_scope: vec!["s".to_string()],
                out_of_scope: vec!["o".to_string()],
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
                details: None,
                priority: None,
                status: Some(TaskStatus::Todo),
                assignee_session_id: None,
                started_at: None,
                completed_at: None,
                canceled_at: None,
                cancel_reason: None,
            },
        )
        .unwrap();

        let drafts = list_tasks(
            &conn,
            &ListTasksFilter {
                status: Some(TaskStatus::Draft),
                tag: None,
                ready: false,
            },
        )
        .unwrap();
        assert_eq!(drafts.len(), 1);
        assert_eq!(drafts[0].title, "todo");

        let todos = list_tasks(
            &conn,
            &ListTasksFilter {
                status: Some(TaskStatus::Todo),
                tag: None,
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
                tags: vec!["rust".to_string()],
                ..default_create_params("tagged")
            },
        )
        .unwrap();
        create_task(&conn, &default_create_params("untagged")).unwrap();

        let result = list_tasks(
            &conn,
            &ListTasksFilter {
                status: None,
                tag: Some("rust".to_string()),
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
                title: None, background: None, details: None, priority: None,
                status: Some(TaskStatus::Todo),
                assignee_session_id: None, started_at: None, completed_at: None,
                canceled_at: None, cancel_reason: None,
            },
        ).unwrap();
        update_task(
            &conn,
            dep.id,
            &UpdateTaskParams {
                title: None, background: None, details: None, priority: None,
                status: Some(TaskStatus::InProgress),
                assignee_session_id: None, started_at: None, completed_at: None,
                canceled_at: None, cancel_reason: None,
            },
        ).unwrap();
        update_task(
            &conn,
            dep.id,
            &UpdateTaskParams {
                title: None, background: None, details: None, priority: None,
                status: Some(TaskStatus::Completed),
                assignee_session_id: None, started_at: None, completed_at: None,
                canceled_at: None, cancel_reason: None,
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
                title: None, background: None, details: None, priority: None,
                status: Some(TaskStatus::Todo),
                assignee_session_id: None, started_at: None, completed_at: None,
                canceled_at: None, cancel_reason: None,
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
                title: None, background: None, details: None, priority: None,
                status: Some(TaskStatus::Todo),
                assignee_session_id: None, started_at: None, completed_at: None,
                canceled_at: None, cancel_reason: None,
            },
        ).unwrap();

        let result = list_tasks(
            &conn,
            &ListTasksFilter {
                status: None,
                tag: None,
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
                title: None, background: None, details: None, priority: None,
                status: Some(TaskStatus::Todo),
                assignee_session_id: None, started_at: None, completed_at: None,
                canceled_at: None, cancel_reason: None,
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
                title: None, background: None, details: None, priority: None,
                status: Some(TaskStatus::InProgress),
                assignee_session_id: None, started_at: None, completed_at: None,
                canceled_at: None, cancel_reason: None,
            },
        ).unwrap();
        update_task(
            conn,
            task.id,
            &UpdateTaskParams {
                title: None, background: None, details: None, priority: None,
                status: Some(TaskStatus::Completed),
                assignee_session_id: None, started_at: None, completed_at: None,
                canceled_at: None, cancel_reason: None,
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
                title: None, background: None, details: None, priority: None,
                status: Some(TaskStatus::Todo),
                assignee_session_id: None, started_at: None, completed_at: None,
                canceled_at: None, cancel_reason: None,
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
                title: None, background: None, details: None, priority: None,
                status: Some(TaskStatus::Todo),
                assignee_session_id: None, started_at: None, completed_at: None,
                canceled_at: None, cancel_reason: None,
            },
        ).unwrap();

        let result = next_task(&conn).unwrap().unwrap();
        assert_eq!(result.title, "ready");
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
                details: None,
                priority: None,
                status: None,
                assignee_session_id: None,
                started_at: None,
                completed_at: None,
                canceled_at: None,
                cancel_reason: None,
            },
        )
        .unwrap();
        assert!(updated.background.is_none());
    }
}
