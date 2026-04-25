use crate::domain::contract::ContractEvent;
use crate::domain::metadata_field::MetadataFieldEvent;
use crate::domain::project::{ProjectEvent, ProjectId};
use crate::domain::task::TaskEvent;
use crate::domain::user::UserEvent;

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
        project_id: ProjectId,
        result: SelectResult,
    },
    Contract(ContractEvent),
    Project(ProjectEvent),
    User(UserEvent),
    MetadataField(MetadataFieldEvent),
}

impl HookTrigger {
    /// Returns the hook config event key name (action form), or `None` if
    /// this trigger does not have a corresponding hook config entry.
    pub fn event_name(&self) -> Option<&'static str> {
        match self {
            HookTrigger::Task(TaskEvent::Created) => Some("task_add"),
            HookTrigger::Task(TaskEvent::Updated { .. }) => Some("task_update"),
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
            HookTrigger::Project(ProjectEvent::Created) => Some("project_create"),
            HookTrigger::Project(ProjectEvent::Updated { .. }) => Some("project_update"),
            HookTrigger::Project(ProjectEvent::MemberAdded { .. }) => Some("project_member_add"),
            HookTrigger::Project(ProjectEvent::MemberRemoved { .. }) => {
                Some("project_member_remove")
            }
            HookTrigger::Project(ProjectEvent::MemberRoleChanged { .. }) => {
                Some("project_member_role_change")
            }
            HookTrigger::User(UserEvent::Created { .. }) => Some("user_create"),
            HookTrigger::User(UserEvent::Updated { .. }) => Some("user_update"),
            HookTrigger::User(UserEvent::ApiKeyIssued { .. }) => Some("user_api_key_issue"),
            HookTrigger::User(UserEvent::ApiKeyRevoked { .. }) => Some("user_api_key_revoke"),
            HookTrigger::User(UserEvent::SessionRevoked { .. }) => Some("user_session_revoke"),
            HookTrigger::MetadataField(MetadataFieldEvent::Defined { .. }) => {
                Some("metadata_field_define")
            }
            HookTrigger::MetadataField(MetadataFieldEvent::Removed { .. }) => {
                Some("metadata_field_remove")
            }
            // TaskEvent variants without a hook config entry (DependencyAdded /
            // DependencyRemoved / DependenciesSet / DodChecked / DodUnchecked).
            // These still have `otel_event_name()` mappings.
            _ => None,
        }
    }

    /// Returns the OTel `event.name` for this trigger as defined in Contract #8,
    /// or `None` for triggers that do not emit a business event (currently only
    /// `TaskSelect` — selection outcomes are not part of the 29 business events).
    ///
    /// Phase B1 / B3 reference this method when emitting LogRecords to ensure
    /// the `senko.<aggregate>.<verb>` naming stays consistent across all emit
    /// sites.
    pub fn otel_event_name(&self) -> Option<&'static str> {
        match self {
            HookTrigger::Task(ev) => Some(match ev {
                TaskEvent::Created => "senko.task.created",
                TaskEvent::Updated { .. } => "senko.task.updated",
                TaskEvent::Published => "senko.task.published",
                TaskEvent::Started => "senko.task.started",
                TaskEvent::Completed => "senko.task.completed",
                TaskEvent::Canceled => "senko.task.canceled",
                TaskEvent::DependencyAdded { .. } => "senko.task.dependency_added",
                TaskEvent::DependencyRemoved { .. } => "senko.task.dependency_removed",
                TaskEvent::DependenciesSet { .. } => "senko.task.dependencies_set",
                TaskEvent::DodChecked { .. } => "senko.task.dod_checked",
                TaskEvent::DodUnchecked { .. } => "senko.task.dod_unchecked",
            }),
            HookTrigger::Contract(ev) => Some(match ev {
                ContractEvent::Created => "senko.contract.created",
                ContractEvent::Updated => "senko.contract.updated",
                ContractEvent::Deleted => "senko.contract.deleted",
                ContractEvent::DodChecked { .. } => "senko.contract.dod_checked",
                ContractEvent::DodUnchecked { .. } => "senko.contract.dod_unchecked",
                ContractEvent::NoteAdded => "senko.contract.note_added",
            }),
            HookTrigger::Project(ev) => Some(match ev {
                ProjectEvent::Created => "senko.project.created",
                ProjectEvent::Updated { .. } => "senko.project.updated",
                ProjectEvent::MemberAdded { .. } => "senko.project.member_added",
                ProjectEvent::MemberRemoved { .. } => "senko.project.member_removed",
                ProjectEvent::MemberRoleChanged { .. } => "senko.project.member_role_changed",
            }),
            HookTrigger::User(ev) => Some(match ev {
                UserEvent::Created { .. } => "senko.user.created",
                UserEvent::Updated { .. } => "senko.user.updated",
                UserEvent::ApiKeyIssued { .. } => "senko.user.api_key_issued",
                UserEvent::ApiKeyRevoked { .. } => "senko.user.api_key_revoked",
                UserEvent::SessionRevoked { .. } => "senko.user.session_revoked",
            }),
            HookTrigger::MetadataField(ev) => Some(match ev {
                MetadataFieldEvent::Defined { .. } => "senko.metadata_field.defined",
                MetadataFieldEvent::Removed { .. } => "senko.metadata_field.removed",
            }),
            HookTrigger::TaskSelect { .. } => None,
        }
    }

    /// Valid event names for CLI validation.
    pub fn valid_event_names() -> &'static [&'static str] {
        &[
            "task_add",
            "task_update",
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
            "project_create",
            "project_update",
            "project_member_add",
            "project_member_remove",
            "project_member_role_change",
            "user_create",
            "user_update",
            "user_api_key_issue",
            "user_api_key_revoke",
            "user_session_revoke",
            "metadata_field_define",
            "metadata_field_remove",
        ]
    }

    /// Parse a user-supplied event name string into a HookTrigger.
    /// Used by the CLI `hooks test` subcommand. Variants that carry payloads
    /// are reconstructed with neutral placeholder values — the caller only
    /// needs the variant kind to look up the matching hook config entry.
    pub fn from_event_name(name: &str) -> Option<Self> {
        use crate::domain::metadata_field::MetadataFieldType;
        use crate::domain::user::{
            ApiKeyId, Role, SessionId, SessionRevokeScope, UserCreationSource, UserId,
        };

        match name {
            "task_add" => Some(HookTrigger::Task(TaskEvent::Created)),
            "task_update" => Some(HookTrigger::Task(TaskEvent::Updated {
                changed_fields: Vec::new(),
            })),
            "task_publish" => Some(HookTrigger::Task(TaskEvent::Published)),
            "task_start" => Some(HookTrigger::Task(TaskEvent::Started)),
            "task_complete" => Some(HookTrigger::Task(TaskEvent::Completed)),
            "task_cancel" => Some(HookTrigger::Task(TaskEvent::Canceled)),
            "task_select" => Some(HookTrigger::TaskSelect {
                project_id: ProjectId(0),
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
            "project_create" => Some(HookTrigger::Project(ProjectEvent::Created)),
            "project_update" => Some(HookTrigger::Project(ProjectEvent::Updated {
                changed_fields: Vec::new(),
            })),
            "project_member_add" => Some(HookTrigger::Project(ProjectEvent::MemberAdded {
                user_id: UserId(0),
                role: Role::Member,
            })),
            "project_member_remove" => Some(HookTrigger::Project(ProjectEvent::MemberRemoved {
                user_id: UserId(0),
            })),
            "project_member_role_change" => {
                Some(HookTrigger::Project(ProjectEvent::MemberRoleChanged {
                    user_id: UserId(0),
                    from_role: Role::Member,
                    to_role: Role::Owner,
                }))
            }
            "user_create" => Some(HookTrigger::User(UserEvent::Created {
                source: UserCreationSource::Manual,
            })),
            "user_update" => Some(HookTrigger::User(UserEvent::Updated {
                changed_fields: Vec::new(),
            })),
            "user_api_key_issue" => Some(HookTrigger::User(UserEvent::ApiKeyIssued {
                api_key_id: ApiKeyId(0),
            })),
            "user_api_key_revoke" => Some(HookTrigger::User(UserEvent::ApiKeyRevoked {
                api_key_id: ApiKeyId(0),
            })),
            "user_session_revoke" => Some(HookTrigger::User(UserEvent::SessionRevoked {
                session_id: SessionId(0),
                scope: SessionRevokeScope::Single,
            })),
            "metadata_field_define" => {
                Some(HookTrigger::MetadataField(MetadataFieldEvent::Defined {
                    field_name: String::new(),
                    field_type: MetadataFieldType::String,
                }))
            }
            "metadata_field_remove" => {
                Some(HookTrigger::MetadataField(MetadataFieldEvent::Removed {
                    field_name: String::new(),
                    field_type: MetadataFieldType::String,
                }))
            }
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
    fn valid_event_names_includes_all_actions() {
        let names = HookTrigger::valid_event_names();
        for expected in [
            // Task aggregate
            "task_add",
            "task_update",
            "task_publish",
            "task_start",
            "task_complete",
            "task_cancel",
            "task_select",
            // Contract aggregate
            "contract_add",
            "contract_edit",
            "contract_delete",
            "contract_dod_check",
            "contract_dod_uncheck",
            "contract_note_add",
            // Project aggregate
            "project_create",
            "project_update",
            "project_member_add",
            "project_member_remove",
            "project_member_role_change",
            // User aggregate
            "user_create",
            "user_update",
            "user_api_key_issue",
            "user_api_key_revoke",
            "user_session_revoke",
            // MetadataField aggregate
            "metadata_field_define",
            "metadata_field_remove",
        ] {
            assert!(
                names.contains(&expected),
                "expected {expected} in valid_event_names"
            );
        }
    }

    #[test]
    fn from_event_name_roundtrips_all_actions() {
        for name in HookTrigger::valid_event_names() {
            let trigger = HookTrigger::from_event_name(name)
                .unwrap_or_else(|| panic!("from_event_name({name}) returned None"));
            assert_eq!(
                trigger.event_name(),
                Some(*name),
                "roundtrip failed for {name}"
            );
        }
    }

    #[test]
    fn otel_event_name_matches_contract_8_spec() {
        use crate::domain::metadata_field::MetadataFieldType;
        use crate::domain::project::ProjectEvent;
        use crate::domain::task::TaskId;
        use crate::domain::user::{
            ApiKeyId, Role, SessionId, SessionRevokeScope, UserCreationSource, UserId,
        };

        // Task aggregate (11 events)
        let task_cases: [(TaskEvent, &str); 11] = [
            (TaskEvent::Created, "senko.task.created"),
            (
                TaskEvent::Updated {
                    changed_fields: vec!["title".into()],
                },
                "senko.task.updated",
            ),
            (TaskEvent::Published, "senko.task.published"),
            (TaskEvent::Started, "senko.task.started"),
            (TaskEvent::Completed, "senko.task.completed"),
            (TaskEvent::Canceled, "senko.task.canceled"),
            (
                TaskEvent::DependencyAdded { dep_id: TaskId(1) },
                "senko.task.dependency_added",
            ),
            (
                TaskEvent::DependencyRemoved { dep_id: TaskId(1) },
                "senko.task.dependency_removed",
            ),
            (
                TaskEvent::DependenciesSet {
                    dep_ids: vec![TaskId(1)],
                },
                "senko.task.dependencies_set",
            ),
            (TaskEvent::DodChecked { index: 0 }, "senko.task.dod_checked"),
            (
                TaskEvent::DodUnchecked { index: 0 },
                "senko.task.dod_unchecked",
            ),
        ];
        for (ev, expected) in task_cases {
            assert_eq!(
                HookTrigger::Task(ev).otel_event_name(),
                Some(expected),
                "task otel name mismatch"
            );
        }

        // Contract aggregate (6 events)
        let contract_cases: [(ContractEvent, &str); 6] = [
            (ContractEvent::Created, "senko.contract.created"),
            (ContractEvent::Updated, "senko.contract.updated"),
            (ContractEvent::Deleted, "senko.contract.deleted"),
            (
                ContractEvent::DodChecked { index: 0 },
                "senko.contract.dod_checked",
            ),
            (
                ContractEvent::DodUnchecked { index: 0 },
                "senko.contract.dod_unchecked",
            ),
            (ContractEvent::NoteAdded, "senko.contract.note_added"),
        ];
        for (ev, expected) in contract_cases {
            assert_eq!(
                HookTrigger::Contract(ev).otel_event_name(),
                Some(expected),
                "contract otel name mismatch"
            );
        }

        // Project aggregate (5 events)
        let project_cases: [(ProjectEvent, &str); 5] = [
            (ProjectEvent::Created, "senko.project.created"),
            (
                ProjectEvent::Updated {
                    changed_fields: vec!["description".into()],
                },
                "senko.project.updated",
            ),
            (
                ProjectEvent::MemberAdded {
                    user_id: UserId(1),
                    role: Role::Owner,
                },
                "senko.project.member_added",
            ),
            (
                ProjectEvent::MemberRemoved { user_id: UserId(1) },
                "senko.project.member_removed",
            ),
            (
                ProjectEvent::MemberRoleChanged {
                    user_id: UserId(1),
                    from_role: Role::Member,
                    to_role: Role::Owner,
                },
                "senko.project.member_role_changed",
            ),
        ];
        for (ev, expected) in project_cases {
            assert_eq!(
                HookTrigger::Project(ev).otel_event_name(),
                Some(expected),
                "project otel name mismatch"
            );
        }

        // User aggregate (5 events)
        let user_cases: [(UserEvent, &str); 5] = [
            (
                UserEvent::Created {
                    source: UserCreationSource::Manual,
                },
                "senko.user.created",
            ),
            (
                UserEvent::Updated {
                    changed_fields: vec!["display_name".into()],
                },
                "senko.user.updated",
            ),
            (
                UserEvent::ApiKeyIssued {
                    api_key_id: ApiKeyId(1),
                },
                "senko.user.api_key_issued",
            ),
            (
                UserEvent::ApiKeyRevoked {
                    api_key_id: ApiKeyId(1),
                },
                "senko.user.api_key_revoked",
            ),
            (
                UserEvent::SessionRevoked {
                    session_id: SessionId(1),
                    scope: SessionRevokeScope::All,
                },
                "senko.user.session_revoked",
            ),
        ];
        for (ev, expected) in user_cases {
            assert_eq!(
                HookTrigger::User(ev).otel_event_name(),
                Some(expected),
                "user otel name mismatch"
            );
        }

        // MetadataField aggregate (2 events)
        let mf_cases: [(MetadataFieldEvent, &str); 2] = [
            (
                MetadataFieldEvent::Defined {
                    field_name: "owner".into(),
                    field_type: MetadataFieldType::String,
                },
                "senko.metadata_field.defined",
            ),
            (
                MetadataFieldEvent::Removed {
                    field_name: "owner".into(),
                    field_type: MetadataFieldType::String,
                },
                "senko.metadata_field.removed",
            ),
        ];
        for (ev, expected) in mf_cases {
            assert_eq!(
                HookTrigger::MetadataField(ev).otel_event_name(),
                Some(expected),
                "metadata_field otel name mismatch"
            );
        }
    }

    #[test]
    fn task_select_has_no_otel_event_name() {
        let trigger = HookTrigger::TaskSelect {
            project_id: ProjectId(0),
            result: SelectResult::Selected,
        };
        assert_eq!(trigger.otel_event_name(), None);
    }
}
