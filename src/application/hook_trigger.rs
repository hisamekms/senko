use crate::domain::task::TaskEvent;

/// Identifies which hook should fire. Maps domain events to hook config keys.
/// Variants whose `event_name()` returns `None` do not trigger any hook.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HookTrigger {
    Task(TaskEvent),
    NoEligibleTask { project_id: i64 },
}

impl HookTrigger {
    /// Returns the hook config event key name, or `None` if this trigger
    /// does not have a corresponding hook config entry.
    pub fn event_name(&self) -> Option<&'static str> {
        match self {
            HookTrigger::Task(TaskEvent::Created) => Some("task_added"),
            HookTrigger::Task(TaskEvent::Readied) => Some("task_ready"),
            HookTrigger::Task(TaskEvent::Started) => Some("task_started"),
            HookTrigger::Task(TaskEvent::Completed) => Some("task_completed"),
            HookTrigger::Task(TaskEvent::Canceled) => Some("task_canceled"),
            HookTrigger::NoEligibleTask { .. } => Some("no_eligible_task"),
            _ => None,
        }
    }

    /// Valid event names for CLI validation.
    pub fn valid_event_names() -> &'static [&'static str] {
        &[
            "task_added",
            "task_ready",
            "task_started",
            "task_completed",
            "task_canceled",
            "no_eligible_task",
        ]
    }

    /// Parse a user-supplied event name string into a HookTrigger.
    /// Used by the CLI `hooks test` subcommand.
    pub fn from_event_name(name: &str) -> Option<Self> {
        match name {
            "task_added" => Some(HookTrigger::Task(TaskEvent::Created)),
            "task_ready" => Some(HookTrigger::Task(TaskEvent::Readied)),
            "task_started" => Some(HookTrigger::Task(TaskEvent::Started)),
            "task_completed" => Some(HookTrigger::Task(TaskEvent::Completed)),
            "task_canceled" => Some(HookTrigger::Task(TaskEvent::Canceled)),
            "no_eligible_task" => Some(HookTrigger::NoEligibleTask { project_id: 0 }),
            _ => None,
        }
    }
}
