use std::collections::HashMap;
use std::io::Write;
use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use chrono::Utc;
use rusqlite::Connection;
use uuid::Uuid;

use crate::db;
use crate::models::{Priority, Task, TaskStatus};

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct Config {
    #[serde(default)]
    pub hooks: HooksConfig,
    #[serde(default)]
    pub workflow: WorkflowConfig,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompletionMode {
    MergeThenComplete,
    PrThenComplete,
}

impl Default for CompletionMode {
    fn default() -> Self {
        CompletionMode::MergeThenComplete
    }
}

impl std::fmt::Display for CompletionMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CompletionMode::MergeThenComplete => write!(f, "merge_then_complete"),
            CompletionMode::PrThenComplete => write!(f, "pr_then_complete"),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct WorkflowConfig {
    #[serde(default)]
    pub completion_mode: CompletionMode,
    #[serde(default = "default_true")]
    pub auto_merge: bool,
}

fn default_true() -> bool {
    true
}

impl Default for WorkflowConfig {
    fn default() -> Self {
        Self {
            completion_mode: CompletionMode::default(),
            auto_merge: true,
        }
    }
}

mod string_or_vec {
    use serde::{Deserialize, Deserializer};

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum StringOrVec {
            String(String),
            Vec(Vec<String>),
        }
        match StringOrVec::deserialize(deserializer)? {
            StringOrVec::String(s) => Ok(vec![s]),
            StringOrVec::Vec(v) => Ok(v),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct HooksConfig {
    #[serde(default, deserialize_with = "string_or_vec::deserialize")]
    pub on_task_added: Vec<String>,
    #[serde(default, deserialize_with = "string_or_vec::deserialize")]
    pub on_task_ready: Vec<String>,
    #[serde(default, deserialize_with = "string_or_vec::deserialize")]
    pub on_task_started: Vec<String>,
    #[serde(default, deserialize_with = "string_or_vec::deserialize")]
    pub on_task_completed: Vec<String>,
    #[serde(default, deserialize_with = "string_or_vec::deserialize")]
    pub on_task_canceled: Vec<String>,
}

#[derive(Debug, Serialize, Clone)]
pub struct UnblockedTask {
    pub id: i64,
    pub title: String,
    pub priority: Priority,
    pub metadata: Option<serde_json::Value>,
}

#[derive(Debug, Serialize)]
pub struct HookEvent {
    pub event_id: String,
    pub event: String,
    pub timestamp: String,
    pub task: Task,
    pub stats: HashMap<String, i64>,
    pub ready_count: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from_status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unblocked_tasks: Option<Vec<UnblockedTask>>,
}

pub fn load_config(project_root: &Path) -> Result<Config> {
    let config_path = project_root.join(".localflow").join("config.toml");
    if !config_path.exists() {
        return Ok(Config::default());
    }
    let content =
        std::fs::read_to_string(&config_path).context("failed to read .localflow/config.toml")?;
    let config: Config = toml::from_str(&content).context("failed to parse config.toml")?;
    Ok(config)
}

pub fn build_event(
    event_name: &str,
    task: &Task,
    conn: &Connection,
    from_status: Option<TaskStatus>,
    unblocked: Option<Vec<UnblockedTask>>,
) -> HookEvent {
    let stats = db::task_stats(conn).unwrap_or_default();
    let ready_count = db::ready_count(conn).unwrap_or(0);
    HookEvent {
        event_id: Uuid::new_v4().to_string(),
        event: event_name.into(),
        timestamp: Utc::now().to_rfc3339(),
        task: task.clone(),
        stats,
        ready_count,
        from_status: from_status.map(|s| s.to_string()),
        unblocked_tasks: unblocked,
    }
}

fn execute_hook(command: &str, json: &str) -> Result<()> {
    let mut child = std::process::Command::new("sh")
        .arg("-c")
        .arg(command)
        .stdin(std::process::Stdio::piped())
        .spawn()
        .context("failed to spawn hook command")?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(json.as_bytes())?;
    }
    // Don't wait — fire and forget. The child process runs independently.
    Ok(())
}

/// Fire hooks for the given event, spawning each hook command as a
/// fire-and-forget child process. Returns immediately.
pub fn fire_hooks(
    config: &Config,
    event_name: &str,
    task: &Task,
    conn: &Connection,
    from_status: Option<TaskStatus>,
    unblocked: Option<Vec<UnblockedTask>>,
) {
    let commands = match event_name {
        "task_added" => &config.hooks.on_task_added,
        "task_ready" => &config.hooks.on_task_ready,
        "task_started" => &config.hooks.on_task_started,
        "task_completed" => &config.hooks.on_task_completed,
        "task_canceled" => &config.hooks.on_task_canceled,
        _ => return,
    };
    if commands.is_empty() {
        return;
    }

    let event = build_event(event_name, task, conn, from_status, unblocked);
    let json = match serde_json::to_string(&event) {
        Ok(j) => j,
        Err(e) => {
            eprintln!("hook error: failed to serialize event: {e}");
            return;
        }
    };

    for cmd in commands {
        if let Err(e) = execute_hook(cmd, &json) {
            eprintln!("hook error ({}): {:#}", event_name, e);
        }
    }
}

/// Compute newly unblocked tasks after a task completion.
/// Call this after `db::complete_task` with the set of ready task IDs
/// captured before the completion.
pub fn compute_unblocked(
    conn: &Connection,
    prev_ready_ids: &std::collections::HashSet<i64>,
) -> Vec<UnblockedTask> {
    let curr_ready = db::list_ready_tasks(conn).unwrap_or_default();
    curr_ready
        .iter()
        .filter(|t| !prev_ready_ids.contains(&t.id))
        .map(|t| UnblockedTask {
            id: t.id,
            title: t.title.clone(),
            priority: t.priority,
            metadata: t.metadata.clone(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup_db() -> (tempfile::TempDir, Connection) {
        let dir = tempfile::tempdir().unwrap();
        let conn = db::open_db(dir.path()).unwrap();
        (dir, conn)
    }

    #[test]
    fn load_config_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        let config = load_config(dir.path()).unwrap();
        assert!(config.hooks.on_task_added.is_empty());
        assert!(config.hooks.on_task_completed.is_empty());
    }

    #[test]
    fn load_config_valid_toml() {
        let dir = tempfile::tempdir().unwrap();
        let localflow_dir = dir.path().join(".localflow");
        std::fs::create_dir_all(&localflow_dir).unwrap();
        std::fs::write(
            localflow_dir.join("config.toml"),
            r#"
[hooks]
on_task_added = "echo added"
on_task_completed = "echo completed"
"#,
        )
        .unwrap();

        let config = load_config(dir.path()).unwrap();
        assert_eq!(config.hooks.on_task_added, vec!["echo added"]);
        assert_eq!(config.hooks.on_task_completed, vec!["echo completed"]);
    }

    #[test]
    fn load_config_empty_hooks() {
        let dir = tempfile::tempdir().unwrap();
        let localflow_dir = dir.path().join(".localflow");
        std::fs::create_dir_all(&localflow_dir).unwrap();
        std::fs::write(localflow_dir.join("config.toml"), "[hooks]\n").unwrap();

        let config = load_config(dir.path()).unwrap();
        assert!(config.hooks.on_task_added.is_empty());
        assert!(config.hooks.on_task_completed.is_empty());
    }

    #[test]
    fn hook_event_serialization() {
        let (_dir, conn) = setup_db();
        let task = Task {
            id: 1,
            title: "Test".into(),
            background: None,
            description: None,
            plan: None,
            priority: crate::models::Priority::P2,
            status: TaskStatus::Draft,
            assignee_session_id: None,
            created_at: "2026-01-01T00:00:00Z".into(),
            updated_at: "2026-01-01T00:00:00Z".into(),
            started_at: None,
            completed_at: None,
            canceled_at: None,
            cancel_reason: None,
            branch: None,
            pr_url: None,
            metadata: None,
            definition_of_done: vec![],
            in_scope: vec![],
            out_of_scope: vec![],
            tags: vec![],
            dependencies: vec![],
        };
        let event = build_event("task_added", &task, &conn, None, None);
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("\"event\":\"task_added\""));
        assert!(json.contains("\"id\":1"));
        assert!(json.contains("\"event_id\""));
        assert!(json.contains("\"timestamp\""));
        assert!(json.contains("\"stats\""));
        assert!(json.contains("\"ready_count\""));
        // unblocked_tasks should be absent when None
        assert!(!json.contains("\"unblocked_tasks\""));
    }

    #[test]
    fn event_has_valid_uuid_and_timestamp() {
        let (_dir, conn) = setup_db();
        let task = Task {
            id: 1,
            title: "Test".into(),
            background: None,
            description: None,
            plan: None,
            priority: crate::models::Priority::P2,
            status: TaskStatus::Draft,
            assignee_session_id: None,
            created_at: "2026-01-01T00:00:00Z".into(),
            updated_at: "2026-01-01T00:00:00Z".into(),
            started_at: None,
            completed_at: None,
            canceled_at: None,
            cancel_reason: None,
            branch: None,
            pr_url: None,
            metadata: None,
            definition_of_done: vec![],
            in_scope: vec![],
            out_of_scope: vec![],
            tags: vec![],
            dependencies: vec![],
        };
        let event = build_event("task_added", &task, &conn, None, None);
        assert!(Uuid::parse_str(&event.event_id).is_ok());
        assert!(chrono::DateTime::parse_from_rfc3339(&event.timestamp).is_ok());
    }

    #[test]
    fn event_has_stats() {
        let (_dir, conn) = setup_db();
        db::create_task(
            &conn,
            &crate::models::CreateTaskParams {
                title: "Task1".into(),
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
            },
        )
        .unwrap();
        let task = db::get_task(&conn, 1).unwrap();
        let event = build_event("task_added", &task, &conn, None, None);
        assert!(event.stats.contains_key("draft"));
        assert_eq!(*event.stats.get("draft").unwrap(), 1);
    }

    #[test]
    fn event_has_ready_count() {
        let (_dir, conn) = setup_db();
        db::create_task(
            &conn,
            &crate::models::CreateTaskParams {
                title: "Ready".into(),
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
            },
        )
        .unwrap();
        db::ready_task(&conn, 1).unwrap();
        let task = db::get_task(&conn, 1).unwrap();
        let event = build_event("task_added", &task, &conn, None, None);
        assert_eq!(event.ready_count, 1);
    }

    #[test]
    fn compute_unblocked_finds_newly_ready() {
        let (_dir, conn) = setup_db();

        // Create task 1 (will be completed) and task 2 (depends on task 1)
        db::create_task(
            &conn,
            &crate::models::CreateTaskParams {
                title: "Dependency".into(),
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
            },
        )
        .unwrap();
        db::ready_task(&conn, 1).unwrap();
        db::start_task(&conn, 1, None, "2025-01-01T00:00:00Z").unwrap();

        db::create_task(
            &conn,
            &crate::models::CreateTaskParams {
                title: "Blocked".into(),
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
            },
        )
        .unwrap();
        db::ready_task(&conn, 2).unwrap();
        db::add_dependency(&conn, 2, 1).unwrap();

        // Capture ready tasks before completion
        let prev_ready: std::collections::HashSet<i64> =
            db::list_ready_tasks(&conn).unwrap().iter().map(|t| t.id).collect();

        // Complete task 1
        db::complete_task(&conn, 1, "2025-01-01T00:00:00Z").unwrap();

        let unblocked = compute_unblocked(&conn, &prev_ready);
        assert_eq!(unblocked.len(), 1);
        assert_eq!(unblocked[0].id, 2);
        assert_eq!(unblocked[0].title, "Blocked");
    }

    #[test]
    fn fire_hooks_executes_multiple_hooks() {
        let dir = tempfile::tempdir().unwrap();
        let marker1 = dir.path().join("hook1.txt");
        let marker2 = dir.path().join("hook2.txt");
        let cmd1 = format!("echo hook1 > {}", marker1.display());
        let cmd2 = format!("echo hook2 > {}", marker2.display());

        let config = Config {
            workflow: Default::default(),
            hooks: HooksConfig {
                on_task_added: vec![cmd1, cmd2],
                on_task_completed: vec![],
                ..Default::default()
            },
        };

        let (_db_dir, conn) = setup_db();
        let task = Task {
            id: 1,
            title: "Test".into(),
            background: None,
            description: None,
            plan: None,
            priority: crate::models::Priority::P2,
            status: TaskStatus::Draft,
            assignee_session_id: None,
            created_at: "2026-01-01T00:00:00Z".into(),
            updated_at: "2026-01-01T00:00:00Z".into(),
            started_at: None,
            completed_at: None,
            canceled_at: None,
            cancel_reason: None,
            branch: None,
            pr_url: None,
            metadata: None,
            definition_of_done: vec![],
            in_scope: vec![],
            out_of_scope: vec![],
            tags: vec![],
            dependencies: vec![],
        };
        fire_hooks(&config, "task_added", &task, &conn, None, None);

        // Give child processes a moment to complete
        std::thread::sleep(std::time::Duration::from_millis(200));

        assert!(marker1.exists(), "first hook should have run");
        assert!(marker2.exists(), "second hook should have run");
    }

    #[test]
    fn fire_hooks_noop_when_no_commands() {
        let (_db_dir, conn) = setup_db();
        let config = Config::default();
        let task = Task {
            id: 1,
            title: "Test".into(),
            background: None,
            description: None,
            plan: None,
            priority: crate::models::Priority::P2,
            status: TaskStatus::Draft,
            assignee_session_id: None,
            created_at: "2026-01-01T00:00:00Z".into(),
            updated_at: "2026-01-01T00:00:00Z".into(),
            started_at: None,
            completed_at: None,
            canceled_at: None,
            cancel_reason: None,
            branch: None,
            pr_url: None,
            metadata: None,
            definition_of_done: vec![],
            in_scope: vec![],
            out_of_scope: vec![],
            tags: vec![],
            dependencies: vec![],
        };
        // Should not panic
        fire_hooks(&config, "task_added", &task, &conn, None, None);
    }

    #[test]
    fn parse_hooks_string_value() {
        let toml_str = r#"
[hooks]
on_task_added = "echo added"
on_task_completed = "echo completed"
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(config.hooks.on_task_added, vec!["echo added"]);
        assert_eq!(config.hooks.on_task_completed, vec!["echo completed"]);
    }

    #[test]
    fn parse_hooks_array_value() {
        let toml_str = r#"
[hooks]
on_task_added = ["echo first", "echo second"]
on_task_completed = ["notify", "log"]
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(
            config.hooks.on_task_added,
            vec!["echo first", "echo second"]
        );
        assert_eq!(config.hooks.on_task_completed, vec!["notify", "log"]);
    }

    #[test]
    fn parse_hooks_missing_fields() {
        let toml_str = "[hooks]\n";
        let config: Config = toml::from_str(toml_str).unwrap();
        assert!(config.hooks.on_task_added.is_empty());
        assert!(config.hooks.on_task_completed.is_empty());
    }

    #[test]
    fn hook_receives_json_on_stdin() {
        let dir = tempfile::tempdir().unwrap();
        let output_file = dir.path().join("stdin_capture.json");
        let cmd = format!("cat > {}", output_file.display());

        let config = Config {
            workflow: Default::default(),
            hooks: HooksConfig {
                on_task_added: vec![cmd],
                ..Default::default()
            },
        };

        let (_db_dir, conn) = setup_db();
        let task = Task {
            id: 42,
            title: "Hook stdin test".into(),
            background: None,
            description: None,
            plan: None,
            priority: crate::models::Priority::P1,
            status: TaskStatus::Draft,
            assignee_session_id: None,
            created_at: "2026-01-01T00:00:00Z".into(),
            updated_at: "2026-01-01T00:00:00Z".into(),
            started_at: None,
            completed_at: None,
            canceled_at: None,
            cancel_reason: None,
            branch: None,
            pr_url: None,
            metadata: None,
            definition_of_done: vec![],
            in_scope: vec![],
            out_of_scope: vec![],
            tags: vec![],
            dependencies: vec![],
        };
        fire_hooks(&config, "task_added", &task, &conn, None, None);

        std::thread::sleep(std::time::Duration::from_millis(200));

        let content = std::fs::read_to_string(&output_file).unwrap();
        let json: serde_json::Value = serde_json::from_str(&content).unwrap();
        assert_eq!(json["event"], "task_added");
        assert_eq!(json["task"]["id"], 42);
        assert_eq!(json["task"]["title"], "Hook stdin test");
    }
}
