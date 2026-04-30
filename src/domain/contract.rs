use std::fmt;
use std::str::FromStr;

use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use super::error::DomainError;
use super::project::ProjectId;
use super::task::{DodItem, MetadataUpdate, TaskId, shallow_merge_metadata};
use super::validator::{
    MAX_ITEMS_COUNT, MAX_LONG_TEXT_LEN, MAX_SHORT_TEXT_LEN, MAX_TAG_LEN, MAX_TAGS_COUNT,
    MAX_TITLE_LEN, validate_metadata, validate_optional_nullable_string_length,
    validate_optional_string_length, validate_string_length, validate_string_vec_items,
};

/// Newtype wrapper around the contract identifier.
///
/// Wraps `i64` with `#[serde(transparent)]` so that the JSON wire format stays a
/// bare integer (e.g. `6`), not `{"0": 6}`. The goal is compile-time safety: a
/// `ContractId` cannot be accidentally mixed with a `TaskId`, `ProjectId`, or
/// `user_id` that also happen to be `i64`.
///
/// Like [`ProjectId`] (and unlike [`crate::domain::task::TaskId`], which has a
/// distinct `TaskDbId` sealed inside `infra`), `ContractId` is the DB primary
/// key itself. The infrastructure layer implements `rusqlite` / `sqlx` traits
/// directly on `ContractId` (see `src/infra/mod.rs`), so no separate sealed
/// newtype is needed.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    Ord,
    PartialOrd,
    Serialize,
    Deserialize,
    utoipa::ToSchema,
)]
#[serde(transparent)]
#[schema(value_type = i64)]
pub struct ContractId(pub i64);

impl fmt::Display for ContractId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl FromStr for ContractId {
    type Err = std::num::ParseIntError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        s.parse::<i64>().map(ContractId)
    }
}

impl From<i64> for ContractId {
    fn from(n: i64) -> Self {
        ContractId(n)
    }
}

impl From<ContractId> for i64 {
    fn from(id: ContractId) -> i64 {
        id.0
    }
}

// --- Domain events ---

/// Domain event emitted by Contract aggregate methods.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ContractEvent {
    Created,
    Updated { changed_fields: Vec<String> },
    Deleted,
    DodChecked { index: usize },
    DodUnchecked { index: usize },
    NoteAdded,
}

// --- ContractNote value object ---

/// A timestamped note attached to a Contract. Notes are append-only and never modified.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContractNote {
    content: String,
    source_task_id: Option<TaskId>,
    created_at: String,
}

impl ContractNote {
    pub fn new(content: String, source_task_id: Option<TaskId>, created_at: String) -> Self {
        Self {
            content,
            source_task_id,
            created_at,
        }
    }

    pub fn content(&self) -> &str {
        &self.content
    }

    pub fn source_task_id(&self) -> Option<TaskId> {
        self.source_task_id
    }

    pub fn created_at(&self) -> &str {
        &self.created_at
    }

    pub fn validate(&self) -> Result<(), DomainError> {
        validate_string_length("content", &self.content, MAX_LONG_TEXT_LEN)
    }
}

// --- Contract aggregate ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Contract {
    id: ContractId,
    project_id: ProjectId,
    title: String,
    description: Option<String>,
    definition_of_done: Vec<DodItem>,
    tags: Vec<String>,
    metadata: Option<serde_json::Value>,
    notes: Vec<ContractNote>,
    created_at: String,
    updated_at: String,
}

impl Contract {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: ContractId,
        project_id: ProjectId,
        title: String,
        description: Option<String>,
        definition_of_done: Vec<DodItem>,
        tags: Vec<String>,
        metadata: Option<serde_json::Value>,
        notes: Vec<ContractNote>,
        created_at: String,
        updated_at: String,
    ) -> Self {
        Self {
            id,
            project_id,
            title,
            description,
            definition_of_done,
            tags,
            metadata,
            notes,
            created_at,
            updated_at,
        }
    }

    // --- Getters ---

    pub fn id(&self) -> ContractId {
        self.id
    }

    pub fn project_id(&self) -> ProjectId {
        self.project_id
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn description(&self) -> Option<&str> {
        self.description.as_deref()
    }

    pub fn definition_of_done(&self) -> &[DodItem] {
        &self.definition_of_done
    }

    pub fn tags(&self) -> &[String] {
        &self.tags
    }

    pub fn metadata(&self) -> Option<&serde_json::Value> {
        self.metadata.as_ref()
    }

    pub fn notes(&self) -> &[ContractNote] {
        &self.notes
    }

    pub fn created_at(&self) -> &str {
        &self.created_at
    }

    pub fn updated_at(&self) -> &str {
        &self.updated_at
    }

    // --- Query methods ---

    /// Returns true when the contract has at least one DoD item and all items are checked.
    /// An empty DoD is not considered completed, because completion cannot be evaluated.
    pub fn is_completed(&self) -> bool {
        !self.definition_of_done.is_empty() && self.definition_of_done.iter().all(|d| d.checked())
    }

    // --- Aggregate methods ---

    /// Apply scalar-field updates. Emits `Updated` when any field changed.
    pub fn update(
        mut self,
        params: &UpdateContractParams,
        now: String,
    ) -> (Contract, Vec<ContractEvent>) {
        let mut changed_fields = Vec::new();
        if let Some(ref title) = params.title {
            self.title = title.clone();
            changed_fields.push("title".to_string());
        }
        if let Some(ref description) = params.description {
            self.description = description.clone();
            changed_fields.push("description".to_string());
        }
        if let Some(ref meta_update) = params.metadata {
            match meta_update {
                MetadataUpdate::Clear => {
                    self.metadata = None;
                }
                MetadataUpdate::Merge(patch) => {
                    self.metadata = shallow_merge_metadata(self.metadata.as_ref(), patch);
                }
                MetadataUpdate::Replace(value) => {
                    self.metadata = Some(value.clone());
                }
            }
            changed_fields.push("metadata".to_string());
        }
        if !changed_fields.is_empty() {
            self.updated_at = now;
            (self, vec![ContractEvent::Updated { changed_fields }])
        } else {
            (self, vec![])
        }
    }

    /// Apply array-field updates (tags, definition_of_done). Emits `Updated` on any change.
    pub fn apply_array_update(
        mut self,
        params: &UpdateContractArrayParams,
        now: String,
    ) -> (Contract, Vec<ContractEvent>) {
        let mut tags_changed = false;
        let mut dod_changed = false;

        if let Some(ref set_tags) = params.set_tags {
            self.tags = set_tags.clone();
            tags_changed = true;
        }
        if !params.add_tags.is_empty() {
            for tag in &params.add_tags {
                if !self.tags.contains(tag) {
                    self.tags.push(tag.clone());
                }
            }
            tags_changed = true;
        }
        if !params.remove_tags.is_empty() {
            self.tags.retain(|t| !params.remove_tags.contains(t));
            tags_changed = true;
        }

        if let Some(ref set_dod) = params.set_definition_of_done {
            self.definition_of_done = set_dod
                .iter()
                .map(|c| DodItem::new(c.clone(), false))
                .collect();
            dod_changed = true;
        }
        if !params.add_definition_of_done.is_empty() {
            for content in &params.add_definition_of_done {
                self.definition_of_done
                    .push(DodItem::new(content.clone(), false));
            }
            dod_changed = true;
        }
        if !params.remove_definition_of_done.is_empty() {
            self.definition_of_done.retain(|d| {
                !params
                    .remove_definition_of_done
                    .contains(&d.content().to_string())
            });
            dod_changed = true;
        }

        let mut changed_fields = Vec::new();
        if tags_changed {
            changed_fields.push("tags".to_string());
        }
        if dod_changed {
            changed_fields.push("definition_of_done".to_string());
        }
        if !changed_fields.is_empty() {
            self.updated_at = now;
            (self, vec![ContractEvent::Updated { changed_fields }])
        } else {
            (self, vec![])
        }
    }

    /// Append a note to the contract.
    pub fn add_note(mut self, note: ContractNote, now: String) -> (Contract, Vec<ContractEvent>) {
        self.notes.push(note);
        self.updated_at = now;
        (self, vec![ContractEvent::NoteAdded])
    }

    /// Check a DoD item by 1-based index.
    pub fn check_dod(
        mut self,
        index: usize,
        now: String,
    ) -> Result<(Contract, Vec<ContractEvent>)> {
        if index == 0 || index > self.definition_of_done.len() {
            return Err(DomainError::DodIndexOutOfRange {
                index,
                task_id: self.id.into(),
                count: self.definition_of_done.len(),
            }
            .into());
        }
        let item = &self.definition_of_done[index - 1];
        self.definition_of_done[index - 1] = DodItem::new(item.content().to_string(), true);
        self.updated_at = now;
        Ok((self, vec![ContractEvent::DodChecked { index }]))
    }

    /// Uncheck a DoD item by 1-based index.
    pub fn uncheck_dod(
        mut self,
        index: usize,
        now: String,
    ) -> Result<(Contract, Vec<ContractEvent>)> {
        if index == 0 || index > self.definition_of_done.len() {
            return Err(DomainError::DodIndexOutOfRange {
                index,
                task_id: self.id.into(),
                count: self.definition_of_done.len(),
            }
            .into());
        }
        let item = &self.definition_of_done[index - 1];
        self.definition_of_done[index - 1] = DodItem::new(item.content().to_string(), false);
        self.updated_at = now;
        Ok((self, vec![ContractEvent::DodUnchecked { index }]))
    }
}

// --- Parameters ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateContractParams {
    pub title: String,
    pub description: Option<String>,
    #[serde(default)]
    pub definition_of_done: Vec<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub metadata: Option<serde_json::Value>,
}

impl CreateContractParams {
    pub fn validate(&self) -> Result<(), DomainError> {
        validate_string_length("title", &self.title, MAX_TITLE_LEN)?;
        validate_optional_string_length("description", &self.description, MAX_LONG_TEXT_LEN)?;
        validate_string_vec_items(
            "definition_of_done",
            &self.definition_of_done,
            MAX_SHORT_TEXT_LEN,
            MAX_ITEMS_COUNT,
        )?;
        validate_string_vec_items("tags", &self.tags, MAX_TAG_LEN, MAX_TAGS_COUNT)?;
        if let Some(ref meta) = self.metadata {
            validate_metadata(meta)?;
        }
        Ok(())
    }
}

#[derive(Clone, Default)]
pub struct UpdateContractParams {
    pub title: Option<String>,
    pub description: Option<Option<String>>,
    pub metadata: Option<MetadataUpdate>,
}

impl UpdateContractParams {
    pub fn validate(&self) -> Result<(), DomainError> {
        if let Some(ref title) = self.title {
            validate_string_length("title", title, MAX_TITLE_LEN)?;
        }
        validate_optional_nullable_string_length(
            "description",
            &self.description,
            MAX_LONG_TEXT_LEN,
        )?;
        if let Some(MetadataUpdate::Replace(ref value)) | Some(MetadataUpdate::Merge(ref value)) =
            self.metadata
        {
            validate_metadata(value)?;
        }
        Ok(())
    }
}

#[derive(Clone, Default)]
pub struct UpdateContractArrayParams {
    pub set_tags: Option<Vec<String>>,
    pub add_tags: Vec<String>,
    pub remove_tags: Vec<String>,
    pub set_definition_of_done: Option<Vec<String>>,
    pub add_definition_of_done: Vec<String>,
    pub remove_definition_of_done: Vec<String>,
}

impl UpdateContractArrayParams {
    pub fn validate(&self) -> Result<(), DomainError> {
        if let Some(ref tags) = self.set_tags {
            validate_string_vec_items("set_tags", tags, MAX_TAG_LEN, MAX_TAGS_COUNT)?;
        }
        validate_string_vec_items("add_tags", &self.add_tags, MAX_TAG_LEN, MAX_TAGS_COUNT)?;
        if let Some(ref dod) = self.set_definition_of_done {
            validate_string_vec_items(
                "set_definition_of_done",
                dod,
                MAX_SHORT_TEXT_LEN,
                MAX_ITEMS_COUNT,
            )?;
        }
        validate_string_vec_items(
            "add_definition_of_done",
            &self.add_definition_of_done,
            MAX_SHORT_TEXT_LEN,
            MAX_ITEMS_COUNT,
        )?;
        Ok(())
    }
}

// --- Repository port ---

// --- List filters ---

/// Filter / paging inputs for `list_contracts`.
#[derive(Clone, Default)]
pub struct ListContractsFilter {
    pub tags: Vec<String>,
    pub limit: Option<u32>,
    pub after: Option<ContractId>,
}

/// Filter / paging inputs for `list_contract_notes`.
#[derive(Clone, Default)]
pub struct ListContractNotesFilter {
    pub limit: Option<u32>,
    /// Cursor payload: the `contract_notes.id` of the last note returned.
    pub after: Option<i64>,
}

// --- Repository port ---

#[async_trait]
pub trait ContractRepository: Send + Sync {
    async fn create_contract(
        &self,
        project_id: ProjectId,
        params: &CreateContractParams,
    ) -> Result<Contract>;

    async fn get_contract(&self, id: ContractId) -> Result<Contract>;

    async fn list_contracts(
        &self,
        project_id: ProjectId,
        filter: &ListContractsFilter,
    ) -> Result<crate::domain::pagination::ListPage<Contract>>;

    async fn update_contract(
        &self,
        id: ContractId,
        update: &UpdateContractParams,
        array_update: &UpdateContractArrayParams,
    ) -> Result<Contract>;

    async fn delete_contract(&self, id: ContractId) -> Result<()>;

    async fn add_note(&self, contract_id: ContractId, note: &ContractNote) -> Result<ContractNote>;

    /// Paginated list of notes for a contract. Ordering: `id ASC`.
    async fn list_contract_notes(
        &self,
        contract_id: ContractId,
        filter: &ListContractNotesFilter,
    ) -> Result<crate::domain::pagination::ListPage<ContractNote>>;

    async fn check_dod(&self, contract_id: ContractId, index: usize) -> Result<Contract>;

    async fn uncheck_dod(&self, contract_id: ContractId, index: usize) -> Result<Contract>;
}

// --- Tests ---

#[cfg(test)]
mod tests {
    use super::*;

    fn make_contract(dod: Vec<DodItem>) -> Contract {
        Contract::new(
            ContractId(1),
            ProjectId(1),
            "test-contract".to_string(),
            Some("desc".to_string()),
            dod,
            vec![],
            None,
            vec![],
            "2026-01-01T00:00:00Z".to_string(),
            "2026-01-01T00:00:00Z".to_string(),
        )
    }

    // --- ContractNote ---

    #[test]
    fn contract_note_new_and_getters() {
        let note = ContractNote::new(
            "hello".to_string(),
            Some(TaskId(42)),
            "2026-01-02T00:00:00Z".to_string(),
        );
        assert_eq!(note.content(), "hello");
        assert_eq!(note.source_task_id(), Some(TaskId(42)));
        assert_eq!(note.created_at(), "2026-01-02T00:00:00Z");
    }

    #[test]
    fn contract_note_validate_ok() {
        let note = ContractNote::new(
            "short".to_string(),
            None,
            "2026-01-01T00:00:00Z".to_string(),
        );
        assert!(note.validate().is_ok());
    }

    #[test]
    fn contract_note_validate_too_long() {
        let note = ContractNote::new(
            "x".repeat(MAX_LONG_TEXT_LEN + 1),
            None,
            "2026-01-01T00:00:00Z".to_string(),
        );
        assert!(note.validate().is_err());
    }

    #[test]
    fn contract_note_serde_roundtrip() {
        let note = ContractNote::new(
            "n".to_string(),
            Some(TaskId(9)),
            "2026-01-01T00:00:00Z".to_string(),
        );
        let json = serde_json::to_string(&note).unwrap();
        let parsed: ContractNote = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, note);
    }

    // --- is_completed ---

    #[test]
    fn is_completed_empty_dod_is_false() {
        let c = make_contract(vec![]);
        assert!(!c.is_completed());
    }

    #[test]
    fn is_completed_partial_checked_is_false() {
        let c = make_contract(vec![
            DodItem::new("a".to_string(), true),
            DodItem::new("b".to_string(), false),
        ]);
        assert!(!c.is_completed());
    }

    #[test]
    fn is_completed_all_checked_is_true() {
        let c = make_contract(vec![
            DodItem::new("a".to_string(), true),
            DodItem::new("b".to_string(), true),
        ]);
        assert!(c.is_completed());
    }

    // --- DoD check/uncheck ---

    #[test]
    fn check_dod_valid_index() {
        let c = make_contract(vec![
            DodItem::new("a".to_string(), false),
            DodItem::new("b".to_string(), false),
        ]);
        let (c, events) = c.check_dod(1, "2026-01-02T00:00:00Z".to_string()).unwrap();
        assert_eq!(events, vec![ContractEvent::DodChecked { index: 1 }]);
        assert!(c.definition_of_done()[0].checked());
        assert!(!c.definition_of_done()[1].checked());
        assert_eq!(c.updated_at(), "2026-01-02T00:00:00Z");
    }

    #[test]
    fn check_dod_index_zero_errors() {
        let c = make_contract(vec![DodItem::new("a".to_string(), false)]);
        assert!(c.check_dod(0, "t".to_string()).is_err());
    }

    #[test]
    fn check_dod_out_of_range_errors() {
        let c = make_contract(vec![DodItem::new("a".to_string(), false)]);
        assert!(c.check_dod(2, "t".to_string()).is_err());
    }

    #[test]
    fn uncheck_dod_valid_index() {
        let c = make_contract(vec![DodItem::new("a".to_string(), true)]);
        let (c, events) = c
            .uncheck_dod(1, "2026-01-02T00:00:00Z".to_string())
            .unwrap();
        assert_eq!(events, vec![ContractEvent::DodUnchecked { index: 1 }]);
        assert!(!c.definition_of_done()[0].checked());
    }

    // --- add_note ---

    #[test]
    fn add_note_appends_and_updates_timestamp() {
        let c = make_contract(vec![]);
        let note = ContractNote::new(
            "n".to_string(),
            Some(TaskId(10)),
            "2026-01-02T00:00:00Z".to_string(),
        );
        let (c, events) = c.add_note(note.clone(), "2026-01-02T00:00:00Z".to_string());
        assert_eq!(events, vec![ContractEvent::NoteAdded]);
        assert_eq!(c.notes().len(), 1);
        assert_eq!(c.notes()[0], note);
        assert_eq!(c.updated_at(), "2026-01-02T00:00:00Z");
    }

    // --- update ---

    #[test]
    fn update_title_emits_updated_event() {
        let c = make_contract(vec![]);
        let params = UpdateContractParams {
            title: Some("new-title".to_string()),
            ..Default::default()
        };
        let (c, events) = c.update(&params, "2026-01-02T00:00:00Z".to_string());
        assert_eq!(
            events,
            vec![ContractEvent::Updated {
                changed_fields: vec!["title".into()],
            }]
        );
        assert_eq!(c.title(), "new-title");
        assert_eq!(c.updated_at(), "2026-01-02T00:00:00Z");
    }

    #[test]
    fn update_description_to_none_emits_event() {
        let c = make_contract(vec![]);
        let params = UpdateContractParams {
            description: Some(None),
            ..Default::default()
        };
        let (c, events) = c.update(&params, "2026-01-02T00:00:00Z".to_string());
        assert_eq!(
            events,
            vec![ContractEvent::Updated {
                changed_fields: vec!["description".into()],
            }]
        );
        assert_eq!(c.description(), None);
    }

    #[test]
    fn update_no_changes_no_event() {
        let c = make_contract(vec![]);
        let params = UpdateContractParams::default();
        let (c, events) = c.update(&params, "2026-01-02T00:00:00Z".to_string());
        assert!(events.is_empty());
        assert_eq!(c.updated_at(), "2026-01-01T00:00:00Z");
    }

    #[test]
    fn update_metadata_merge() {
        let mut c = make_contract(vec![]);
        c.metadata = Some(serde_json::json!({"a": 1, "b": 2}));
        let params = UpdateContractParams {
            metadata: Some(MetadataUpdate::Merge(serde_json::json!({"b": 3, "c": 4}))),
            ..Default::default()
        };
        let (c, events) = c.update(&params, "2026-01-02T00:00:00Z".to_string());
        assert_eq!(
            events,
            vec![ContractEvent::Updated {
                changed_fields: vec!["metadata".into()],
            }]
        );
        assert_eq!(
            c.metadata(),
            Some(&serde_json::json!({"a": 1, "b": 3, "c": 4}))
        );
    }

    // --- apply_array_update ---

    #[test]
    fn array_update_set_tags() {
        let c = make_contract(vec![]);
        let params = UpdateContractArrayParams {
            set_tags: Some(vec!["x".to_string(), "y".to_string()]),
            ..Default::default()
        };
        let (c, events) = c.apply_array_update(&params, "2026-01-02T00:00:00Z".to_string());
        assert_eq!(
            events,
            vec![ContractEvent::Updated {
                changed_fields: vec!["tags".into()],
            }]
        );
        assert_eq!(c.tags(), &["x".to_string(), "y".to_string()]);
    }

    #[test]
    fn array_update_set_dod_resets_checked() {
        let c = make_contract(vec![DodItem::new("old".to_string(), true)]);
        let params = UpdateContractArrayParams {
            set_definition_of_done: Some(vec!["new1".to_string(), "new2".to_string()]),
            ..Default::default()
        };
        let (c, events) = c.apply_array_update(&params, "2026-01-02T00:00:00Z".to_string());
        assert_eq!(
            events,
            vec![ContractEvent::Updated {
                changed_fields: vec!["definition_of_done".into()],
            }]
        );
        assert_eq!(c.definition_of_done().len(), 2);
        assert!(!c.definition_of_done()[0].checked());
        assert!(!c.definition_of_done()[1].checked());
    }

    #[test]
    fn array_update_remove_dod_by_content() {
        let c = make_contract(vec![
            DodItem::new("keep".to_string(), false),
            DodItem::new("drop".to_string(), false),
        ]);
        let params = UpdateContractArrayParams {
            remove_definition_of_done: vec!["drop".to_string()],
            ..Default::default()
        };
        let (c, _) = c.apply_array_update(&params, "2026-01-02T00:00:00Z".to_string());
        assert_eq!(c.definition_of_done().len(), 1);
        assert_eq!(c.definition_of_done()[0].content(), "keep");
    }

    #[test]
    fn array_update_empty_no_event() {
        let c = make_contract(vec![]);
        let params = UpdateContractArrayParams::default();
        let (_, events) = c.apply_array_update(&params, "2026-01-02T00:00:00Z".to_string());
        assert!(events.is_empty());
    }

    // --- Params validation ---

    #[test]
    fn create_params_validate_ok() {
        let p = CreateContractParams {
            title: "t".to_string(),
            description: Some("d".to_string()),
            definition_of_done: vec!["a".to_string()],
            tags: vec!["x".to_string()],
            metadata: Some(serde_json::json!({"k": "v"})),
        };
        assert!(p.validate().is_ok());
    }

    #[test]
    fn create_params_validate_title_too_long() {
        let p = CreateContractParams {
            title: "x".repeat(MAX_TITLE_LEN + 1),
            description: None,
            definition_of_done: vec![],
            tags: vec![],
            metadata: None,
        };
        assert!(p.validate().is_err());
    }

    #[test]
    fn create_params_validate_too_many_tags() {
        let p = CreateContractParams {
            title: "t".to_string(),
            description: None,
            definition_of_done: vec![],
            tags: (0..=MAX_TAGS_COUNT).map(|i| format!("t{i}")).collect(),
            metadata: None,
        };
        assert!(p.validate().is_err());
    }

    #[test]
    fn update_params_validate_title_too_long() {
        let p = UpdateContractParams {
            title: Some("x".repeat(MAX_TITLE_LEN + 1)),
            ..Default::default()
        };
        assert!(p.validate().is_err());
    }

    #[test]
    fn update_array_params_validate_too_many_dod() {
        let p = UpdateContractArrayParams {
            add_definition_of_done: (0..=MAX_ITEMS_COUNT).map(|i| format!("d{i}")).collect(),
            ..Default::default()
        };
        assert!(p.validate().is_err());
    }

    // --- Serde ---

    #[test]
    fn contract_serde_roundtrip() {
        let c = make_contract(vec![DodItem::new("a".to_string(), false)]);
        let json = serde_json::to_string(&c).unwrap();
        let parsed: Contract = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.id(), c.id());
        assert_eq!(parsed.title(), c.title());
        assert_eq!(parsed.definition_of_done(), c.definition_of_done());
    }
}
