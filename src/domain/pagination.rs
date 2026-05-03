use crate::domain::error::DomainError;

/// A page of items with an optional cursor to fetch the next page.
///
/// `next_cursor == None` signals the end of the collection.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ListPage<T> {
    pub items: Vec<T>,
    pub next_cursor: Option<String>,
}

impl<T> Default for ListPage<T> {
    fn default() -> Self {
        Self {
            items: Vec::new(),
            next_cursor: None,
        }
    }
}

/// Given items fetched with peek-ahead (`LIMIT limit + 1`), split them into a
/// [`ListPage`] by trimming the extra row and turning the last retained item
/// into a cursor via `to_cursor`.
///
/// Callers are responsible for doing the `LIMIT limit + 1` fetch. If `limit`
/// is `None`, every item is returned and `next_cursor` is `None`.
pub fn build_page<T>(
    mut items: Vec<T>,
    limit: Option<u32>,
    to_cursor: impl FnOnce(&T) -> String,
) -> ListPage<T> {
    if let Some(l) = limit
        && items.len() as u32 > l
    {
        items.truncate(l as usize);
        let next_cursor = items.last().map(to_cursor);
        return ListPage { items, next_cursor };
    }
    ListPage {
        items,
        next_cursor: None,
    }
}

/// Decoded cursor payload.
///
/// Wire format: base64 URL-safe (no padding) of one of these JSON shapes:
/// - `{"id": <i64>}` — id-only cursor (paired with `order_by=id`).
///   Backwards compatible with cursors emitted before composite cursors existed.
/// - `{"k":"updated_at","v":"<ISO 8601>","id":<i64>}` — for `order_by=updated_at`.
/// - `{"k":"priority","v":<i32>,"id":<i64>}` — for `order_by=priority`.
///
/// `serde(untagged)` lets the deserializer try the tagged form first and fall
/// back to the bare-id form. Old clients that round-trip an `{"id": 42}` cursor
/// continue to work without any change on their side.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum CursorPayload {
    Tagged(TaggedCursor),
    Id(IdCursor),
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "k", rename_all = "snake_case")]
pub enum TaggedCursor {
    UpdatedAt { v: String, id: i64 },
    Priority { v: i32, id: i64 },
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IdCursor {
    pub id: i64,
}

impl CursorPayload {
    /// Short label used in `CursorMismatch` errors.
    pub fn kind(&self) -> &'static str {
        match self {
            CursorPayload::Id(_) => "id",
            CursorPayload::Tagged(TaggedCursor::UpdatedAt { .. }) => "updated_at",
            CursorPayload::Tagged(TaggedCursor::Priority { .. }) => "priority",
        }
    }
}

/// Opaque cursor for list pagination.
pub struct Cursor;

impl Cursor {
    /// Encode any id that can be converted into `i64` as an opaque id-only
    /// cursor. Wire shape: `{"id": <i64>}`.
    pub fn encode<T: Into<i64>>(id: T) -> String {
        Self::encode_id(id)
    }

    /// Encode an id-only cursor. Equivalent to [`Cursor::encode`].
    pub fn encode_id<T: Into<i64>>(id: T) -> String {
        encode_payload(&CursorPayload::Id(IdCursor { id: id.into() }))
    }

    /// Encode a composite cursor pinned to `(updated_at, id)`.
    pub fn encode_updated_at<T: Into<i64>>(v: &str, id: T) -> String {
        encode_payload(&CursorPayload::Tagged(TaggedCursor::UpdatedAt {
            v: v.to_string(),
            id: id.into(),
        }))
    }

    /// Encode a composite cursor pinned to `(priority, id)`.
    pub fn encode_priority<T: Into<i64>>(v: i32, id: T) -> String {
        encode_payload(&CursorPayload::Tagged(TaggedCursor::Priority {
            v,
            id: id.into(),
        }))
    }

    /// Re-encode a [`CursorPayload`] back into its opaque wire string. Useful
    /// for relays that decode + forward without inspecting the variant.
    pub fn encode_payload(payload: &CursorPayload) -> String {
        encode_payload(payload)
    }

    /// Decode an opaque cursor string into a [`CursorPayload`]. Accepts both
    /// the legacy id-only shape (`{"id": <i64>}`) and the new tagged shapes.
    pub fn decode_payload(raw: &str) -> Result<CursorPayload, DomainError> {
        use base64::Engine as _;
        let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(raw)
            .map_err(|_| DomainError::InvalidCursor)?;
        serde_json::from_slice::<CursorPayload>(&bytes).map_err(|_| DomainError::InvalidCursor)
    }

    /// Backwards-compat shim: decode an opaque cursor into any id newtype.
    /// Returns `InvalidCursor` if the cursor is composite (not id-only).
    pub fn decode<T: From<i64>>(raw: &str) -> Result<T, DomainError> {
        match Self::decode_payload(raw)? {
            CursorPayload::Id(IdCursor { id }) => Ok(T::from(id)),
            // Older callers that ask for a plain id but receive a composite
            // cursor are using the wrong API for the request shape; surface
            // it as a generic InvalidCursor rather than CursorMismatch (the
            // latter wants the requested order_by name we don't have here).
            CursorPayload::Tagged(_) => Err(DomainError::InvalidCursor),
        }
    }
}

fn encode_payload(payload: &CursorPayload) -> String {
    use base64::Engine as _;
    let json = serde_json::to_vec(payload).expect("cursor payload is serializable");
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(json)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursor_roundtrip_i64() {
        let c = Cursor::encode(42i64);
        let back: i64 = Cursor::decode(&c).unwrap();
        assert_eq!(back, 42);
    }

    #[test]
    fn cursor_roundtrip_newtype() {
        #[derive(Debug, PartialEq, Eq, Clone, Copy)]
        struct FooId(i64);
        impl From<i64> for FooId {
            fn from(n: i64) -> Self {
                FooId(n)
            }
        }
        impl From<FooId> for i64 {
            fn from(f: FooId) -> i64 {
                f.0
            }
        }

        let c = Cursor::encode(FooId(7));
        let back: FooId = Cursor::decode(&c).unwrap();
        assert_eq!(back, FooId(7));
    }

    #[test]
    fn cursor_decode_rejects_garbage() {
        assert!(Cursor::decode::<i64>("not-base64!").is_err());
    }

    #[test]
    fn cursor_decode_rejects_bad_json() {
        use base64::Engine as _;
        let raw = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(b"not json");
        assert!(Cursor::decode::<i64>(&raw).is_err());
    }

    #[test]
    fn cursor_decode_rejects_missing_id() {
        use base64::Engine as _;
        let raw = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(b"{\"x\":1}");
        assert!(Cursor::decode::<i64>(&raw).is_err());
    }

    #[test]
    fn list_page_default_is_empty() {
        let p: ListPage<i64> = ListPage::default();
        assert!(p.items.is_empty());
        assert!(p.next_cursor.is_none());
    }

    #[test]
    fn build_page_no_limit_returns_all() {
        let p = build_page(vec![1i64, 2, 3], None, |n| Cursor::encode(*n));
        assert_eq!(p.items, vec![1, 2, 3]);
        assert!(p.next_cursor.is_none());
    }

    #[test]
    fn build_page_fewer_than_limit_has_no_cursor() {
        let p = build_page(vec![1i64, 2], Some(5), |n| Cursor::encode(*n));
        assert_eq!(p.items, vec![1, 2]);
        assert!(p.next_cursor.is_none());
    }

    #[test]
    fn build_page_exactly_limit_has_no_cursor() {
        // limit+1 peek-ahead returned exactly `limit`, so no more pages.
        let p = build_page(vec![1i64, 2], Some(2), |n| Cursor::encode(*n));
        assert_eq!(p.items, vec![1, 2]);
        assert!(p.next_cursor.is_none());
    }

    #[test]
    fn build_page_over_limit_trims_and_sets_cursor() {
        // peek-ahead returned limit+1; trim to limit and cursor is on last retained item.
        let p = build_page(vec![1i64, 2, 3], Some(2), |n| Cursor::encode(*n));
        assert_eq!(p.items, vec![1, 2]);
        let cursor = p.next_cursor.expect("next_cursor present");
        let back: i64 = Cursor::decode(&cursor).unwrap();
        assert_eq!(back, 2);
    }

    // --- Composite cursor tests ---

    #[test]
    fn composite_cursor_roundtrip_updated_at() {
        let c = Cursor::encode_updated_at("2026-05-03T11:39:48Z", 42i64);
        let payload = Cursor::decode_payload(&c).unwrap();
        assert_eq!(
            payload,
            CursorPayload::Tagged(TaggedCursor::UpdatedAt {
                v: "2026-05-03T11:39:48Z".to_string(),
                id: 42,
            })
        );
        assert_eq!(payload.kind(), "updated_at");
    }

    #[test]
    fn composite_cursor_roundtrip_priority() {
        let c = Cursor::encode_priority(0, 7i64);
        let payload = Cursor::decode_payload(&c).unwrap();
        assert_eq!(
            payload,
            CursorPayload::Tagged(TaggedCursor::Priority { v: 0, id: 7 })
        );
        assert_eq!(payload.kind(), "priority");
    }

    #[test]
    fn id_cursor_roundtrip_via_payload_api() {
        let c = Cursor::encode_id(123i64);
        let payload = Cursor::decode_payload(&c).unwrap();
        assert_eq!(payload, CursorPayload::Id(IdCursor { id: 123 }));
        assert_eq!(payload.kind(), "id");
    }

    #[test]
    fn legacy_id_only_cursor_decodes_as_id_payload() {
        // Bytes a pre-composite-cursor client would have stored:
        //   base64(URL_SAFE_NO_PAD)( {"id": 99} )
        use base64::Engine as _;
        let raw = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(b"{\"id\":99}");
        let payload = Cursor::decode_payload(&raw).unwrap();
        assert_eq!(payload, CursorPayload::Id(IdCursor { id: 99 }));
    }

    #[test]
    fn legacy_decode_t_rejects_composite_cursor() {
        // A composite cursor cannot be decoded back into a plain id newtype —
        // the call site must have asked the wrong way.
        let composite = Cursor::encode_updated_at("2026-01-01T00:00:00Z", 1i64);
        assert!(Cursor::decode::<i64>(&composite).is_err());
    }

    #[test]
    fn composite_cursor_decode_rejects_bad_kind() {
        // Tagged shape with an unknown `k` should fail.
        use base64::Engine as _;
        let raw = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(br#"{"k":"garbage","v":"x","id":1}"#);
        assert!(Cursor::decode_payload(&raw).is_err());
    }

    #[test]
    fn composite_cursor_decode_rejects_missing_value() {
        // Tagged shape without `v` should fail.
        use base64::Engine as _;
        let raw = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(br#"{"k":"updated_at","id":1}"#);
        assert!(Cursor::decode_payload(&raw).is_err());
    }
}
