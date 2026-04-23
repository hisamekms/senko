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

/// Opaque cursor for list pagination.
///
/// Wire format: base64 URL-safe (no padding) of JSON `{"id": <i64>}`. Callers
/// treat the string as opaque. The inner `i64` is the row id of the last
/// item returned in the previous page.
pub struct Cursor;

impl Cursor {
    /// Encode any id that can be converted into `i64` as an opaque cursor.
    pub fn encode<T: Into<i64>>(id: T) -> String {
        use base64::Engine as _;
        let payload = serde_json::json!({ "id": id.into() });
        let json = serde_json::to_vec(&payload).expect("cursor payload is serializable");
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(json)
    }

    /// Decode an opaque cursor into any id that can be built from `i64`.
    pub fn decode<T: From<i64>>(raw: &str) -> Result<T, DomainError> {
        use base64::Engine as _;
        let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(raw)
            .map_err(|_| DomainError::InvalidCursor)?;
        let v: serde_json::Value =
            serde_json::from_slice(&bytes).map_err(|_| DomainError::InvalidCursor)?;
        let id = v
            .get("id")
            .and_then(|x| x.as_i64())
            .ok_or(DomainError::InvalidCursor)?;
        Ok(T::from(id))
    }
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
}
