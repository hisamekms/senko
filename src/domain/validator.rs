use std::collections::{HashSet, VecDeque};

use super::error::DomainError;
use super::metadata_field::{MetadataField, MetadataFieldType};

// --- Input size limits ---
pub const MAX_TITLE_LEN: usize = 500;
pub const MAX_LONG_TEXT_LEN: usize = 50_000;
pub const MAX_TAG_LEN: usize = 100;
pub const MAX_TAGS_COUNT: usize = 20;
pub const MAX_SHORT_TEXT_LEN: usize = 500;
pub const MAX_ITEMS_COUNT: usize = 50;
pub const MAX_SESSION_ID_LEN: usize = 100;
pub const MAX_USERNAME_LEN: usize = 100;
pub const MAX_DISPLAY_NAME_LEN: usize = 100;
pub const MAX_PROJECT_NAME_LEN: usize = 500;
pub const MAX_PROJECT_DESCRIPTION_LEN: usize = 50_000;

pub fn validate_string_length(field: &str, value: &str, max_len: usize) -> Result<(), DomainError> {
    let len = value.chars().count();
    if len > max_len {
        return Err(DomainError::ValidationError {
            field: field.to_string(),
            message: format!("{field} exceeds maximum length of {max_len} characters (got {len})"),
        });
    }
    Ok(())
}

pub fn validate_optional_string_length(
    field: &str,
    value: &Option<String>,
    max_len: usize,
) -> Result<(), DomainError> {
    if let Some(v) = value {
        validate_string_length(field, v, max_len)?;
    }
    Ok(())
}

pub fn validate_optional_nullable_string_length(
    field: &str,
    value: &Option<Option<String>>,
    max_len: usize,
) -> Result<(), DomainError> {
    if let Some(Some(v)) = value {
        validate_string_length(field, v, max_len)?;
    }
    Ok(())
}

pub fn validate_string_vec_items(
    field: &str,
    items: &[String],
    max_item_len: usize,
    max_count: usize,
) -> Result<(), DomainError> {
    if items.len() > max_count {
        return Err(DomainError::ValidationError {
            field: field.to_string(),
            message: format!(
                "{field} exceeds maximum of {max_count} items (got {})",
                items.len()
            ),
        });
    }
    for (i, item) in items.iter().enumerate() {
        let len = item.chars().count();
        if len > max_item_len {
            return Err(DomainError::ValidationError {
                field: format!("{field}[{i}]"),
                message: format!(
                    "{field}[{i}] exceeds maximum length of {max_item_len} characters (got {len})"
                ),
            });
        }
    }
    Ok(())
}

/// Check if adding dep_id as a dependency of task_id would create a cycle.
/// Performs BFS from dep_id following its dependencies; if task_id is reachable, it's a cycle.
///
/// `get_dependencies` returns the dependency IDs for a given task.
pub fn has_cycle<F>(
    task_id: crate::domain::task::TaskId,
    dep_id: crate::domain::task::TaskId,
    get_dependencies: F,
) -> bool
where
    F: Fn(crate::domain::task::TaskId) -> Vec<crate::domain::task::TaskId>,
{
    let mut visited = HashSet::new();
    let mut queue = VecDeque::new();
    queue.push_back(dep_id);
    visited.insert(dep_id);

    while let Some(current) = queue.pop_front() {
        for d in get_dependencies(current) {
            if d == task_id {
                return true;
            }
            if visited.insert(d) {
                queue.push_back(d);
            }
        }
    }
    false
}

/// Async version of cycle detection for use in the application layer.
pub async fn has_cycle_async<F, Fut>(
    task_id: crate::domain::task::TaskId,
    dep_id: crate::domain::task::TaskId,
    get_dependencies: F,
) -> bool
where
    F: Fn(crate::domain::task::TaskId) -> Fut,
    Fut: std::future::Future<Output = Vec<crate::domain::task::TaskId>>,
{
    let mut visited = HashSet::new();
    let mut queue = VecDeque::new();
    queue.push_back(dep_id);
    visited.insert(dep_id);

    while let Some(current) = queue.pop_front() {
        for d in get_dependencies(current).await {
            if d == task_id {
                return true;
            }
            if visited.insert(d) {
                queue.push_back(d);
            }
        }
    }
    false
}

/// Maximum serialized size for metadata JSON (64 KiB).
pub const METADATA_MAX_SIZE: usize = 65_536;

/// Maximum nesting depth for metadata JSON.
pub const METADATA_MAX_DEPTH: u32 = 10;

/// Validate metadata JSON for size and nesting depth limits.
pub fn validate_metadata(value: &serde_json::Value) -> Result<(), DomainError> {
    let size = serde_json::to_string(value).map(|s| s.len()).unwrap_or(0);
    if size > METADATA_MAX_SIZE {
        return Err(DomainError::MetadataTooLarge {
            size,
            max: METADATA_MAX_SIZE,
        });
    }
    let depth = json_depth(value);
    if depth > METADATA_MAX_DEPTH {
        return Err(DomainError::MetadataTooDeep {
            depth,
            max: METADATA_MAX_DEPTH,
        });
    }
    Ok(())
}

/// Validate that task metadata satisfies required metadata field definitions.
///
/// Checks that all `required_on_complete` fields are present and have the correct type.
/// Returns `Ok(())` if no required fields exist or all are satisfied.
pub fn validate_metadata_on_complete(
    task_metadata: Option<&serde_json::Value>,
    fields: &[MetadataField],
    task_id: crate::domain::task::TaskId,
) -> Result<(), DomainError> {
    let required: Vec<&MetadataField> =
        fields.iter().filter(|f| f.required_on_complete()).collect();

    if required.is_empty() {
        return Ok(());
    }

    let obj = task_metadata.and_then(|v| v.as_object());

    let mut missing = Vec::new();
    let mut type_errors = Vec::new();

    for field in &required {
        let name = field.name();
        match obj.and_then(|m| m.get(name)) {
            None | Some(serde_json::Value::Null) => {
                missing.push(name.to_string());
            }
            Some(value) => {
                let ok = match field.field_type() {
                    MetadataFieldType::String => value.is_string(),
                    MetadataFieldType::Number => value.is_number(),
                    MetadataFieldType::Boolean => value.is_boolean(),
                };
                if !ok {
                    type_errors.push(format!(
                        "{} (expected {}, got {})",
                        name,
                        field.field_type(),
                        json_type_name(value),
                    ));
                }
            }
        }
    }

    if missing.is_empty() && type_errors.is_empty() {
        return Ok(());
    }

    let mut parts = Vec::new();
    if !missing.is_empty() {
        parts.push(format!("missing required field(s): {}", missing.join(", ")));
    }
    if !type_errors.is_empty() {
        parts.push(format!("type mismatch: {}", type_errors.join(", ")));
    }

    Err(DomainError::CannotCompleteTask {
        task_id,
        reason: parts.join("; "),
    })
}

fn json_type_name(value: &serde_json::Value) -> &'static str {
    match value {
        serde_json::Value::String(_) => "string",
        serde_json::Value::Number(_) => "number",
        serde_json::Value::Bool(_) => "boolean",
        serde_json::Value::Array(_) => "array",
        serde_json::Value::Object(_) => "object",
        serde_json::Value::Null => "null",
    }
}

fn json_depth(value: &serde_json::Value) -> u32 {
    match value {
        serde_json::Value::Array(arr) => 1 + arr.iter().map(json_depth).max().unwrap_or(0),
        serde_json::Value::Object(map) => 1 + map.values().map(json_depth).max().unwrap_or(0),
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::project::ProjectId;
    use std::collections::HashMap;

    #[test]
    fn string_at_limit_ok() {
        let s = "a".repeat(MAX_TITLE_LEN);
        assert!(validate_string_length("title", &s, MAX_TITLE_LEN).is_ok());
    }

    #[test]
    fn string_over_limit_err() {
        let s = "a".repeat(MAX_TITLE_LEN + 1);
        let err = validate_string_length("title", &s, MAX_TITLE_LEN).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("501"), "should contain actual length: {msg}");
        assert!(msg.contains("500"), "should contain max length: {msg}");
    }

    #[test]
    fn empty_string_ok() {
        assert!(validate_string_length("title", "", MAX_TITLE_LEN).is_ok());
    }

    #[test]
    fn multibyte_chars_counted_correctly() {
        let s = "あいうえ"; // 4 chars, 12 bytes
        assert!(validate_string_length("test", s, 4).is_ok());
        assert!(validate_string_length("test", s, 3).is_err());
    }

    #[test]
    fn optional_none_ok() {
        assert!(validate_optional_string_length("f", &None, 10).is_ok());
    }

    #[test]
    fn optional_some_over_limit() {
        let v = Some("a".repeat(11));
        assert!(validate_optional_string_length("f", &v, 10).is_err());
    }

    #[test]
    fn nullable_none_ok() {
        assert!(validate_optional_nullable_string_length("f", &None, 10).is_ok());
        assert!(validate_optional_nullable_string_length("f", &Some(None), 10).is_ok());
    }

    #[test]
    fn nullable_some_some_over_limit() {
        let v = Some(Some("a".repeat(11)));
        assert!(validate_optional_nullable_string_length("f", &v, 10).is_err());
    }

    #[test]
    fn vec_count_over_limit() {
        let items: Vec<String> = (0..MAX_TAGS_COUNT + 1).map(|i| format!("t{i}")).collect();
        let err =
            validate_string_vec_items("tags", &items, MAX_TAG_LEN, MAX_TAGS_COUNT).unwrap_err();
        assert!(err.to_string().contains("21"));
    }

    #[test]
    fn vec_item_too_long() {
        let items = vec!["a".repeat(MAX_TAG_LEN + 1)];
        let err =
            validate_string_vec_items("tags", &items, MAX_TAG_LEN, MAX_TAGS_COUNT).unwrap_err();
        assert!(err.to_string().contains("tags[0]"));
    }

    #[test]
    fn vec_empty_ok() {
        assert!(validate_string_vec_items("tags", &[], MAX_TAG_LEN, MAX_TAGS_COUNT).is_ok());
    }

    use crate::domain::task::TaskId;

    fn make_graph(edges: &[(i64, Vec<i64>)]) -> impl Fn(TaskId) -> Vec<TaskId> + '_ {
        let map: HashMap<i64, &Vec<i64>> = edges.iter().map(|(k, v)| (*k, v)).collect();
        move |id: TaskId| {
            map.get(&id.into())
                .map(|v| v.iter().copied().map(TaskId).collect())
                .unwrap_or_default()
        }
    }

    #[test]
    fn no_cycle_linear_chain() {
        // 1 -> 2 -> 3, adding 4 depends on 3
        let edges = [(3, vec![2]), (2, vec![1]), (1, vec![])];
        assert!(!has_cycle(TaskId(4), TaskId(3), make_graph(&edges)));
    }

    #[test]
    fn direct_cycle() {
        // 1 -> 2, adding 2 depends on 1 would create: 2 -> 1 -> 2
        let edges = [(1, vec![2]), (2, vec![])];
        assert!(has_cycle(TaskId(2), TaskId(1), make_graph(&edges)));
    }

    #[test]
    fn indirect_cycle() {
        // 1 -> 2 -> 3, adding 3 depends on 1 would create: 3 -> 1 -> 2 -> 3
        let edges = [(1, vec![2]), (2, vec![3]), (3, vec![])];
        assert!(has_cycle(TaskId(3), TaskId(1), make_graph(&edges)));
    }

    #[test]
    fn diamond_no_cycle() {
        // 1 -> 2, 1 -> 3, 2 -> 4, 3 -> 4, adding 5 depends on 4
        let edges = [(4, vec![2, 3]), (2, vec![1]), (3, vec![1]), (1, vec![])];
        assert!(!has_cycle(TaskId(5), TaskId(4), make_graph(&edges)));
    }

    #[test]
    fn self_dependency_not_detected() {
        // has_cycle doesn't check self-dependency (task_id == dep_id);
        // that's validated separately at the call site
        let edges = [(1, vec![])];
        assert!(!has_cycle(TaskId(1), TaskId(1), make_graph(&edges)));
    }

    #[test]
    fn no_dependencies_no_cycle() {
        let edges: [(i64, Vec<i64>); 0] = [];
        assert!(!has_cycle(TaskId(2), TaskId(1), make_graph(&edges)));
    }

    // --- metadata validation tests ---

    #[test]
    fn metadata_within_limits() {
        let val = serde_json::json!({"key": "value", "num": 42});
        assert!(validate_metadata(&val).is_ok());
    }

    #[test]
    fn metadata_too_large() {
        let big = "x".repeat(70_000);
        let val = serde_json::json!({"big": big});
        let err = validate_metadata(&val).unwrap_err();
        assert!(matches!(err, DomainError::MetadataTooLarge { .. }));
    }

    #[test]
    fn metadata_depth_at_limit() {
        // 10 levels deep: {a:{a:{a:{a:{a:{a:{a:{a:{a:{a:1}}}}}}}}}}
        let mut val = serde_json::json!(1);
        for _ in 0..METADATA_MAX_DEPTH {
            val = serde_json::json!({"a": val});
        }
        assert!(validate_metadata(&val).is_ok());
    }

    #[test]
    fn metadata_too_deep() {
        // 11 levels deep
        let mut val = serde_json::json!(1);
        for _ in 0..=METADATA_MAX_DEPTH {
            val = serde_json::json!({"a": val});
        }
        let err = validate_metadata(&val).unwrap_err();
        assert!(matches!(err, DomainError::MetadataTooDeep { .. }));
    }

    #[test]
    fn metadata_empty_object() {
        let val = serde_json::json!({});
        assert!(validate_metadata(&val).is_ok());
    }

    #[test]
    fn metadata_flat_array() {
        let val = serde_json::json!([1, 2, 3]);
        assert!(validate_metadata(&val).is_ok());
    }

    #[test]
    fn json_depth_nested_arrays() {
        let val = serde_json::json!([[[1]]]);
        assert_eq!(json_depth(&val), 3);
    }

    #[test]
    fn json_depth_mixed() {
        let val = serde_json::json!({"a": [{"b": 1}]});
        // object(1) -> array(2) -> object(3) -> leaf = depth 3
        assert_eq!(json_depth(&val), 3);
    }

    #[test]
    fn json_depth_scalar() {
        assert_eq!(json_depth(&serde_json::json!(42)), 0);
        assert_eq!(json_depth(&serde_json::json!("hello")), 0);
        assert_eq!(json_depth(&serde_json::Value::Null), 0);
    }

    // --- validate_metadata_on_complete tests ---

    fn make_field(name: &str, ft: MetadataFieldType, required: bool) -> MetadataField {
        MetadataField::new(
            1,
            ProjectId(1),
            name.to_string(),
            ft,
            required,
            None,
            "2026-01-01T00:00:00Z".to_string(),
        )
    }

    #[test]
    fn complete_no_required_fields() {
        assert!(validate_metadata_on_complete(None, &[], TaskId(1)).is_ok());
    }

    #[test]
    fn complete_no_required_fields_with_optional() {
        let fields = vec![make_field("notes", MetadataFieldType::String, false)];
        assert!(validate_metadata_on_complete(None, &fields, TaskId(1)).is_ok());
    }

    #[test]
    fn complete_required_fields_present_correct_types() {
        let fields = vec![
            make_field("sprint", MetadataFieldType::String, true),
            make_field("points", MetadataFieldType::Number, true),
            make_field("reviewed", MetadataFieldType::Boolean, true),
        ];
        let meta = serde_json::json!({"sprint": "v1", "points": 5, "reviewed": true});
        assert!(validate_metadata_on_complete(Some(&meta), &fields, TaskId(1)).is_ok());
    }

    #[test]
    fn complete_required_field_missing_no_metadata() {
        let fields = vec![make_field("sprint", MetadataFieldType::String, true)];
        let err = validate_metadata_on_complete(None, &fields, TaskId(1)).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("sprint"),
            "error should mention field name: {msg}"
        );
    }

    #[test]
    fn complete_required_field_missing_empty_metadata() {
        let fields = vec![make_field("sprint", MetadataFieldType::String, true)];
        let meta = serde_json::json!({});
        let err = validate_metadata_on_complete(Some(&meta), &fields, TaskId(1)).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("sprint"),
            "error should mention field name: {msg}"
        );
    }

    #[test]
    fn complete_multiple_required_fields_missing() {
        let fields = vec![
            make_field("sprint", MetadataFieldType::String, true),
            make_field("points", MetadataFieldType::Number, true),
        ];
        let err = validate_metadata_on_complete(None, &fields, TaskId(1)).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("sprint"), "should mention sprint: {msg}");
        assert!(msg.contains("points"), "should mention points: {msg}");
    }

    #[test]
    fn complete_type_mismatch_string_got_number() {
        let fields = vec![make_field("sprint", MetadataFieldType::String, true)];
        let meta = serde_json::json!({"sprint": 42});
        let err = validate_metadata_on_complete(Some(&meta), &fields, TaskId(1)).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("sprint"), "should mention field name: {msg}");
        assert!(
            msg.contains("string"),
            "should mention expected type: {msg}"
        );
    }

    #[test]
    fn complete_type_mismatch_number_got_string() {
        let fields = vec![make_field("points", MetadataFieldType::Number, true)];
        let meta = serde_json::json!({"points": "five"});
        let err = validate_metadata_on_complete(Some(&meta), &fields, TaskId(1)).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("points"), "should mention field name: {msg}");
        assert!(
            msg.contains("number"),
            "should mention expected type: {msg}"
        );
    }

    #[test]
    fn complete_type_mismatch_boolean_got_string() {
        let fields = vec![make_field("done", MetadataFieldType::Boolean, true)];
        let meta = serde_json::json!({"done": "yes"});
        let err = validate_metadata_on_complete(Some(&meta), &fields, TaskId(1)).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("done"), "should mention field name: {msg}");
        assert!(
            msg.contains("boolean"),
            "should mention expected type: {msg}"
        );
    }

    #[test]
    fn complete_non_required_field_absent_ok() {
        let fields = vec![
            make_field("sprint", MetadataFieldType::String, true),
            make_field("notes", MetadataFieldType::String, false),
        ];
        let meta = serde_json::json!({"sprint": "v1"});
        assert!(validate_metadata_on_complete(Some(&meta), &fields, TaskId(1)).is_ok());
    }

    #[test]
    fn complete_null_value_treated_as_missing() {
        let fields = vec![make_field("sprint", MetadataFieldType::String, true)];
        let meta = serde_json::json!({"sprint": null});
        let err = validate_metadata_on_complete(Some(&meta), &fields, TaskId(1)).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("sprint"),
            "null value should be treated as missing: {msg}"
        );
    }

    #[test]
    fn complete_mixed_missing_and_type_error() {
        let fields = vec![
            make_field("sprint", MetadataFieldType::String, true),
            make_field("points", MetadataFieldType::Number, true),
        ];
        let meta = serde_json::json!({"points": "not a number"});
        let err = validate_metadata_on_complete(Some(&meta), &fields, TaskId(1)).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("sprint"),
            "should mention missing sprint: {msg}"
        );
        assert!(
            msg.contains("points"),
            "should mention type error for points: {msg}"
        );
    }
}
