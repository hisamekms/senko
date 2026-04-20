use crate::domain::contract::ContractEvent;
use crate::domain::task::TaskEvent;

/// Result of a task selection attempt (only meaningful for `task_select`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectResult {
    Selected,
    None,
}

impl SelectResult {
    pub fn as_str(&self) -> &'static str {
        match self {
            SelectResult::Selected => "selected",
            SelectResult::None => "none",
        }
    }
}

/// Identifies which hook should fire. Maps domain events to hook config keys.
/// Variants whose `event_name()` returns `None` do not trigger any hook.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HookTrigger {
    Task(TaskEvent),
    /// Task selection outcome (from `task next`).
    TaskSelect {
        project_id: i64,
        result: SelectResult,
    },
    Contract(ContractEvent),
}

impl HookTrigger {
    /// Returns the hook config event key name (action form), or `None` if
    /// this trigger does not have a corresponding hook config entry.
    pub fn event_name(&self) -> Option<&'static str> {
        match self {
            HookTrigger::Task(TaskEvent::Created) => Some("task_add"),
            HookTrigger::Task(TaskEvent::Published) => Some("task_publish"),
            HookTrigger::Task(TaskEvent::Started) => Some("task_start"),
            HookTrigger::Task(TaskEvent::Completed) => Some("task_complete"),
            HookTrigger::Task(TaskEvent::Canceled) => Some("task_cancel"),
            HookTrigger::TaskSelect { .. } => Some("task_select"),
            HookTrigger::Contract(ContractEvent::Created) => Some("contract_add"),
            HookTrigger::Contract(ContractEvent::Updated) => Some("contract_edit"),
            HookTrigger::Contract(ContractEvent::Deleted) => Some("contract_delete"),
            HookTrigger::Contract(ContractEvent::DodChecked { .. }) => Some("contract_dod_check"),
            HookTrigger::Contract(ContractEvent::DodUnchecked { .. }) => {
                Some("contract_dod_uncheck")
            }
            HookTrigger::Contract(ContractEvent::NoteAdded) => Some("contract_note_add"),
            _ => None,
        }
    }

    /// Valid event names for CLI validation.
    pub fn valid_event_names() -> &'static [&'static str] {
        &[
            "task_add",
            "task_publish",
            "task_start",
            "task_complete",
            "task_cancel",
            "task_select",
            "contract_add",
            "contract_edit",
            "contract_delete",
            "contract_dod_check",
            "contract_dod_uncheck",
            "contract_note_add",
        ]
    }

    /// Parse a user-supplied event name string into a HookTrigger.
    /// Used by the CLI `hooks test` subcommand.
    pub fn from_event_name(name: &str) -> Option<Self> {
        match name {
            "task_add" => Some(HookTrigger::Task(TaskEvent::Created)),
            "task_publish" => Some(HookTrigger::Task(TaskEvent::Published)),
            "task_start" => Some(HookTrigger::Task(TaskEvent::Started)),
            "task_complete" => Some(HookTrigger::Task(TaskEvent::Completed)),
            "task_cancel" => Some(HookTrigger::Task(TaskEvent::Canceled)),
            "task_select" => Some(HookTrigger::TaskSelect {
                project_id: 0,
                result: SelectResult::Selected,
            }),
            "contract_add" => Some(HookTrigger::Contract(ContractEvent::Created)),
            "contract_edit" => Some(HookTrigger::Contract(ContractEvent::Updated)),
            "contract_delete" => Some(HookTrigger::Contract(ContractEvent::Deleted)),
            "contract_dod_check" => Some(HookTrigger::Contract(ContractEvent::DodChecked {
                index: 0,
            })),
            "contract_dod_uncheck" => Some(HookTrigger::Contract(ContractEvent::DodUnchecked {
                index: 0,
            })),
            "contract_note_add" => Some(HookTrigger::Contract(ContractEvent::NoteAdded)),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contract_triggers_have_event_names() {
        let cases = [
            (ContractEvent::Created, "contract_add"),
            (ContractEvent::Updated, "contract_edit"),
            (ContractEvent::Deleted, "contract_delete"),
            (ContractEvent::DodChecked { index: 0 }, "contract_dod_check"),
            (
                ContractEvent::DodUnchecked { index: 0 },
                "contract_dod_uncheck",
            ),
            (ContractEvent::NoteAdded, "contract_note_add"),
        ];
        for (ev, expected) in cases {
            assert_eq!(HookTrigger::Contract(ev).event_name(), Some(expected));
        }
    }

    #[test]
    fn valid_event_names_includes_contract_actions() {
        let names = HookTrigger::valid_event_names();
        for expected in [
            "contract_add",
            "contract_edit",
            "contract_delete",
            "contract_dod_check",
            "contract_dod_uncheck",
            "contract_note_add",
        ] {
            assert!(
                names.contains(&expected),
                "expected {expected} in valid_event_names"
            );
        }
    }

    #[test]
    fn from_event_name_roundtrips_contract_actions() {
        for expected in [
            "contract_add",
            "contract_edit",
            "contract_delete",
            "contract_dod_check",
            "contract_dod_uncheck",
            "contract_note_add",
        ] {
            let trigger = HookTrigger::from_event_name(expected).expect("trigger parse succeeds");
            assert_eq!(trigger.event_name(), Some(expected));
        }
    }
}
