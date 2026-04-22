use crate::application::port::TaskBackend;
use crate::application::port::auth::AuthError;
use crate::domain::project::ProjectId;
use crate::domain::user::{ProjectMember, Role, UserId};

#[derive(Debug, Clone, Copy)]
pub enum Permission {
    View,  // Viewer, Member, Owner
    Edit,  // Member, Owner
    Admin, // Owner only
}

pub async fn require_project_role(
    backend: &dyn TaskBackend,
    user_id: UserId,
    project_id: ProjectId,
    permission: Permission,
) -> std::result::Result<ProjectMember, AuthError> {
    let member = backend
        .get_project_member(project_id, user_id)
        .await
        .map_err(|_| {
            AuthError::Forbidden(format!(
                "user {user_id} is not a member of project {project_id}"
            ))
        })?;

    let allowed = match permission {
        Permission::View => true,
        Permission::Edit => matches!(member.role(), Role::Owner | Role::Member),
        Permission::Admin => matches!(member.role(), Role::Owner),
    };

    if !allowed {
        return Err(AuthError::Forbidden(format!(
            "insufficient permissions: {:?} role cannot perform {:?} operations",
            member.role(),
            permission
        )));
    }

    Ok(member)
}
