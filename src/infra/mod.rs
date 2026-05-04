pub mod auth;
pub mod config;
pub mod hook;
pub mod http;
pub mod pr_verifier;
pub mod project_root;
pub mod sqlite;
pub mod xdg;

#[cfg(feature = "aws-secrets")]
pub mod secrets;

#[cfg(feature = "postgres")]
pub mod postgres;

/// Escape `%`, `_`, and `\` in a user-supplied substring so it is matched
/// literally inside a SQL `LIKE ... ESCAPE '\'` expression. Both SQLite and
/// PostgreSQL accept this escape clause, so the same helper feeds both
/// repository implementations.
pub(in crate::infra) fn escape_like(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for c in input.chars() {
        match c {
            '\\' | '%' | '_' => {
                out.push('\\');
                out.push(c);
            }
            other => out.push(other),
        }
    }
    out
}

/// Internal newtype for the auto-increment DB primary key of a task row.
///
/// Kept distinct from [`crate::domain::task::TaskId`] (the per-project
/// user-facing `task_number`) so the two identifiers cannot be accidentally
/// interchanged inside the infra layer.
///
/// `pub(in crate::infra)` visibility prevents any code outside the `infra`
/// module tree from naming this type; the domain / application / presentation
/// / remote layers therefore cannot import it even transitively.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(in crate::infra) struct TaskDbId(pub(in crate::infra) i64);

impl rusqlite::ToSql for TaskDbId {
    fn to_sql(&self) -> rusqlite::Result<rusqlite::types::ToSqlOutput<'_>> {
        self.0.to_sql()
    }
}

impl rusqlite::types::FromSql for TaskDbId {
    fn column_result(value: rusqlite::types::ValueRef<'_>) -> rusqlite::types::FromSqlResult<Self> {
        i64::column_result(value).map(TaskDbId)
    }
}

#[cfg(feature = "postgres")]
impl sqlx::Type<sqlx::Postgres> for TaskDbId {
    fn type_info() -> <sqlx::Postgres as sqlx::Database>::TypeInfo {
        <i64 as sqlx::Type<sqlx::Postgres>>::type_info()
    }
}

#[cfg(feature = "postgres")]
impl<'q> sqlx::Encode<'q, sqlx::Postgres> for TaskDbId {
    fn encode_by_ref(
        &self,
        buf: &mut <sqlx::Postgres as sqlx::Database>::ArgumentBuffer<'q>,
    ) -> Result<sqlx::encode::IsNull, sqlx::error::BoxDynError> {
        <i64 as sqlx::Encode<'q, sqlx::Postgres>>::encode_by_ref(&self.0, buf)
    }
}

#[cfg(feature = "postgres")]
impl<'r> sqlx::Decode<'r, sqlx::Postgres> for TaskDbId {
    fn decode(
        value: <sqlx::Postgres as sqlx::Database>::ValueRef<'r>,
    ) -> Result<Self, sqlx::error::BoxDynError> {
        <i64 as sqlx::Decode<'r, sqlx::Postgres>>::decode(value).map(TaskDbId)
    }
}

impl rusqlite::ToSql for crate::domain::project::ProjectId {
    fn to_sql(&self) -> rusqlite::Result<rusqlite::types::ToSqlOutput<'_>> {
        self.0.to_sql()
    }
}

impl rusqlite::types::FromSql for crate::domain::project::ProjectId {
    fn column_result(value: rusqlite::types::ValueRef<'_>) -> rusqlite::types::FromSqlResult<Self> {
        i64::column_result(value).map(crate::domain::project::ProjectId)
    }
}

#[cfg(feature = "postgres")]
impl sqlx::Type<sqlx::Postgres> for crate::domain::project::ProjectId {
    fn type_info() -> <sqlx::Postgres as sqlx::Database>::TypeInfo {
        <i64 as sqlx::Type<sqlx::Postgres>>::type_info()
    }
}

#[cfg(feature = "postgres")]
impl<'q> sqlx::Encode<'q, sqlx::Postgres> for crate::domain::project::ProjectId {
    fn encode_by_ref(
        &self,
        buf: &mut <sqlx::Postgres as sqlx::Database>::ArgumentBuffer<'q>,
    ) -> Result<sqlx::encode::IsNull, sqlx::error::BoxDynError> {
        <i64 as sqlx::Encode<'q, sqlx::Postgres>>::encode_by_ref(&self.0, buf)
    }
}

#[cfg(feature = "postgres")]
impl<'r> sqlx::Decode<'r, sqlx::Postgres> for crate::domain::project::ProjectId {
    fn decode(
        value: <sqlx::Postgres as sqlx::Database>::ValueRef<'r>,
    ) -> Result<Self, sqlx::error::BoxDynError> {
        <i64 as sqlx::Decode<'r, sqlx::Postgres>>::decode(value)
            .map(crate::domain::project::ProjectId)
    }
}

impl rusqlite::ToSql for crate::domain::contract::ContractId {
    fn to_sql(&self) -> rusqlite::Result<rusqlite::types::ToSqlOutput<'_>> {
        self.0.to_sql()
    }
}

impl rusqlite::types::FromSql for crate::domain::contract::ContractId {
    fn column_result(value: rusqlite::types::ValueRef<'_>) -> rusqlite::types::FromSqlResult<Self> {
        i64::column_result(value).map(crate::domain::contract::ContractId)
    }
}

#[cfg(feature = "postgres")]
impl sqlx::Type<sqlx::Postgres> for crate::domain::contract::ContractId {
    fn type_info() -> <sqlx::Postgres as sqlx::Database>::TypeInfo {
        <i64 as sqlx::Type<sqlx::Postgres>>::type_info()
    }
}

#[cfg(feature = "postgres")]
impl<'q> sqlx::Encode<'q, sqlx::Postgres> for crate::domain::contract::ContractId {
    fn encode_by_ref(
        &self,
        buf: &mut <sqlx::Postgres as sqlx::Database>::ArgumentBuffer<'q>,
    ) -> Result<sqlx::encode::IsNull, sqlx::error::BoxDynError> {
        <i64 as sqlx::Encode<'q, sqlx::Postgres>>::encode_by_ref(&self.0, buf)
    }
}

#[cfg(feature = "postgres")]
impl<'r> sqlx::Decode<'r, sqlx::Postgres> for crate::domain::contract::ContractId {
    fn decode(
        value: <sqlx::Postgres as sqlx::Database>::ValueRef<'r>,
    ) -> Result<Self, sqlx::error::BoxDynError> {
        <i64 as sqlx::Decode<'r, sqlx::Postgres>>::decode(value)
            .map(crate::domain::contract::ContractId)
    }
}

impl rusqlite::ToSql for crate::domain::user::UserId {
    fn to_sql(&self) -> rusqlite::Result<rusqlite::types::ToSqlOutput<'_>> {
        self.0.to_sql()
    }
}

impl rusqlite::types::FromSql for crate::domain::user::UserId {
    fn column_result(value: rusqlite::types::ValueRef<'_>) -> rusqlite::types::FromSqlResult<Self> {
        i64::column_result(value).map(crate::domain::user::UserId)
    }
}

#[cfg(feature = "postgres")]
impl sqlx::Type<sqlx::Postgres> for crate::domain::user::UserId {
    fn type_info() -> <sqlx::Postgres as sqlx::Database>::TypeInfo {
        <i64 as sqlx::Type<sqlx::Postgres>>::type_info()
    }
}

#[cfg(feature = "postgres")]
impl<'q> sqlx::Encode<'q, sqlx::Postgres> for crate::domain::user::UserId {
    fn encode_by_ref(
        &self,
        buf: &mut <sqlx::Postgres as sqlx::Database>::ArgumentBuffer<'q>,
    ) -> Result<sqlx::encode::IsNull, sqlx::error::BoxDynError> {
        <i64 as sqlx::Encode<'q, sqlx::Postgres>>::encode_by_ref(&self.0, buf)
    }
}

#[cfg(feature = "postgres")]
impl<'r> sqlx::Decode<'r, sqlx::Postgres> for crate::domain::user::UserId {
    fn decode(
        value: <sqlx::Postgres as sqlx::Database>::ValueRef<'r>,
    ) -> Result<Self, sqlx::error::BoxDynError> {
        <i64 as sqlx::Decode<'r, sqlx::Postgres>>::decode(value).map(crate::domain::user::UserId)
    }
}

impl rusqlite::ToSql for crate::domain::user::Username {
    fn to_sql(&self) -> rusqlite::Result<rusqlite::types::ToSqlOutput<'_>> {
        self.0.to_sql()
    }
}

impl rusqlite::types::FromSql for crate::domain::user::Username {
    fn column_result(value: rusqlite::types::ValueRef<'_>) -> rusqlite::types::FromSqlResult<Self> {
        String::column_result(value).map(crate::domain::user::Username)
    }
}

#[cfg(feature = "postgres")]
impl sqlx::Type<sqlx::Postgres> for crate::domain::user::Username {
    fn type_info() -> <sqlx::Postgres as sqlx::Database>::TypeInfo {
        <String as sqlx::Type<sqlx::Postgres>>::type_info()
    }
}

#[cfg(feature = "postgres")]
impl<'q> sqlx::Encode<'q, sqlx::Postgres> for crate::domain::user::Username {
    fn encode_by_ref(
        &self,
        buf: &mut <sqlx::Postgres as sqlx::Database>::ArgumentBuffer<'q>,
    ) -> Result<sqlx::encode::IsNull, sqlx::error::BoxDynError> {
        <String as sqlx::Encode<'q, sqlx::Postgres>>::encode_by_ref(&self.0, buf)
    }
}

#[cfg(feature = "postgres")]
impl<'r> sqlx::Decode<'r, sqlx::Postgres> for crate::domain::user::Username {
    fn decode(
        value: <sqlx::Postgres as sqlx::Database>::ValueRef<'r>,
    ) -> Result<Self, sqlx::error::BoxDynError> {
        <String as sqlx::Decode<'r, sqlx::Postgres>>::decode(value)
            .map(crate::domain::user::Username)
    }
}
