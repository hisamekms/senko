use std::fmt;
use std::str::FromStr;

use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use super::error::DomainError;
use super::project::ProjectId;

// --- MetadataFieldType enum ---

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MetadataFieldType {
    String,
    Number,
    Boolean,
}

impl fmt::Display for MetadataFieldType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            MetadataFieldType::String => "string",
            MetadataFieldType::Number => "number",
            MetadataFieldType::Boolean => "boolean",
        };
        write!(f, "{s}")
    }
}

impl FromStr for MetadataFieldType {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "string" => Ok(MetadataFieldType::String),
            "number" => Ok(MetadataFieldType::Number),
            "boolean" => Ok(MetadataFieldType::Boolean),
            _ => Err(DomainError::InvalidMetadataFieldType {
                value: s.to_string(),
            }
            .into()),
        }
    }
}

// --- Domain events ---

/// Domain event emitted by MetadataField service mutations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum MetadataFieldEvent {
    Defined {
        field_name: String,
        field_type: MetadataFieldType,
    },
    Removed {
        field_name: String,
        field_type: MetadataFieldType,
    },
}

// --- MetadataField entity ---

/// Maximum length for a metadata field name.
pub const METADATA_FIELD_NAME_MAX_LEN: usize = 64;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetadataField {
    id: i64,
    project_id: ProjectId,
    name: String,
    field_type: MetadataFieldType,
    required_on_complete: bool,
    description: Option<String>,
    created_at: String,
}

impl MetadataField {
    pub fn new(
        id: i64,
        project_id: ProjectId,
        name: String,
        field_type: MetadataFieldType,
        required_on_complete: bool,
        description: Option<String>,
        created_at: String,
    ) -> Self {
        Self {
            id,
            project_id,
            name,
            field_type,
            required_on_complete,
            description,
            created_at,
        }
    }

    pub fn id(&self) -> i64 {
        self.id
    }

    pub fn project_id(&self) -> ProjectId {
        self.project_id
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn field_type(&self) -> MetadataFieldType {
        self.field_type
    }

    pub fn required_on_complete(&self) -> bool {
        self.required_on_complete
    }

    pub fn description(&self) -> Option<&str> {
        self.description.as_deref()
    }

    pub fn created_at(&self) -> &str {
        &self.created_at
    }
}

// --- Validation ---

pub fn validate_field_name(name: &str) -> Result<(), DomainError> {
    if name.is_empty() {
        return Err(DomainError::InvalidMetadataFieldName {
            reason: "field name must not be empty".to_string(),
        });
    }
    if name.len() > METADATA_FIELD_NAME_MAX_LEN {
        return Err(DomainError::InvalidMetadataFieldName {
            reason: format!(
                "field name must not exceed {} characters (got {})",
                METADATA_FIELD_NAME_MAX_LEN,
                name.len()
            ),
        });
    }
    if !name.starts_with(|c: char| c.is_ascii_lowercase()) {
        return Err(DomainError::InvalidMetadataFieldName {
            reason: "field name must start with a lowercase letter".to_string(),
        });
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
    {
        return Err(DomainError::InvalidMetadataFieldName {
            reason:
                "field name must contain only lowercase letters, digits, underscores, and hyphens"
                    .to_string(),
        });
    }
    Ok(())
}

// --- Params ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateMetadataFieldParams {
    pub name: String,
    pub field_type: MetadataFieldType,
    #[serde(default)]
    pub required_on_complete: bool,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateMetadataFieldParams {
    pub required_on_complete: Option<bool>,
    pub description: Option<Option<String>>,
}

// --- Repository trait ---

#[async_trait]
pub trait MetadataFieldRepository: Send + Sync {
    async fn create_metadata_field(
        &self,
        project_id: ProjectId,
        params: &CreateMetadataFieldParams,
    ) -> Result<MetadataField>;

    async fn get_metadata_field(
        &self,
        project_id: ProjectId,
        field_id: i64,
    ) -> Result<MetadataField>;

    async fn list_metadata_fields(
        &self,
        project_id: ProjectId,
        filter: &ListMetadataFieldsFilter,
    ) -> Result<super::pagination::ListPage<MetadataField>>;

    async fn update_metadata_field(
        &self,
        project_id: ProjectId,
        field_id: i64,
        params: &UpdateMetadataFieldParams,
    ) -> Result<MetadataField>;

    async fn delete_metadata_field(&self, project_id: ProjectId, field_id: i64) -> Result<()>;
}

/// Filter / paging inputs for `list_metadata_fields`.
#[derive(Clone, Default)]
pub struct ListMetadataFieldsFilter {
    pub limit: Option<u32>,
    /// Cursor payload: the `metadata_fields.id` of the last row returned.
    pub after: Option<i64>,
}

// --- Tests ---

#[cfg(test)]
mod tests {
    use super::*;

    // MetadataFieldType tests

    #[test]
    fn field_type_display_roundtrip() {
        for (variant, expected) in [
            (MetadataFieldType::String, "string"),
            (MetadataFieldType::Number, "number"),
            (MetadataFieldType::Boolean, "boolean"),
        ] {
            let displayed = variant.to_string();
            assert_eq!(displayed, expected);
            let parsed: MetadataFieldType = displayed.parse().unwrap();
            assert_eq!(parsed, variant);
        }
    }

    #[test]
    fn field_type_serde_roundtrip() {
        for variant in [
            MetadataFieldType::String,
            MetadataFieldType::Number,
            MetadataFieldType::Boolean,
        ] {
            let json = serde_json::to_string(&variant).unwrap();
            let deserialized: MetadataFieldType = serde_json::from_str(&json).unwrap();
            assert_eq!(deserialized, variant);
        }
        // Verify snake_case serialization
        assert_eq!(
            serde_json::to_string(&MetadataFieldType::String).unwrap(),
            "\"string\""
        );
        assert_eq!(
            serde_json::to_string(&MetadataFieldType::Boolean).unwrap(),
            "\"boolean\""
        );
    }

    #[test]
    fn field_type_from_str_case_insensitive() {
        assert_eq!(
            "String".parse::<MetadataFieldType>().unwrap(),
            MetadataFieldType::String
        );
        assert_eq!(
            "STRING".parse::<MetadataFieldType>().unwrap(),
            MetadataFieldType::String
        );
        assert_eq!(
            "NUMBER".parse::<MetadataFieldType>().unwrap(),
            MetadataFieldType::Number
        );
        assert_eq!(
            "Boolean".parse::<MetadataFieldType>().unwrap(),
            MetadataFieldType::Boolean
        );
    }

    #[test]
    fn field_type_from_str_invalid() {
        assert!("int".parse::<MetadataFieldType>().is_err());
        assert!("text".parse::<MetadataFieldType>().is_err());
        assert!("".parse::<MetadataFieldType>().is_err());
        assert!("bool".parse::<MetadataFieldType>().is_err());
    }

    // validate_field_name tests

    #[test]
    fn valid_simple_name() {
        assert!(validate_field_name("sprint").is_ok());
    }

    #[test]
    fn valid_name_with_underscore() {
        assert!(validate_field_name("story_points").is_ok());
    }

    #[test]
    fn valid_name_with_hyphen() {
        assert!(validate_field_name("story-points").is_ok());
    }

    #[test]
    fn valid_name_with_digits() {
        assert!(validate_field_name("field1").is_ok());
        assert!(validate_field_name("v2_value").is_ok());
    }

    #[test]
    fn accepts_max_length_name() {
        let name = "a".repeat(METADATA_FIELD_NAME_MAX_LEN);
        assert!(validate_field_name(&name).is_ok());
    }

    #[test]
    fn rejects_empty_name() {
        let err = validate_field_name("").unwrap_err();
        assert!(matches!(err, DomainError::InvalidMetadataFieldName { .. }));
    }

    #[test]
    fn rejects_too_long_name() {
        let name = "a".repeat(METADATA_FIELD_NAME_MAX_LEN + 1);
        let err = validate_field_name(&name).unwrap_err();
        assert!(matches!(err, DomainError::InvalidMetadataFieldName { .. }));
    }

    #[test]
    fn rejects_name_starting_with_digit() {
        let err = validate_field_name("1field").unwrap_err();
        assert!(matches!(err, DomainError::InvalidMetadataFieldName { .. }));
    }

    #[test]
    fn rejects_name_starting_with_underscore() {
        let err = validate_field_name("_field").unwrap_err();
        assert!(matches!(err, DomainError::InvalidMetadataFieldName { .. }));
    }

    #[test]
    fn rejects_name_with_uppercase() {
        let err = validate_field_name("Sprint").unwrap_err();
        assert!(matches!(err, DomainError::InvalidMetadataFieldName { .. }));

        let err = validate_field_name("FIELD").unwrap_err();
        assert!(matches!(err, DomainError::InvalidMetadataFieldName { .. }));
    }

    #[test]
    fn rejects_name_with_spaces() {
        let err = validate_field_name("my field").unwrap_err();
        assert!(matches!(err, DomainError::InvalidMetadataFieldName { .. }));
    }

    #[test]
    fn rejects_name_with_special_chars() {
        for name in ["field.name", "field@name", "field!"] {
            let err = validate_field_name(name).unwrap_err();
            assert!(
                matches!(err, DomainError::InvalidMetadataFieldName { .. }),
                "expected error for name: {name}"
            );
        }
    }

    // MetadataField entity tests

    #[test]
    fn new_and_getters() {
        let field = MetadataField::new(
            1,
            ProjectId(10),
            "sprint".to_string(),
            MetadataFieldType::String,
            true,
            Some("Sprint name".to_string()),
            "2026-04-12T00:00:00Z".to_string(),
        );
        assert_eq!(field.id(), 1);
        assert_eq!(field.project_id(), ProjectId(10));
        assert_eq!(field.name(), "sprint");
        assert_eq!(field.field_type(), MetadataFieldType::String);
        assert!(field.required_on_complete());
        assert_eq!(field.description(), Some("Sprint name"));
        assert_eq!(field.created_at(), "2026-04-12T00:00:00Z");
    }

    #[test]
    fn new_with_none_description() {
        let field = MetadataField::new(
            2,
            ProjectId(10),
            "points".to_string(),
            MetadataFieldType::Number,
            false,
            None,
            "2026-04-12T00:00:00Z".to_string(),
        );
        assert_eq!(field.description(), None);
        assert!(!field.required_on_complete());
    }
}
