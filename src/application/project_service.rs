use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;

use crate::application::auth::{Permission, require_project_role};
use crate::application::port::{ProjectOperations, TaskBackend};
use crate::domain::pagination::ListPage;
use crate::domain::project::{
    CreateProjectParams, ListProjectMembersFilter, ListProjectsFilter, Project, ProjectEvent,
    ProjectId, UpdateProjectParams,
};
use crate::domain::task::ListTasksFilter;
use crate::domain::user::{AddProjectMemberParams, ProjectMember, Role, UserId};

/// Emit Contract #8 business events for a `ProjectService` mutation.
///
/// Iterates the events vector returned by the mutation and dispatches one
/// `senko.project.*` LogRecord per variant. Common attributes (enduser.*,
/// senko.operation.id, baggage) are auto-attached by `BusinessAttributesProcessor`.
fn emit_project_events(project_id: ProjectId, events: &[ProjectEvent]) {
    for ev in events {
        match ev {
            ProjectEvent::Created => {
                crate::emit_business_event!(
                    "senko.project.created",
                    senko.project.id = project_id.0,
                );
            }
            ProjectEvent::Updated { changed_fields } => {
                let changed_fields_json = serde_json::to_string(changed_fields).unwrap_or_default();
                crate::emit_business_event!(
                    "senko.project.updated",
                    senko.project.id = project_id.0,
                    changed_fields = changed_fields_json.as_str(),
                );
            }
            ProjectEvent::MemberAdded { user_id, role } => {
                crate::emit_business_event!(
                    "senko.project.member_added",
                    senko.project.id = project_id.0,
                    senko.user.id = user_id.0,
                    role = %role,
                );
            }
            ProjectEvent::MemberRemoved { user_id } => {
                crate::emit_business_event!(
                    "senko.project.member_removed",
                    senko.project.id = project_id.0,
                    senko.user.id = user_id.0,
                );
            }
            ProjectEvent::MemberRoleChanged {
                user_id,
                from_role,
                to_role,
            } => {
                crate::emit_business_event!(
                    "senko.project.member_role_changed",
                    senko.project.id = project_id.0,
                    senko.user.id = user_id.0,
                    from_role = %from_role,
                    to_role = %to_role,
                );
            }
        }
    }
}

pub struct ProjectService {
    backend: Arc<dyn TaskBackend>,
}

impl ProjectService {
    pub fn new(backend: Arc<dyn TaskBackend>) -> Self {
        Self { backend }
    }
}

#[async_trait]
impl ProjectOperations for ProjectService {
    async fn list_projects(
        &self,
        filter: &ListProjectsFilter,
        caller_user_id: Option<UserId>,
    ) -> Result<ListPage<Project>> {
        self.backend.list_projects(filter, caller_user_id).await
    }

    async fn create_project(
        &self,
        params: &CreateProjectParams,
        caller_user_id: Option<UserId>,
    ) -> Result<(Project, Vec<ProjectEvent>)> {
        params.validate()?;
        let project = self.backend.create_project(params).await?;
        let mut events = vec![ProjectEvent::Created];
        if let Some(uid) = caller_user_id {
            let member_params = AddProjectMemberParams::new(uid, Some(Role::Owner));
            self.backend
                .add_project_member(project.id(), &member_params)
                .await?;
            events.push(ProjectEvent::MemberAdded {
                user_id: uid,
                role: Role::Owner,
            });
        }
        emit_project_events(project.id(), &events);
        Ok((project, events))
    }

    async fn get_project(&self, id: ProjectId) -> Result<Project> {
        self.backend.get_project(id).await
    }

    async fn get_project_by_name(&self, name: &str) -> Result<Project> {
        self.backend.get_project_by_name(name).await
    }

    async fn update_project(
        &self,
        id: ProjectId,
        params: &UpdateProjectParams,
        caller_user_id: Option<UserId>,
    ) -> Result<(Project, Vec<ProjectEvent>)> {
        params.validate()?;
        if let Some(uid) = caller_user_id {
            require_project_role(self.backend.as_ref(), uid, id, Permission::Admin).await?;
        }
        let prev = self.backend.get_project(id).await?;
        let (_preview, events) = prev.clone().update(params);
        if events.is_empty() {
            // No-op: skip the DB write and return the unchanged project.
            return Ok((prev, events));
        }
        let updated = self.backend.update_project(id, params).await?;
        emit_project_events(id, &events);
        Ok((updated, events))
    }

    async fn delete_project(&self, id: ProjectId, caller_user_id: Option<UserId>) -> Result<()> {
        if let Some(uid) = caller_user_id {
            require_project_role(self.backend.as_ref(), uid, id, Permission::Admin).await?;
        }
        let project = self.backend.get_project(id).await?;
        let tasks = self
            .backend
            .list_tasks(id, &ListTasksFilter::default())
            .await?
            .items;
        project.validate_deletable(tasks.len() as i64)?;
        self.backend.delete_project(id).await
    }

    // --- Member management ---

    async fn list_project_members(
        &self,
        project_id: ProjectId,
        filter: &ListProjectMembersFilter,
    ) -> Result<ListPage<ProjectMember>> {
        self.backend.list_project_members(project_id, filter).await
    }

    async fn add_project_member(
        &self,
        project_id: ProjectId,
        params: &AddProjectMemberParams,
        caller_user_id: Option<UserId>,
    ) -> Result<(ProjectMember, Vec<ProjectEvent>)> {
        if let Some(uid) = caller_user_id {
            require_project_role(self.backend.as_ref(), uid, project_id, Permission::Admin).await?;
        }
        let member = self.backend.add_project_member(project_id, params).await?;
        let events = vec![ProjectEvent::MemberAdded {
            user_id: member.user_id(),
            role: member.role(),
        }];
        emit_project_events(project_id, &events);
        Ok((member, events))
    }

    async fn remove_project_member(
        &self,
        project_id: ProjectId,
        user_id: UserId,
        caller_user_id: Option<UserId>,
    ) -> Result<Vec<ProjectEvent>> {
        if let Some(uid) = caller_user_id {
            require_project_role(self.backend.as_ref(), uid, project_id, Permission::Admin).await?;
        }
        self.backend
            .remove_project_member(project_id, user_id)
            .await?;
        let events = vec![ProjectEvent::MemberRemoved { user_id }];
        emit_project_events(project_id, &events);
        Ok(events)
    }

    async fn get_project_member(
        &self,
        project_id: ProjectId,
        user_id: UserId,
    ) -> Result<ProjectMember> {
        self.backend.get_project_member(project_id, user_id).await
    }

    async fn update_member_role(
        &self,
        project_id: ProjectId,
        user_id: UserId,
        role: Role,
        caller_user_id: Option<UserId>,
    ) -> Result<(ProjectMember, Vec<ProjectEvent>)> {
        if let Some(uid) = caller_user_id {
            require_project_role(self.backend.as_ref(), uid, project_id, Permission::Admin).await?;
        }
        let prev = self.backend.get_project_member(project_id, user_id).await?;
        let from_role = prev.role();
        let member = self
            .backend
            .update_member_role(project_id, user_id, role)
            .await?;
        let events = if from_role == role {
            vec![]
        } else {
            vec![ProjectEvent::MemberRoleChanged {
                user_id,
                from_role,
                to_role: role,
            }]
        };
        emit_project_events(project_id, &events);
        Ok((member, events))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::port::TaskBackend;
    use crate::domain::user::{CreateUserParams, User, Username};
    use crate::infra::sqlite::SqliteBackend;

    const DEFAULT_PROJECT: ProjectId = ProjectId(1);

    fn new_backend() -> Arc<dyn TaskBackend> {
        Arc::new(SqliteBackend::new_in_memory().unwrap())
    }

    async fn create_user(backend: &Arc<dyn TaskBackend>, name: &str) -> User {
        backend
            .create_user(&CreateUserParams {
                username: Username(name.to_string()),
                sub: None,
                display_name: None,
                email: None,
            })
            .await
            .unwrap()
    }

    fn project_params() -> CreateProjectParams {
        CreateProjectParams {
            name: "alpha".to_string(),
            description: Some("initial description".to_string()),
        }
    }

    #[tokio::test]
    async fn create_project_emits_created_and_member_added_when_caller_present() {
        let backend = new_backend();
        let svc = ProjectService::new(backend.clone());
        let owner = create_user(&backend, "owner").await;

        let (project, events) = svc
            .create_project(&project_params(), Some(owner.id()))
            .await
            .unwrap();
        assert_eq!(project.name(), "alpha");
        assert_eq!(
            events,
            vec![
                ProjectEvent::Created,
                ProjectEvent::MemberAdded {
                    user_id: owner.id(),
                    role: Role::Owner,
                },
            ]
        );
    }

    #[tokio::test]
    async fn create_project_without_caller_emits_only_created() {
        let backend = new_backend();
        let svc = ProjectService::new(backend);

        let (_project, events) = svc.create_project(&project_params(), None).await.unwrap();
        assert_eq!(events, vec![ProjectEvent::Created]);
    }

    #[tokio::test]
    async fn list_projects_forwards_caller_user_id() {
        let backend = new_backend();
        let svc = ProjectService::new(backend.clone());
        let alice = create_user(&backend, "alice").await;
        let bob = create_user(&backend, "bob").await;

        // Default project (id 1) has no members in this fixture; create two
        // additional projects, each owned by a different user.
        let (alice_project, _) = svc
            .create_project(
                &CreateProjectParams {
                    name: "alice-only".to_string(),
                    description: None,
                },
                Some(alice.id()),
            )
            .await
            .unwrap();
        let (_bob_project, _) = svc
            .create_project(
                &CreateProjectParams {
                    name: "bob-only".to_string(),
                    description: None,
                },
                Some(bob.id()),
            )
            .await
            .unwrap();

        // Caller scoped to Alice: only Alice's project comes back.
        let scoped = svc
            .list_projects(&ListProjectsFilter::default(), Some(alice.id()))
            .await
            .unwrap();
        let names: Vec<&str> = scoped.items.iter().map(|p| p.name()).collect();
        assert_eq!(names, vec!["alice-only"]);
        assert_eq!(scoped.items[0].id(), alice_project.id());

        // None caller (master / no-auth): everything visible.
        let unscoped = svc
            .list_projects(&ListProjectsFilter::default(), None)
            .await
            .unwrap();
        let unscoped_names: Vec<&str> = unscoped.items.iter().map(|p| p.name()).collect();
        assert!(unscoped_names.contains(&"default"));
        assert!(unscoped_names.contains(&"alice-only"));
        assert!(unscoped_names.contains(&"bob-only"));
    }

    #[tokio::test]
    async fn update_project_emits_updated_with_changed_fields() {
        let backend = new_backend();
        let svc = ProjectService::new(backend);

        let params = UpdateProjectParams {
            description: Some(Some("Replaced description".to_string())),
        };
        let (project, events) = svc
            .update_project(DEFAULT_PROJECT, &params, None)
            .await
            .unwrap();
        assert_eq!(project.description(), Some("Replaced description"));
        assert_eq!(
            events,
            vec![ProjectEvent::Updated {
                changed_fields: vec!["description".to_string()],
            }]
        );
    }

    #[tokio::test]
    async fn update_project_no_op_emits_no_event() {
        let backend = new_backend();
        let svc = ProjectService::new(backend);

        // Default project description is "Default project". Pass the same value.
        let params = UpdateProjectParams {
            description: Some(Some("Default project".to_string())),
        };
        let (_project, events) = svc
            .update_project(DEFAULT_PROJECT, &params, None)
            .await
            .unwrap();
        assert!(events.is_empty(), "no-op update should emit no event");
    }

    #[tokio::test]
    async fn add_member_emits_member_added() {
        let backend = new_backend();
        let svc = ProjectService::new(backend.clone());
        let user = create_user(&backend, "newbie").await;

        let params = AddProjectMemberParams::new(user.id(), Some(Role::Member));
        let (member, events) = svc
            .add_project_member(DEFAULT_PROJECT, &params, None)
            .await
            .unwrap();
        assert_eq!(member.user_id(), user.id());
        assert_eq!(
            events,
            vec![ProjectEvent::MemberAdded {
                user_id: user.id(),
                role: Role::Member,
            }]
        );
    }

    #[tokio::test]
    async fn remove_member_emits_member_removed() {
        let backend = new_backend();
        let svc = ProjectService::new(backend.clone());
        let user = create_user(&backend, "leaver").await;

        let params = AddProjectMemberParams::new(user.id(), Some(Role::Member));
        svc.add_project_member(DEFAULT_PROJECT, &params, None)
            .await
            .unwrap();

        let events = svc
            .remove_project_member(DEFAULT_PROJECT, user.id(), None)
            .await
            .unwrap();
        assert_eq!(
            events,
            vec![ProjectEvent::MemberRemoved { user_id: user.id() }]
        );
    }

    #[tokio::test]
    async fn update_member_role_emits_role_changed() {
        let backend = new_backend();
        let svc = ProjectService::new(backend.clone());
        let user = create_user(&backend, "promoted").await;

        let params = AddProjectMemberParams::new(user.id(), Some(Role::Member));
        svc.add_project_member(DEFAULT_PROJECT, &params, None)
            .await
            .unwrap();

        let (member, events) = svc
            .update_member_role(DEFAULT_PROJECT, user.id(), Role::Owner, None)
            .await
            .unwrap();
        assert_eq!(member.role(), Role::Owner);
        assert_eq!(
            events,
            vec![ProjectEvent::MemberRoleChanged {
                user_id: user.id(),
                from_role: Role::Member,
                to_role: Role::Owner,
            }]
        );
    }

    #[tokio::test]
    async fn update_member_role_no_op_emits_no_event() {
        let backend = new_backend();
        let svc = ProjectService::new(backend.clone());
        let user = create_user(&backend, "stayer").await;

        let params = AddProjectMemberParams::new(user.id(), Some(Role::Member));
        svc.add_project_member(DEFAULT_PROJECT, &params, None)
            .await
            .unwrap();

        let (_member, events) = svc
            .update_member_role(DEFAULT_PROJECT, user.id(), Role::Member, None)
            .await
            .unwrap();
        assert!(events.is_empty(), "no-op role update should emit no event");
    }

    // --- Phase B3: business-event emission wire-up check ------------------
    //
    // Single end-to-end check that `update_project` through
    // `BusinessAttributesProcessor` + `OpenTelemetryTracingBridge` produces
    // a `senko.project.updated` LogRecord with the expected callsite
    // attributes. The macro / processor / bridge interactions are covered
    // exhaustively in `application::telemetry::tests`; this test guards the
    // wire-up only and exercises the shared `test_support` helpers.

    #[tokio::test(flavor = "current_thread")]
    async fn update_project_emits_otel_log_record() {
        use crate::application::telemetry::test_support::{
            build_capture_provider, capture_layer, lookup_attr,
        };
        use opentelemetry::logs::AnyValue;
        use tracing_subscriber::layer::SubscriberExt;

        let backend = new_backend();
        let svc = ProjectService::new(backend);
        let (exporter, provider) = build_capture_provider();
        let subscriber = tracing_subscriber::registry().with(capture_layer(&provider));

        {
            let _guard = tracing::subscriber::set_default(subscriber);
            let params = UpdateProjectParams {
                description: Some(Some("emit-verification".to_string())),
            };
            svc.update_project(DEFAULT_PROJECT, &params, None)
                .await
                .unwrap();
        }

        provider.force_flush().expect("flush ok");
        let logs = exporter.get_emitted_logs().expect("logs exported");
        let updated = logs
            .iter()
            .find(|d| d.record.event_name() == Some("senko.project.updated"))
            .expect("senko.project.updated should be emitted");

        assert_eq!(
            lookup_attr(&updated.record, "senko.project.id"),
            Some(AnyValue::Int(DEFAULT_PROJECT.0))
        );
        assert_eq!(
            lookup_attr(&updated.record, "changed_fields"),
            Some(AnyValue::String("[\"description\"]".into())),
        );
    }
}
