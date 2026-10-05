//! One schema, two backends, identical results (ADR 0006 §8; JSON:API wire
//! contract §5.4 and §8.2).
//!
//! Every scenario is written once, against [`Backend`], and run on SQLite
//! in memory and on a temp vault. Each run records what the store returned
//! in a transcript; the two transcripts must be equal, and each scenario
//! also asserts the expected values so that both backends agreeing on a
//! wrong answer still fails.
//!
//! Records cross the boundary as JSON: the two generated crates have
//! distinct `Item` types, but serialize the same.

use serde_json::{Value, json};

type R<T> = Result<T, StoreError>;

/// A store error with the backend's own type erased. The typed variants and
/// their payloads are part of the parity contract; the catch-all's text
/// (`DbError` on SeaORM, `Md` on markdown) is not, so two `Backend` errors
/// compare equal whatever they say.
#[derive(Debug, Clone)]
enum StoreError {
    NotFound(&'static str, String),
    IdRequired(&'static str, String),
    AlreadyExists(&'static str, String),
    ParentRequired(&'static str, String),
    ParentCycle(&'static str, String),
    Backend(String),
}

impl PartialEq for StoreError {
    fn eq(&self, other: &Self) -> bool {
        use StoreError::*;
        match (self, other) {
            (NotFound(a, x), NotFound(b, y))
            | (IdRequired(a, x), IdRequired(b, y))
            | (AlreadyExists(a, x), AlreadyExists(b, y))
            | (ParentRequired(a, x), ParentRequired(b, y))
            | (ParentCycle(a, x), ParentCycle(b, y)) => a == b && x == y,
            (Backend(_), Backend(_)) => true,
            _ => false,
        }
    }
}

/// The store surface the scenarios drive. Writes take and reads return
/// JSON; an update takes the `Update{Entity}Input` DTO's JSON form.
#[allow(async_fn_in_trait)]
trait Backend: Sized {
    const NAME: &'static str;
    async fn open() -> Self;

    async fn create_item(&self, item: Value) -> R<Value>;
    async fn get_item(&self, id: &str) -> R<Value>;
    async fn update_item(&self, id: &str, patch: Value) -> R<Value>;
    async fn delete_item(&self, id: &str) -> R<()>;
    /// `sort` is transport sort keys (`"title"`, `"-int32"`), parsed as
    /// every transport parses them.
    async fn list_items(&self, sort: &[&str], limit: Option<u64>, offset: Option<u64>) -> R<Vec<Value>>;
    async fn count_items(&self) -> R<u64>;
    /// Create `item` with the float `field` set to NaN, which JSON cannot
    /// carry: only server-side Rust can write one.
    async fn create_item_nan(&self, item: Value, field: &str) -> R<Value>;
    /// Update `id`, setting only the float `field`, to NaN.
    async fn update_item_nan(&self, id: &str, field: &str) -> R<Value>;

    async fn create_tag(&self, tag: Value) -> R<Value>;

    async fn create_section(&self, section: Value) -> R<Value>;
    async fn update_section(&self, id: &str, patch: Value) -> R<Value>;
    async fn list_sections(&self) -> R<Vec<Value>>;

    async fn create_fixed(&self, fixed: Value) -> R<Value>;
    async fn get_fixed(&self, id: &str) -> R<Value>;
    async fn count_fixeds(&self) -> R<u64>;

    async fn create_stamped(&self, stamped: Value) -> R<Value>;
    async fn get_stamped(&self, id: &str) -> R<Value>;
    async fn count_stampeds(&self) -> R<u64>;

    async fn create_doc(&self, doc: Value) -> R<Value>;
    async fn get_doc(&self, id: &str) -> R<Value>;
    async fn update_doc(&self, id: &str, patch: Value) -> R<Value>;

    async fn create_match(&self, r#match: Value) -> R<Value>;
    async fn create_order(&self, order: Value) -> R<Value>;
    async fn get_order(&self, id: &str) -> R<Value>;
    async fn update_order(&self, id: &str, patch: Value) -> R<Value>;

    /// Rename a stored item the way an edit made outside the store would
    /// (an SQL `UPDATE`, a file rename), to reach ids no create can make.
    async fn rename_item(&self, from: &str, to: &str);
}

/// The generated method calls are spelled identically in both crates, so
/// one body serves both; only the crates and the catch-all variant differ.
macro_rules! store_methods {
    ($krate:ident, $other:ident) => {
        async fn create_item(&self, item: Value) -> R<Value> {
            let item = serde_json::from_value(item).expect("an Item");
            self.store.create_item(item).await.map(to_json).map_err(err)
        }
        async fn get_item(&self, id: &str) -> R<Value> {
            self.store.get_item(id).await.map(to_json).map_err(err)
        }
        async fn update_item(&self, id: &str, patch: Value) -> R<Value> {
            let input: $krate::schema::UpdateItemInput = serde_json::from_value(patch).expect("an UpdateItemInput");
            self.store.update_item(id, input.into()).await.map(to_json).map_err(err)
        }
        async fn delete_item(&self, id: &str) -> R<()> {
            self.store.delete_item(id).await.map_err(err)
        }
        async fn list_items(&self, sort: &[&str], limit: Option<u64>, offset: Option<u64>) -> R<Vec<Value>> {
            let order = ontogen_core::order::parse_sort::<$krate::store::item::ItemSortField>(sort).expect("sort keys");
            self.store
                .list_items(&order, limit, offset)
                .await
                .map(|v| v.into_iter().map(to_json).collect())
                .map_err(err)
        }
        async fn count_items(&self) -> R<u64> {
            self.store.count_items().await.map_err(err)
        }
        async fn create_item_nan(&self, item: Value, field: &str) -> R<Value> {
            let mut item: $krate::schema::Item = serde_json::from_value(item).expect("an Item");
            match field {
                "float32" => item.float32 = f32::NAN,
                "float64" => item.float64 = f64::NAN,
                "maybe_float32" => item.maybe_float32 = Some(f32::NAN),
                "maybe_float64" => item.maybe_float64 = Some(f64::NAN),
                other => panic!("Item has no float field {other}"),
            }
            self.store.create_item(item).await.map(to_json).map_err(err)
        }
        async fn update_item_nan(&self, id: &str, field: &str) -> R<Value> {
            let mut updates = $krate::store::item::ItemUpdate::default();
            match field {
                "float32" => updates.float32 = Some(f32::NAN),
                "float64" => updates.float64 = Some(f64::NAN),
                "maybe_float32" => updates.maybe_float32 = Some(Some(f32::NAN)),
                "maybe_float64" => updates.maybe_float64 = Some(Some(f64::NAN)),
                other => panic!("Item has no float field {other}"),
            }
            self.store.update_item(id, updates).await.map(to_json).map_err(err)
        }
        async fn create_tag(&self, tag: Value) -> R<Value> {
            let tag = serde_json::from_value(tag).expect("a Tag");
            self.store.create_tag(tag).await.map(to_json).map_err(err)
        }
        async fn create_section(&self, section: Value) -> R<Value> {
            let section = serde_json::from_value(section).expect("a Section");
            self.store.create_section(section).await.map(to_json).map_err(err)
        }
        async fn update_section(&self, id: &str, patch: Value) -> R<Value> {
            let input: $krate::schema::UpdateSectionInput =
                serde_json::from_value(patch).expect("an UpdateSectionInput");
            self.store.update_section(id, input.into()).await.map(to_json).map_err(err)
        }
        async fn list_sections(&self) -> R<Vec<Value>> {
            self.store.list_sections(&[], None, None).await.map(|v| v.into_iter().map(to_json).collect()).map_err(err)
        }
        async fn create_fixed(&self, fixed: Value) -> R<Value> {
            let fixed = serde_json::from_value(fixed).expect("a Fixed");
            self.store.create_fixed(fixed).await.map(to_json).map_err(err)
        }
        async fn get_fixed(&self, id: &str) -> R<Value> {
            self.store.get_fixed(id).await.map(to_json).map_err(err)
        }
        async fn count_fixeds(&self) -> R<u64> {
            self.store.count_fixeds().await.map_err(err)
        }
        async fn create_stamped(&self, stamped: Value) -> R<Value> {
            let stamped = serde_json::from_value(stamped).expect("a Stamped");
            self.store.create_stamped(stamped).await.map(to_json).map_err(err)
        }
        async fn get_stamped(&self, id: &str) -> R<Value> {
            self.store.get_stamped(id).await.map(to_json).map_err(err)
        }
        async fn count_stampeds(&self) -> R<u64> {
            self.store.count_stampeds().await.map_err(err)
        }
        async fn create_doc(&self, doc: Value) -> R<Value> {
            let doc = serde_json::from_value(doc).expect("a Doc");
            self.store.create_doc(doc).await.map(to_json).map_err(err)
        }
        async fn get_doc(&self, id: &str) -> R<Value> {
            self.store.get_doc(id).await.map(to_json).map_err(err)
        }
        async fn update_doc(&self, id: &str, patch: Value) -> R<Value> {
            let input: $krate::schema::UpdateDocInput = serde_json::from_value(patch).expect("an UpdateDocInput");
            self.store.update_doc(id, input.into()).await.map(to_json).map_err(err)
        }
        async fn create_match(&self, r#match: Value) -> R<Value> {
            let r#match = serde_json::from_value(r#match).expect("a Match");
            self.store.create_match(r#match).await.map(to_json).map_err(err)
        }
        async fn create_order(&self, order: Value) -> R<Value> {
            let order = serde_json::from_value(order).expect("an Order");
            self.store.create_order(order).await.map(to_json).map_err(err)
        }
        async fn get_order(&self, id: &str) -> R<Value> {
            self.store.get_order(id).await.map(to_json).map_err(err)
        }
        async fn update_order(&self, id: &str, patch: Value) -> R<Value> {
            let input: $krate::schema::UpdateOrderInput = serde_json::from_value(patch).expect("an UpdateOrderInput");
            self.store.update_order(id, input.into()).await.map(to_json).map_err(err)
        }
    };
}

macro_rules! error_mappers {
    ($krate:ident, $other:ident) => {
        fn err(e: $krate::schema::AppError) -> StoreError {
            use $krate::schema::AppError as E;
            match e {
                E::DocNotFound(id) => StoreError::NotFound("Doc", id),
                E::DocIdRequired(r) => StoreError::IdRequired("Doc", r),
                E::DocAlreadyExists(id) => StoreError::AlreadyExists("Doc", id),
                E::DocParentCycle(id) => StoreError::ParentCycle("Doc", id),
                E::MatchNotFound(id) => StoreError::NotFound("Match", id),
                E::MatchIdRequired(r) => StoreError::IdRequired("Match", r),
                E::MatchAlreadyExists(id) => StoreError::AlreadyExists("Match", id),
                E::OrderNotFound(id) => StoreError::NotFound("Order", id),
                E::OrderIdRequired(r) => StoreError::IdRequired("Order", r),
                E::OrderAlreadyExists(id) => StoreError::AlreadyExists("Order", id),
                E::SeaOrmNotFound(id) => StoreError::NotFound("SeaOrm", id),
                E::SeaOrmIdRequired(r) => StoreError::IdRequired("SeaOrm", r),
                E::SeaOrmAlreadyExists(id) => StoreError::AlreadyExists("SeaOrm", id),
                E::SeaQueryNotFound(id) => StoreError::NotFound("SeaQuery", id),
                E::SeaQueryIdRequired(r) => StoreError::IdRequired("SeaQuery", r),
                E::SeaQueryAlreadyExists(id) => StoreError::AlreadyExists("SeaQuery", id),
                E::FixedNotFound(id) => StoreError::NotFound("Fixed", id),
                E::FixedIdRequired(r) => StoreError::IdRequired("Fixed", r),
                E::FixedAlreadyExists(id) => StoreError::AlreadyExists("Fixed", id),
                E::ItemNotFound(id) => StoreError::NotFound("Item", id),
                E::ItemIdRequired(r) => StoreError::IdRequired("Item", r),
                E::ItemAlreadyExists(id) => StoreError::AlreadyExists("Item", id),
                E::ItemParentCycle(id) => StoreError::ParentCycle("Item", id),
                E::SectionNotFound(id) => StoreError::NotFound("Section", id),
                E::SectionIdRequired(r) => StoreError::IdRequired("Section", r),
                E::SectionAlreadyExists(id) => StoreError::AlreadyExists("Section", id),
                E::SectionParentRequired(id) => StoreError::ParentRequired("Section", id),
                E::SectionParentCycle(id) => StoreError::ParentCycle("Section", id),
                E::TagNotFound(id) => StoreError::NotFound("Tag", id),
                E::TagIdRequired(r) => StoreError::IdRequired("Tag", r),
                E::TagAlreadyExists(id) => StoreError::AlreadyExists("Tag", id),
                E::StampedNotFound(id) => StoreError::NotFound("Stamped", id),
                E::StampedIdRequired(r) => StoreError::IdRequired("Stamped", r),
                E::StampedAlreadyExists(id) => StoreError::AlreadyExists("Stamped", id),
                E::$other(msg) => StoreError::Backend(msg),
            }
        }
    };
}

fn to_json<T: serde::Serialize>(value: T) -> Value {
    serde_json::to_value(value).expect("serializable")
}

mod sqlite {
    use super::*;

    pub struct Sqlite {
        pub store: parity_seaorm::Store,
    }

    impl Backend for Sqlite {
        const NAME: &'static str = "seaorm";

        async fn open() -> Self {
            Self { store: parity_seaorm::Store::open_in_memory().await.expect("sqlite") }
        }

        store_methods!(parity_seaorm, DbError);

        async fn rename_item(&self, from: &str, to: &str) {
            use sea_orm::ConnectionTrait;
            let sql = format!("UPDATE items SET id = '{to}' WHERE id = '{from}'");
            let renamed = self.store.db().execute_unprepared(&sql).await.expect("rename");
            assert_eq!(renamed.rows_affected(), 1, "rename {from:?}");
        }
    }

    error_mappers!(parity_seaorm, DbError);
}

mod vault {
    use super::*;

    pub struct Vault {
        _dir: tempfile::TempDir,
        pub store: parity_markdown::Store,
    }

    impl Backend for Vault {
        const NAME: &'static str = "markdown";

        async fn open() -> Self {
            let dir = tempfile::tempdir().expect("tempdir");
            let store =
                parity_markdown::Store::new(parity_markdown::persistence::markdown::generated::open_vault(dir.path()));
            Self { _dir: dir, store }
        }

        store_methods!(parity_markdown, Md);

        async fn rename_item(&self, from: &str, to: &str) {
            let dir = self.store.vault().root().join("items");
            std::fs::rename(dir.join(format!("{from}.md")), dir.join(format!("{to}.md"))).expect("rename");
        }
    }

    error_mappers!(parity_markdown, Md);
}

use sqlite::Sqlite;
use vault::Vault;

// ─── Harness ────────────────────────────────────────────────────────────────

/// What one backend returned, step by step, for comparison with the other.
struct Transcript {
    backend: &'static str,
    steps: Vec<(String, Value)>,
}

impl Transcript {
    fn new<B: Backend>() -> Self {
        Self { backend: B::NAME, steps: Vec::new() }
    }

    /// Record a result under `label` and hand it back.
    fn record<T: serde::Serialize + Clone>(&mut self, label: impl Into<String>, result: &R<T>) -> R<T> {
        let value = match result {
            Ok(v) => json!({ "ok": to_json(v.clone()) }),
            Err(StoreError::Backend(_)) => json!({ "err": "Backend" }),
            Err(e) => json!({ "err": format!("{e:?}") }),
        };
        self.steps.push((label.into(), value));
        result.clone()
    }

    /// Assert `result` equals `expected` on this backend, and record it.
    fn expect<T: serde::Serialize + Clone + PartialEq + std::fmt::Debug>(
        &mut self,
        label: &str,
        result: R<T>,
        expected: R<T>,
    ) {
        assert_eq!(result, expected, "[{}] {label}", self.backend);
        self.record(label, &result).ok();
    }
}

/// Run `scenario` on both backends and assert the transcripts match step
/// for step.
macro_rules! parity {
    ($scenario:ident) => {{
        let sqlite = $scenario(&Sqlite::open().await, Transcript::new::<Sqlite>()).await;
        let vault = $scenario(&Vault::open().await, Transcript::new::<Vault>()).await;
        assert_eq!(sqlite.steps.len(), vault.steps.len(), "the scenario took different paths");
        for ((label, s), (_, v)) in sqlite.steps.iter().zip(&vault.steps) {
            assert_eq!(s, v, "{label}: seaorm (left) and markdown (right) disagree");
        }
    }};
}

fn ids(records: &[Value]) -> Vec<String> {
    records.iter().map(|r| r["id"].as_str().expect("an id").to_string()).collect()
}

fn listed_ids(result: R<Vec<Value>>) -> R<Vec<String>> {
    result.map(|records| ids(&records))
}

/// An item with every field set to a plain value; `overrides` replaces
/// any of them.
fn item(id: &str, overrides: Value) -> Value {
    let mut item = json!({
        "id": id,
        "title": format!("Item {id}"),
        "int32": 0, "int64": 0, "float32": 0.0, "float64": 0.0, "flag": false, "kind": "alpha",
        "maybe_text": null, "maybe_int32": null, "maybe_int64": null, "maybe_float32": null,
        "maybe_float64": null, "maybe_flag": null, "maybe_kind": null,
        "n_u8": 0, "n_u16": 0, "n_u32": 0, "n_u64": 0, "n_usize": 0, "n_u128": 0,
        "n_i8": 0, "n_i16": 0, "n_isize": 0, "n_i128": 0, "maybe_u32": null, "maybe_u64": null,
        "parent_id": null, "children": [], "tags": [], "body": "",
    });
    for (k, v) in overrides.as_object().expect("an object") {
        assert!(item.get(k).is_some(), "Item has no field {k}");
        item[k] = v.clone();
    }
    item
}

fn section(id: &str, parent: &str) -> Value {
    json!({ "id": id, "title": format!("Section {id}"), "parent_id": parent, "children": [] })
}

fn section_with(id: &str, parent: &str, children: &[&str]) -> Value {
    json!({ "id": id, "title": format!("Section {id}"), "parent_id": parent, "children": children })
}

fn ok_ids(list: &[&str]) -> R<Vec<String>> {
    Ok(list.iter().map(|s| s.to_string()).collect())
}

// ─── Default order and pagination ───────────────────────────────────────────

/// Ids chosen so byte order differs from locale (punctuation-blind) and
/// length order, and so a prefix's separator decides: `-` < `.` < digits <
/// `_` < lowercase < `~`.
const ORDER_IDS: &[&str] = &["z", "a~b", "a", "_x", "a-2", "a.b", "a2", "a_b", "~", "alpha", "-z", "z.z", "10", "9"];

async fn default_order_and_pages<B: Backend>(b: &B, mut t: Transcript) -> Transcript {
    for id in ORDER_IDS {
        t.record(format!("create {id}"), &b.create_item(item(id, json!({}))).await.map(|v| v["id"].clone()))
            .expect("create");
    }
    let mut expected: Vec<&str> = ORDER_IDS.to_vec();
    expected.sort_unstable(); // `str: Ord` is byte order
    assert_eq!(expected, ["-z", "10", "9", "_x", "a", "a-2", "a.b", "a2", "a_b", "alpha", "a~b", "z", "z.z", "~"]);

    t.expect("list", listed_ids(b.list_items(&[], None, None).await), ok_ids(&expected));
    t.expect("count", b.count_items().await, Ok(expected.len() as u64));

    let n = expected.len() as u64;
    for limit in [0, 1, 2, 3, 5, n - 1, n, n + 5] {
        let mut offset = 0;
        loop {
            let start = (offset as usize).min(expected.len());
            let end = (start + limit as usize).min(expected.len());
            t.expect(
                &format!("page limit={limit} offset={offset}"),
                listed_ids(b.list_items(&[], Some(limit), Some(offset)).await),
                ok_ids(&expected[start..end]),
            );
            if offset > n || limit == 0 {
                break;
            }
            offset += limit;
        }
    }
    t.expect("offset past the end", listed_ids(b.list_items(&[], None, Some(n + 1)).await), ok_ids(&[]));
    t.expect(
        "offset only",
        listed_ids(b.list_items(&[], None, Some(n - 2)).await),
        ok_ids(&expected[expected.len() - 2..]),
    );
    t
}

#[tokio::test]
async fn lists_are_in_id_byte_order_on_every_page() {
    parity!(default_order_and_pages);
}

/// A limit or offset past what the engine binds (i64) behaves as on
/// markdown: an offset past the end is an empty page, a limit is every row.
async fn oversized_pages<B: Backend>(b: &B, mut t: Transcript) -> Transcript {
    for id in ["a", "b", "c"] {
        t.record(format!("create {id}"), &b.create_item(item(id, json!({}))).await).unwrap();
    }
    let huge = i64::MAX as u64 + 1;
    let cases: [(Option<u64>, Option<u64>, &[&str]); 8] = [
        (Some(u64::MAX), None, &["a", "b", "c"]),
        (None, Some(u64::MAX), &[]),
        (Some(u64::MAX), Some(u64::MAX), &[]),
        (Some(u64::MAX), Some(1), &["b", "c"]),
        (Some(1), Some(u64::MAX), &[]),
        (Some(huge), Some(0), &["a", "b", "c"]),
        (None, Some(huge), &[]),
        (Some(i64::MAX as u64), Some(i64::MAX as u64), &[]),
    ];
    for (limit, offset, expected) in cases {
        t.expect(
            &format!("limit={limit:?} offset={offset:?}"),
            listed_ids(b.list_items(&[], limit, offset).await),
            ok_ids(expected),
        );
    }
    t
}

#[tokio::test]
async fn an_oversized_limit_or_offset_pages_like_any_other() {
    parity!(oversized_pages);
}

// ─── Field values ───────────────────────────────────────────────────────────

async fn field_values_round_trip<B: Backend>(b: &B, mut t: Transcript) -> Transcript {
    let wide = item(
        "wide",
        json!({
            "title": "Wide — ünïcode", "int32": i32::MIN, "int64": i64::MAX, "float32": 1.5, "float64": -2.25e10,
            "flag": true, "kind": "gamma",
            "maybe_text": "", "maybe_int32": i32::MAX, "maybe_int64": i64::MIN, "maybe_float32": -0.25,
            "maybe_float64": 1e-300, "maybe_flag": false, "maybe_kind": "beta",
            "n_u8": u8::MAX, "n_u16": u16::MAX, "n_u32": 3_000_000_000u32, "n_u64": i64::MAX, "n_usize": i64::MAX,
            "n_u128": i64::MAX, "n_i8": i8::MIN, "n_i16": i16::MIN, "n_isize": i64::MIN, "n_i128": i64::MIN,
            "maybe_u32": 3_000_000_000u32, "maybe_u64": i64::MAX, "body": "# Body\n\nwith *markdown*\n",
        }),
    );
    let max = item("max", json!({ "n_u32": u32::MAX, "maybe_u32": u32::MAX }));
    let none = item("none", json!({ "maybe_u32": null }));
    for record in [&wide, &max, &none] {
        let id = record["id"].as_str().unwrap();
        t.expect(&format!("create {id}"), b.create_item(record.clone()).await, Ok(record.clone()));
        t.expect(&format!("get {id}"), b.get_item(id).await, Ok(record.clone()));
    }
    t.expect("list", b.list_items(&[], None, None).await, Ok(vec![max.clone(), none.clone(), wide.clone()]));

    let mut updated = none.clone();
    updated["maybe_u32"] = json!(u32::MAX);
    updated["maybe_text"] = json!("set");
    t.expect(
        "update sets maybe_u32 to u32::MAX",
        b.update_item("none", json!({ "maybe_u32": u32::MAX, "maybe_text": "set" })).await,
        Ok(updated.clone()),
    );
    updated["maybe_u32"] = Value::Null;
    t.expect("update clears maybe_u32", b.update_item("none", json!({ "maybe_u32": null })).await, Ok(updated.clone()));
    t.expect("get after updates", b.get_item("none").await, Ok(updated));
    t
}

#[tokio::test]
async fn every_field_type_round_trips_exactly() {
    parity!(field_values_round_trip);
}

/// A value outside `i64` is refused on both backends with the catch-all,
/// before anything is written: SeaORM cannot store it (ADR 0006 §4), so the
/// markdown store refuses it too, although a vault could hold it.
async fn integers_outside_i64<B: Backend>(b: &B, mut t: Transcript) -> Transcript {
    let huge = i64::MAX as u64 + 1;
    let cases =
        [("n_u64", huge), ("n_u64", u64::MAX), ("maybe_u64", u64::MAX), ("n_usize", huge), ("n_u128", u64::MAX)];
    for (i, (field, value)) in cases.iter().enumerate() {
        let label = format!("create {field} = {value}");
        let result = b.create_item(item(&format!("big{i}"), json!({ *field: value }))).await;
        assert!(matches!(result, Err(StoreError::Backend(_))), "[{}] {label}: {result:?}", B::NAME);
        t.record(label, &result).unwrap_err();
    }
    t.expect("nothing was stored", b.count_items().await, Ok(0));

    let base = item("base", json!({ "maybe_u64": 7 }));
    t.expect("create base", b.create_item(base.clone()).await, Ok(base.clone()));
    for (field, value) in cases {
        let label = format!("update {field} = {value}");
        let result = b.update_item("base", json!({ field: value })).await;
        assert!(matches!(result, Err(StoreError::Backend(_))), "[{}] {label}: {result:?}", B::NAME);
        t.record(label, &result).unwrap_err();
    }
    t.expect("base is unchanged", b.get_item("base").await, Ok(base));

    // Refused before an id is derived, so a title with nothing to derive
    // from does not decide the error.
    let result = b.create_item(item("", json!({ "title": "!!!", "n_u64": u64::MAX }))).await;
    assert!(matches!(result, Err(StoreError::Backend(_))), "[{}] empty slug and u64::MAX: {result:?}", B::NAME);
    t.record("create with an empty slug and n_u64 = u64::MAX", &result).unwrap_err();
    t.expect("still only base", listed_ids(b.list_items(&[], None, None).await), ok_ids(&["base"]));
    t
}

#[tokio::test]
async fn an_integer_outside_i64_fails_the_write() {
    parity!(integers_outside_i64);
}

// ─── Linkage order ──────────────────────────────────────────────────────────

async fn linkage_order<B: Backend>(b: &B, mut t: Transcript) -> Transcript {
    for tag in ["t1", "t2", "t3"] {
        t.record(format!("create tag {tag}"), &b.create_tag(json!({ "id": tag, "title": tag })).await).unwrap();
    }

    // has_many: id ascending, whatever order the children were written in.
    t.record("create p", &b.create_item(item("p", json!({}))).await).unwrap();
    for child in ["c3", "c1", "c~", "c-2"] {
        t.record(format!("create {child}"), &b.create_item(item(child, json!({ "parent_id": "p" }))).await).unwrap();
    }
    let children = |v: R<Value>| v.map(|p| p["children"].clone());
    t.expect("children of p", children(b.get_item("p").await), Ok(json!(["c-2", "c1", "c3", "c~"])));

    // Children listed on create are pointed at the new record.
    t.expect(
        "create q listing two of p's children",
        children(b.create_item(item("q", json!({ "children": ["c~", "c1"] }))).await),
        Ok(json!(["c1", "c~"])),
    );
    t.expect("p keeps the rest", children(b.get_item("p").await), Ok(json!(["c-2", "c3"])));

    // A record that is its own parent is not its own child.
    t.expect(
        "create a self-parented record",
        children(b.create_item(item("self", json!({ "parent_id": "self" }))).await),
        Ok(json!([])),
    );
    t.record("create a child of self", &b.create_item(item("self-kid", json!({ "parent_id": "self" }))).await).unwrap();
    t.expect("self lists only its other child", children(b.get_item("self").await), Ok(json!(["self-kid"])));
    t.expect(
        "self's own parent is itself",
        b.get_item("self").await.map(|v| v["parent_id"].clone()),
        Ok(json!("self")),
    );

    // many_to_many: the order written, sorted or not, through updates.
    let tags = |v: R<Value>| v.map(|r| r["tags"].clone());
    t.expect(
        "create m with unsorted tags",
        tags(b.create_item(item("m", json!({ "tags": ["t3", "t1", "t2"] }))).await),
        Ok(json!(["t3", "t1", "t2"])),
    );
    t.expect("get m", tags(b.get_item("m").await), Ok(json!(["t3", "t1", "t2"])));
    t.expect(
        "reorder m's tags",
        tags(b.update_item("m", json!({ "tags": ["t2", "t3", "t1"] })).await),
        Ok(json!(["t2", "t3", "t1"])),
    );
    t.expect(
        "an update not touching tags keeps their order",
        tags(b.update_item("m", json!({ "title": "renamed" })).await),
        Ok(json!(["t2", "t3", "t1"])),
    );
    t.expect("drop a tag", tags(b.update_item("m", json!({ "tags": ["t1", "t2"] })).await), Ok(json!(["t1", "t2"])));
    let listed = b
        .list_items(&[], None, None)
        .await
        .map(|all| all.into_iter().map(|r| json!([r["id"], r["children"], r["tags"]])).collect::<Vec<_>>());
    t.record("every record's linkage, listed", &listed).unwrap();
    t
}

#[tokio::test]
async fn has_many_is_id_ascending_and_many_to_many_is_written_order() {
    parity!(linkage_order);
}

// ─── Requested order ────────────────────────────────────────────────────────

/// Every sort key of `Item`: its sortable fields (ADR 0006 §2), each
/// ascending and descending.
const SORTABLE: &[&str] = &[
    "id",
    "title",
    "int32",
    "int64",
    "float32",
    "float64",
    "flag",
    "kind",
    "maybe_text",
    "maybe_int32",
    "maybe_int64",
    "maybe_float32",
    "maybe_float64",
    "maybe_flag",
    "maybe_kind",
    "n_u8",
    "n_u16",
    "n_u32",
    "n_u64",
    "n_usize",
    "n_u128",
    "n_i8",
    "n_i16",
    "n_isize",
    "n_i128",
    "maybe_u32",
    "maybe_u64",
];

/// Records whose fields order differently from their ids and from each
/// other, with nulls, ties, case and non-ASCII strings, both zeros, and
/// enums whose stored strings sort apart from their declaration.
fn sort_fixture() -> Vec<Value> {
    vec![
        item(
            "e",
            json!({
                "title": "B", "int32": 7, "int64": -5, "float32": 0.0, "float64": -0.0, "flag": true,
                "kind": "gamma", "maybe_text": "B", "maybe_int32": -1, "maybe_float32": 2.5, "maybe_flag": true,
                "maybe_kind": "delta", "n_u8": 200, "n_u16": 9, "n_u32": 3_000_000_000u32, "n_u64": 10, "n_usize": 9,
                "n_u128": 1, "n_i8": -128, "n_i16": 5, "n_isize": -9, "n_i128": 9, "maybe_u32": 3_000_000_000u32,
                "maybe_u64": 9,
            }),
        ),
        item(
            "b",
            json!({
                "title": "a", "int32": -7, "int64": 5, "float32": -0.0, "float64": 0.0, "kind": "alpha",
                "maybe_text": "", "maybe_int64": 10, "maybe_float32": -0.0, "maybe_float64": 1.0, "n_u8": 9,
                "n_u16": 10, "n_u32": 9, "n_u64": 9, "n_usize": 10, "n_u128": 3_000_000_000u64, "n_i8": 9,
                "n_i16": -10, "n_isize": 10, "n_i128": -9, "maybe_u32": 10,
            }),
        ),
        item(
            "h",
            json!({
                "title": "é", "int32": 7, "float32": 1.5, "float64": -1e10, "flag": true, "kind": "beta",
                "maybe_int32": 9, "maybe_int64": -3, "maybe_float64": -0.0, "maybe_flag": false,
                "maybe_kind": "gamma", "n_u8": 10, "n_u32": 10, "n_u64": i64::MAX, "n_u128": 9, "n_i8": 10,
                "n_i16": 9, "n_isize": 9, "n_i128": 10, "maybe_u32": 9, "maybe_u64": 10,
            }),
        ),
        item(
            "a",
            json!({
                "title": "z", "float32": -1.25, "float64": 2.0, "kind": "delta", "maybe_text": "a",
                "maybe_int32": -1, "maybe_float32": 0.0, "maybe_float64": 0.0, "maybe_flag": true,
                "maybe_kind": "alpha",
            }),
        ),
        item("j", json!({ "title": "B", "maybe_text": "é", "maybe_kind": "beta", "maybe_u32": 3_000_000_000u32 })),
        item("c", json!({})),
        item("g", json!({ "title": "", "maybe_text": "z", "maybe_kind": "gamma", "maybe_flag": false })),
        item("d", json!({ "title": "a", "kind": "gamma", "flag": true, "maybe_u32": 0 })),
    ]
}

/// ADR 0006 §3 restated over the records' JSON, independently of either
/// store: nulls first, strings by bytes, numbers numerically with `-0.0`
/// equal to `0.0`, `false < true`, and enums by the string they are
/// stored as (the JSON value).
fn reference_cmp(a: &Value, b: &Value) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    match (a, b) {
        (Value::Null, Value::Null) => Ordering::Equal,
        (Value::Null, _) => Ordering::Less,
        (_, Value::Null) => Ordering::Greater,
        (Value::String(x), Value::String(y)) => x.as_bytes().cmp(y.as_bytes()),
        (Value::Bool(x), Value::Bool(y)) => x.cmp(y),
        (Value::Number(x), Value::Number(y)) => {
            let int = |n: &serde_json::Number| n.as_i64().map(i128::from).or(n.as_u64().map(i128::from));
            match (int(x), int(y)) {
                (Some(x), Some(y)) => x.cmp(&y),
                _ => {
                    let (x, y) = (x.as_f64().unwrap(), y.as_f64().unwrap());
                    if x == 0.0 && y == 0.0 { Ordering::Equal } else { x.total_cmp(&y) }
                }
            }
        }
        _ => panic!("incomparable {a} and {b}"),
    }
}

/// The ids of `records` in the order `sort` asks for, then by id.
fn reference_order(records: &[Value], sort: &[&str]) -> Vec<String> {
    let mut sorted = records.to_vec();
    sorted.sort_by(|a, b| {
        sort.iter()
            .chain(std::iter::once(&"id"))
            .map(|key| {
                let (field, desc) = key.strip_prefix('-').map_or((*key, false), |f| (f, true));
                let ord = reference_cmp(&a[field], &b[field]);
                if desc { ord.reverse() } else { ord }
            })
            .find(|o| o.is_ne())
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    ids(&sorted)
}

async fn load<B: Backend>(b: &B, t: &mut Transcript, records: &[Value]) {
    for record in records {
        let id = record["id"].as_str().unwrap();
        t.expect(&format!("create {id}"), b.create_item(record.clone()).await.map(|r| r["id"].clone()), Ok(json!(id)));
    }
}

async fn every_field_both_ways<B: Backend>(b: &B, mut t: Transcript) -> Transcript {
    let records = sort_fixture();
    load(b, &mut t, &records).await;
    t.expect(
        "default order",
        listed_ids(b.list_items(&[], None, None).await),
        ok_ids(&["a", "b", "c", "d", "e", "g", "h", "j"]),
    );
    for field in SORTABLE {
        for key in [field.to_string(), format!("-{field}")] {
            let expected = reference_order(&records, &[key.as_str()]);
            t.expect(&format!("sort {key}"), listed_ids(b.list_items(&[key.as_str()], None, None).await), Ok(expected));
        }
    }
    t
}

#[tokio::test]
async fn every_sortable_field_orders_alike_both_ways() {
    parity!(every_field_both_ways);
}

async fn multi_key_orders<B: Backend>(b: &B, mut t: Transcript) -> Transcript {
    let records = sort_fixture();
    load(b, &mut t, &records).await;
    let orders: &[&[&str]] = &[
        &["-flag", "title"],
        &["kind", "-maybe_text", "float64"],
        &["-maybe_kind", "int32", "-id"],
        &["maybe_flag", "-n_u32", "maybe_float32"],
        &["title", "-id"],
        &["-maybe_u32", "-title"],
    ];
    for sort in orders {
        let expected = reference_order(&records, sort);
        t.expect(&format!("sort {sort:?}"), listed_ids(b.list_items(sort, None, None).await), Ok(expected));
    }
    t
}

#[tokio::test]
async fn a_multi_key_order_with_mixed_directions_orders_alike() {
    parity!(multi_key_orders);
}

/// The rules spelled out, so that the reference comparator cannot be wrong
/// in the same way as both stores.
async fn pinned_orders<B: Backend>(b: &B, mut t: Transcript) -> Transcript {
    let records = sort_fixture();
    load(b, &mut t, &records).await;
    let cases: &[(&[&str], &[&str])] = &[
        // Byte order: "" < "B" < "Item c" < "a" < "z" < "é"; the two "B"s
        // and the two "a"s tie and fall back to id.
        (&["title"], &["g", "e", "j", "c", "b", "d", "a", "h"]),
        // Nulls first, an empty string is a value.
        (&["maybe_text"], &["c", "d", "h", "b", "e", "a", "g", "j"]),
        // Nulls last when descending; the tie-break stays ascending.
        (&["-maybe_text"], &["j", "g", "a", "e", "b", "c", "d", "h"]),
        // -0.0 equals 0.0: id decides between them, whichever way.
        (&["float64"], &["h", "b", "c", "d", "e", "g", "j", "a"]),
        (&["-float64"], &["a", "b", "c", "d", "e", "g", "j", "h"]),
        // Numeric, not text (9 < 10) and above i32::MAX.
        (&["maybe_u32"], &["a", "c", "g", "d", "h", "b", "e", "j"]),
        (&["-maybe_u32"], &["e", "j", "b", "h", "d", "a", "c", "g"]),
        // By stored string (alpha < beta < delta < gamma), not declaration
        // order (gamma, alpha, beta, zeta).
        (&["kind"], &["b", "c", "g", "j", "h", "a", "d", "e"]),
        (&["maybe_kind"], &["b", "c", "d", "a", "j", "e", "g", "h"]),
        // A requested id key replaces the tie-break.
        (&["-id"], &["j", "h", "g", "e", "d", "c", "b", "a"]),
        (&["flag", "-id"], &["j", "g", "c", "b", "a", "h", "e", "d"]),
    ];
    for (sort, expected) in cases {
        assert_eq!(reference_order(&records, sort), *expected, "the reference agrees on {sort:?}");
        t.expect(&format!("pinned {sort:?}"), listed_ids(b.list_items(sort, None, None).await), ok_ids(expected));
    }
    t
}

#[tokio::test]
async fn bytes_nulls_zeros_numbers_and_enums_order_as_adr_0006_says() {
    parity!(pinned_orders);
}

/// A limit/offset walk over an order whose first key ties across most
/// records: every page is the matching slice of the whole list.
async fn pages_across_ties<B: Backend>(b: &B, mut t: Transcript) -> Transcript {
    let mut records = Vec::new();
    for (i, id) in ["m", "c", "x", "a", "q", "b", "z", "k", "e", "t"].iter().enumerate() {
        let int32 = if i % 4 == 0 { 1 } else { 0 };
        records.push(item(id, json!({ "int32": int32, "flag": i % 3 == 0 })));
    }
    load(b, &mut t, &records).await;
    let sorts: &[&[&str]] = &[&["int32"], &["-int32"], &["flag", "-int32"], &["-flag"]];
    for sort in sorts {
        let whole = reference_order(&records, sort);
        t.expect(&format!("{sort:?} whole"), listed_ids(b.list_items(sort, None, None).await), Ok(whole.clone()));
        for limit in [1, 2, 3, 4] {
            for offset in (0..=whole.len()).step_by(limit) {
                let page = whole[offset..(offset + limit).min(whole.len())].to_vec();
                t.expect(
                    &format!("{sort:?} limit={limit} offset={offset}"),
                    listed_ids(b.list_items(sort, Some(limit as u64), Some(offset as u64)).await),
                    Ok(page),
                );
            }
        }
    }
    t
}

#[tokio::test]
async fn every_page_of_a_walk_across_ties_is_a_slice_of_the_order() {
    parity!(pages_across_ties);
}

/// `sort_items` over SeaORM's records fetched in other orders gives what
/// `list_items` gives: the in-memory comparator and the SQL agree, and
/// neither depends on the order rows arrive in.
#[tokio::test]
async fn sort_items_over_unordered_rows_matches_the_sql_order() {
    use parity_seaorm::persistence::db::entities::item as rows;
    use parity_seaorm::store::item::{ItemSortField, sort_items};
    use sea_orm::{EntityTrait, QueryOrder};

    let b = Sqlite::open().await;
    let mut t = Transcript::new::<Sqlite>();
    load(&b, &mut t, &sort_fixture()).await;
    let fetches = [
        rows::Entity::find().order_by_desc(rows::Column::Id),
        rows::Entity::find().order_by_asc(rows::Column::Title).order_by_desc(rows::Column::Int32),
        rows::Entity::find().order_by_desc(rows::Column::MaybeFloat64).order_by_asc(rows::Column::NU8),
    ];
    let orders: &[&[&str]] =
        &[&[], &["title"], &["-kind", "float32"], &["maybe_text", "-maybe_u32"], &["-flag", "-id"]];
    for fetch in fetches {
        let models = fetch.all(b.store.db()).await.expect("fetch");
        let fetched: Vec<parity_seaorm::schema::Item> =
            models.iter().map(parity_seaorm::schema::Item::from_model).collect::<Result<_, _>>().expect("decode");
        assert_ne!(
            fetched.iter().map(|i| i.id.as_str()).collect::<Vec<_>>(),
            ["a", "b", "c", "d", "e", "g", "h", "j"],
            "the rows arrive in some order other than id order"
        );
        for sort in orders {
            let order = ontogen_core::order::parse_sort::<ItemSortField>(*sort).expect("sort keys");
            let mut sorted = fetched.clone();
            sort_items(&mut sorted, &order);
            let in_memory: Vec<String> = sorted.into_iter().map(|i| i.id).collect();
            assert_eq!(Ok(in_memory), listed_ids(b.list_items(sort, None, None).await), "{sort:?}");
        }
    }
}

// ─── NaN ────────────────────────────────────────────────────────────────────

const FLOATS: &[&str] = &["float32", "float64", "maybe_float32", "maybe_float64"];

/// A NaN a write sets is refused on both backends with the catch-all, and
/// nothing is stored (ADR 0006 §3).
async fn nan_writes<B: Backend>(b: &B, mut t: Transcript) -> Transcript {
    for field in FLOATS {
        let label = format!("create with {field} NaN");
        let result = b.create_item_nan(item(&format!("nan-{field}"), json!({})), field).await;
        assert!(matches!(result, Err(StoreError::Backend(_))), "[{}] {label}: {result:?}", B::NAME);
        t.record(label, &result).unwrap_err();
    }
    t.expect("nothing was stored", b.count_items().await, Ok(0));

    let base = item("base", json!({ "float32": 1.5, "maybe_float64": 2.5 }));
    t.expect("create base", b.create_item(base.clone()).await, Ok(base.clone()));
    for field in FLOATS {
        let label = format!("update {field} to NaN");
        let result = b.update_item_nan("base", field).await;
        assert!(matches!(result, Err(StoreError::Backend(_))), "[{}] {label}: {result:?}", B::NAME);
        t.record(label, &result).unwrap_err();
    }
    t.expect("base is unchanged", b.get_item("base").await, Ok(base));
    t
}

#[tokio::test]
async fn a_nan_write_is_refused_and_stores_nothing() {
    parity!(nan_writes);
}

/// Markdown only, since SQLite cannot hold a NaN: one written into the file
/// by hand does not fail an update that leaves the field alone.
#[tokio::test]
async fn a_hand_written_nan_does_not_fail_an_update_that_leaves_it() {
    let b = Vault::open().await;
    b.create_item(item("hand", json!({ "float64": 1.5 }))).await.expect("create");
    let path = b.store.vault().root().join("items/hand.md");
    let text = std::fs::read_to_string(&path).expect("read");
    let edited = text.replace("float64: 1.5\n", "float64: .nan\n");
    assert_ne!(text, edited, "the field is in the file: {text}");
    std::fs::write(&path, edited).expect("write");

    let updated = b.update_item("hand", json!({ "title": "Renamed" })).await.expect("the update succeeds");
    assert_eq!(updated["title"], json!("Renamed"));
    assert_eq!(updated["float64"], Value::Null, "the NaN is still there (JSON has no NaN)");
    let refused = b.update_item_nan("hand", "float32").await;
    assert!(matches!(refused, Err(StoreError::Backend(_))), "setting a NaN is still refused: {refused:?}");
}

// ─── Create: ids ────────────────────────────────────────────────────────────

const EMPTY_SLUG: &str = "field \"title\" produced an empty slug";
const NO_ID: &str = "this store requires the caller to supply an id";

async fn create_ids<B: Backend>(b: &B, mut t: Transcript) -> Transcript {
    let created_id = |v: R<Value>| v.map(|r| r["id"].as_str().unwrap().to_string());
    let derive = |title: &str| item("", json!({ "title": title }));

    // Nothing to derive an id from: a blank id is absent, and the title slugs to nothing.
    for id in ["", " ", "\t", " \n "] {
        t.expect(
            &format!("id {id:?}, title that slugs to empty"),
            created_id(b.create_item(item(id, json!({ "title": "!!! ---" }))).await),
            Err(StoreError::IdRequired("Item", EMPTY_SLUG.into())),
        );
    }
    t.expect("nothing was written", b.count_items().await, Ok(0));

    // Derived ids probe past taken and reserved ones.
    t.expect("Same", created_id(b.create_item(derive("Same")).await), Ok("same".into()));
    t.expect("Same again", created_id(b.create_item(derive("Same")).await), Ok("same-2".into()));
    t.expect(
        "Same, blank id",
        created_id(b.create_item(item("  ", json!({ "title": "Same" }))).await),
        Ok("same-3".into()),
    );
    t.expect(
        "a provided id beside the run",
        created_id(b.create_item(item("same-4", json!({}))).await),
        Ok("same-4".into()),
    );
    t.expect("Same skips the provided one", created_id(b.create_item(derive("Same")).await), Ok("same-5".into()));
    t.expect("Index is reserved", created_id(b.create_item(derive("Index")).await), Ok("index-2".into()));
    t.expect("LOG is reserved", created_id(b.create_item(derive("LOG")).await), Ok("log-2".into()));
    t.expect("Index again", created_id(b.create_item(derive("index")).await), Ok("index-3".into()));

    // A provided id wins over the strategy, and a duplicate is refused.
    t.expect(
        "provided id",
        created_id(b.create_item(item("dup", json!({ "title": "First" }))).await),
        Ok("dup".into()),
    );
    t.expect(
        "duplicate provided id",
        created_id(b.create_item(item("dup", json!({ "title": "Second" }))).await),
        Err(StoreError::AlreadyExists("Item", "dup".into())),
    );
    t.expect("the original is untouched", b.get_item("dup").await.map(|r| r["title"].clone()), Ok(json!("First")));

    // An invalid provided id is a server-side bug: the backend's catch-all.
    // Uppercase, non-ASCII and over-long ids break the create rule like
    // path syntax does.
    let too_long = "a".repeat(201);
    for id in [
        "a/b",
        "index",
        "Log",
        ".hidden",
        "trailing.",
        "trailing ",
        "c:d",
        "B",
        "Zeta",
        "é",
        "Ω",
        "café",
        "a b",
        "a+b",
        too_long.as_str(),
    ] {
        let result = created_id(b.create_item(item(id, json!({}))).await);
        assert!(matches!(result, Err(StoreError::Backend(_))), "[{}] create {id:?}: {result:?}", B::NAME);
        t.record(format!("invalid id {id:?}"), &result).unwrap_err();
    }
    t.expect(
        "only the valid creates were stored",
        listed_ids(b.list_items(&[], None, None).await),
        ok_ids(&["dup", "index-2", "index-3", "log-2", "same", "same-2", "same-3", "same-4", "same-5"]),
    );

    // IdStrategy::Provided: a blank id has nothing to fall back on.
    for id in ["", "\t"] {
        t.expect(
            &format!("provided store, id {id:?}"),
            b.create_fixed(json!({ "id": id, "title": "Anything" })).await,
            Err(StoreError::IdRequired("Fixed", NO_ID.into())),
        );
    }
    t.expect("provided store, nothing written", b.count_fixeds().await, Ok(0));
    let fixed = json!({ "id": "f", "title": "One" });
    t.expect("provided store, an id", b.create_fixed(fixed.clone()).await, Ok(fixed.clone()));
    t.expect(
        "provided store, duplicate",
        b.create_fixed(json!({ "id": "f", "title": "Two" })).await,
        Err(StoreError::AlreadyExists("Fixed", "f".into())),
    );
    t.expect("provided store, original kept", b.get_fixed("f").await, Ok(fixed));
    t
}

#[tokio::test]
async fn create_fills_and_refuses_ids_the_same_way() {
    parity!(create_ids);
}

/// Whether `id` is a UUID v4 as both stores write it: lowercase hex in
/// 8-4-4-4-12 groups, version 4, RFC 4122 variant.
fn is_uuid_v4(id: &str) -> bool {
    let groups: Vec<&str> = id.split('-').collect();
    groups.iter().map(|g| g.len()).eq([8, 4, 4, 4, 12])
        && groups.iter().all(|g| g.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)))
        && groups[2].starts_with('4')
        && groups[3].starts_with(['8', '9', 'a', 'b'])
}

/// `result` with a minted `id` replaced by `"<uuid>"`, since each backend
/// mints its own.
fn minted(result: R<Value>) -> R<Value> {
    result.map(|mut record| {
        let id = record["id"].as_str().expect("an id");
        assert!(is_uuid_v4(id), "a minted id is a UUID v4: {id:?}");
        record["id"] = json!("<uuid>");
        record
    })
}

async fn uuid_ids<B: Backend>(b: &B, mut t: Transcript) -> Transcript {
    let stamped = |id: &str, title: &str| json!({ "id": id, "title": title });
    let minted_record = |title: &str| Ok(stamped("<uuid>", title));

    // A create without an id, blank or empty, mints one; a get finds it.
    let first = b.create_stamped(stamped("", "Same")).await;
    let first_id = first.as_ref().map(|r| r["id"].as_str().unwrap().to_string()).expect("created");
    t.expect("no id", minted(first), minted_record("Same"));
    t.expect("get the minted id", minted(b.get_stamped(&first_id).await), minted_record("Same"));
    let second = b.create_stamped(stamped(" \t", "Same")).await;
    let second_id = second.as_ref().map(|r| r["id"].as_str().unwrap().to_string()).expect("created");
    t.expect("blank id", minted(second), minted_record("Same"));
    t.expect("two creates mint two ids", Ok(first_id != second_id), Ok(true));

    // A provided id wins over the strategy, under the create rule.
    t.expect("provided id", b.create_stamped(stamped("kept", "Own")).await, Ok(stamped("kept", "Own")));
    t.expect(
        "duplicate provided id",
        b.create_stamped(stamped("kept", "Again")).await,
        Err(StoreError::AlreadyExists("Stamped", "kept".into())),
    );
    let refused = b.create_stamped(stamped("Kept", "Upper")).await;
    assert!(matches!(refused, Err(StoreError::Backend(_))), "[{}] create \"Kept\": {refused:?}", B::NAME);
    t.record("invalid provided id", &refused).unwrap_err();
    t.expect("three stored", b.count_stampeds().await, Ok(3));
    t
}

#[tokio::test]
async fn a_uuid_store_mints_a_uuid_v4_and_keeps_a_provided_id() {
    parity!(uuid_ids);
}

/// The 200-byte limit, and slugs cut to fit it, on provided and derived ids.
async fn id_length<B: Backend>(b: &B, mut t: Transcript) -> Transcript {
    let created_id = |v: R<Value>| v.map(|r| r["id"].as_str().unwrap().to_string());
    let derive = |title: &str| item("", json!({ "title": title }));

    let longest = "x".repeat(200);
    t.expect("a 200-byte id", created_id(b.create_item(item(&longest, json!({}))).await), Ok(longest.clone()));
    t.expect("get the 200-byte id", created_id(b.get_item(&longest).await), Ok(longest.clone()));
    let too_long = format!("{longest}x");
    let result = created_id(b.create_item(item(&too_long, json!({}))).await);
    assert!(matches!(result, Err(StoreError::Backend(_))), "[{}] a 201-byte id: {result:?}", B::NAME);
    t.record("a 201-byte id", &result).unwrap_err();
    t.expect(
        "the 201-byte id was not stored",
        b.get_item(&too_long).await,
        Err(StoreError::NotFound("Item", too_long)),
    );

    // A slug is cut to 190 bytes (a dangling `-` trimmed), whatever the
    // title's length, and probing keeps the cut base whole.
    let words = "Word ".repeat(100);
    let cut = "word-".repeat(38).trim_end_matches('-').to_string();
    assert_eq!(cut.len(), 189);
    t.expect("a long title", created_id(b.create_item(derive(&words)).await), Ok(cut.clone()));
    t.expect("the same long title", created_id(b.create_item(derive(&words)).await), Ok(format!("{cut}-2")));
    t.expect(
        "a longer title with the same first 190 bytes",
        created_id(b.create_item(derive(&format!("{words} and more"))).await),
        Ok(format!("{cut}-3")),
    );
    let run = "x".repeat(300);
    t.expect("a 300-character title", created_id(b.create_item(derive(&run)).await), Ok("x".repeat(190)));
    t.expect("again", created_id(b.create_item(derive(&run)).await), Ok(format!("{}-2", "x".repeat(190))));

    // Latin letters fold to ASCII; anything else separates.
    t.expect(
        "a title with diacritics",
        created_id(b.create_item(derive("Crème Brûlée à Łódź")).await),
        Ok("creme-brulee-a-lodz".into()),
    );
    t.expect("Straße", created_id(b.create_item(derive("Straße")).await), Ok("strasse".into()));
    t.expect("a title with no Latin letters", created_id(b.create_item(derive("日本 Ω 2")).await), Ok("2".into()));
    let folded = "é".repeat(120);
    t.expect("a long title of two-byte letters", created_id(b.create_item(derive(&folded)).await), Ok("e".repeat(120)));

    let mut expected = vec![
        "2".to_string(),
        cut.clone(),
        format!("{cut}-2"),
        format!("{cut}-3"),
        "creme-brulee-a-lodz".into(),
        "e".repeat(120),
        "strasse".into(),
        longest,
        "x".repeat(190),
        format!("{}-2", "x".repeat(190)),
    ];
    expected.sort_unstable();
    let listed = listed_ids(b.list_items(&[], None, None).await);
    assert_eq!(listed, Ok(expected.clone()), "[{}] list", B::NAME);
    t.record("list", &listed).unwrap();
    t
}

#[tokio::test]
async fn ids_are_at_most_200_bytes_and_long_slugs_are_cut_alike() {
    parity!(id_length);
}

/// Windows device names are refused as provided ids, whole or before the
/// first `.` and in any case; a title that slugs to one probes past it.
async fn device_names<B: Backend>(b: &B, mut t: Transcript) -> Transcript {
    let created_id = |v: R<Value>| v.map(|r| r["id"].as_str().unwrap().to_string());
    let derive = |title: &str| item("", json!({ "title": title }));

    for id in ["con", "nul.x", "com1.backup", "lpt9", "CON", "Aux.md", "com0", "Lpt0.txt"] {
        let result = created_id(b.create_item(item(id, json!({}))).await);
        let Err(StoreError::Backend(message)) = &result else {
            panic!("[{}] create {id:?}: {result:?}", B::NAME);
        };
        assert!(
            message.contains(&format!("invalid id {id:?}: is reserved: Windows has no file named con"))
                && message.ends_with("and not a reserved name: choose another id"),
            "[{}] the message states the clause and the rule: {message}",
            B::NAME
        );
        t.record(format!("device name {id:?}"), &result).unwrap_err();
        t.expect(&format!("{id:?} was not stored"), b.get_item(id).await, Err(StoreError::NotFound("Item", id.into())));
    }
    for id in ["console", "con-2x", "xcon", "a.con", "com10", "lpt10"] {
        t.expect(
            &format!("{id:?} is no device name"),
            created_id(b.create_item(item(id, json!({}))).await),
            Ok(id.into()),
        );
    }
    t.expect("Con derives con-2", created_id(b.create_item(derive("Con")).await), Ok("con-2".into()));
    t.expect("CON again derives con-3", created_id(b.create_item(derive("CON")).await), Ok("con-3".into()));
    t.expect("nul.x slugs to nul-x", created_id(b.create_item(derive("nul.x")).await), Ok("nul-x".into()));
    t.expect("LPT1 derives lpt1-2", created_id(b.create_item(derive("LPT1")).await), Ok("lpt1-2".into()));
    t.expect("COM0 derives com0-2", created_id(b.create_item(derive("COM0")).await), Ok("com0-2".into()));
    t.expect(
        "only the valid creates were stored",
        listed_ids(b.list_items(&[], None, None).await),
        ok_ids(&[
            "a.con", "com0-2", "com10", "con-2", "con-2x", "con-3", "console", "lpt1-2", "lpt10", "nul-x", "xcon",
        ]),
    );
    t
}

#[tokio::test]
async fn device_names_are_refused_and_derived_past_alike() {
    parity!(device_names);
}

// ─── Lookups ────────────────────────────────────────────────────────────────

async fn lookups<B: Backend>(b: &B, mut t: Transcript) -> Transcript {
    t.record("create kept", &b.create_item(item("kept", json!({}))).await).unwrap();
    // Ids no record can have (markdown cannot even hold them; SeaORM never
    // created them); ids a lookup accepts but no create could have made
    // (uppercase, non-ASCII, over 200 bytes); and a valid id that was never
    // created.
    let (long, longer) = ("l".repeat(201), "l".repeat(300));
    for id in [
        "index",
        "LOG",
        "a/b",
        ".hidden",
        "..",
        " ",
        "\t",
        "trailing.",
        "c:d",
        "Ghost",
        "NEVER",
        "é",
        "Ω",
        "a b",
        long.as_str(),
        longer.as_str(),
        "never",
    ] {
        let not_found = StoreError::NotFound("Item", id.to_string());
        t.expect(&format!("get {id:?}"), b.get_item(id).await, Err(not_found.clone()));
        t.expect(&format!("update {id:?}"), b.update_item(id, json!({ "title": "x" })).await, Err(not_found.clone()));
        t.expect(&format!("delete {id:?}"), b.delete_item(id).await, Err(not_found));
    }
    t.expect("delete kept", b.delete_item("kept").await, Ok(()));
    t.expect("get deleted", b.get_item("kept").await, Err(StoreError::NotFound("Item", "kept".into())));
    t.expect("delete again", b.delete_item("kept").await, Err(StoreError::NotFound("Item", "kept".into())));
    t.expect("count", b.count_items().await, Ok(0));
    t
}

#[tokio::test]
async fn lookups_of_missing_and_impossible_ids_are_not_found() {
    parity!(lookups);
}

/// Spellings of a stored id that macOS and Windows filesystems resolve to
/// the stored file: other letter cases, and the other Unicode
/// normalization form.
async fn spelling_variants<B: Backend>(b: &B, mut t: Transcript) -> Transcript {
    let kept = t.record("create kept", &b.create_item(item("kept", json!({}))).await).unwrap();
    t.record("create hand", &b.create_item(item("hand", json!({ "title": "Hand-named" }))).await).unwrap();
    let cafe = "caf\u{e9}";
    b.rename_item("hand", cafe).await;
    let hand = t.record("get the hand-named record", &b.get_item(cafe).await).unwrap();
    assert_eq!((hand["id"].as_str(), hand["title"].as_str()), (Some(cafe), Some("Hand-named")));

    for id in ["KEPT", "Kept", "kepT", "cafe\u{301}", "CAF\u{c9}", "Caf\u{e9}"] {
        let not_found = StoreError::NotFound("Item", id.to_string());
        t.expect(&format!("get {id:?}"), b.get_item(id).await, Err(not_found.clone()));
        t.expect(&format!("update {id:?}"), b.update_item(id, json!({ "title": "x" })).await, Err(not_found.clone()));
        t.expect(&format!("delete {id:?}"), b.delete_item(id).await, Err(not_found));
    }
    t.expect("kept is unchanged", b.get_item("kept").await, Ok(kept));
    t.expect("the hand-named record is unchanged", b.get_item(cafe).await, Ok(hand));
    t.expect("nothing was removed", listed_ids(b.list_items(&[], None, None).await), ok_ids(&[cafe, "kept"]));
    t
}

#[tokio::test]
async fn a_case_or_normalization_variant_of_an_id_is_not_found() {
    parity!(spelling_variants);
}

// ─── has_many writes ────────────────────────────────────────────────────────

async fn has_many_drop<B: Backend>(b: &B, mut t: Transcript) -> Transcript {
    t.record("create p", &b.create_item(item("p", json!({}))).await).unwrap();
    for kid in ["k1", "k2", "k3"] {
        t.record(format!("create {kid}"), &b.create_item(item(kid, json!({ "parent_id": "p" }))).await).unwrap();
    }
    t.record("create adopted", &b.create_item(item("adopted", json!({}))).await).unwrap();
    t.record("create other", &b.create_item(item("other", json!({}))).await).unwrap();
    t.record("create moved", &b.create_item(item("moved", json!({ "parent_id": "other" }))).await).unwrap();

    let children = |v: R<Value>| v.map(|r| r["children"].clone());
    let parent = |v: R<Value>| v.map(|r| r["parent_id"].clone());
    t.expect(
        "drop k2, adopt one, move one",
        children(b.update_item("p", json!({ "children": ["k3", "moved", "k1", "adopted"] })).await),
        Ok(json!(["adopted", "k1", "k3", "moved"])),
    );
    t.expect("the dropped child has no parent", parent(b.get_item("k2").await), Ok(Value::Null));
    t.expect("the adopted child points at p", parent(b.get_item("adopted").await), Ok(json!("p")));
    t.expect("the moved child points at p", parent(b.get_item("moved").await), Ok(json!("p")));
    t.expect("its old parent lost it", children(b.get_item("other").await), Ok(json!([])));
    t.expect(
        "an update without children leaves them",
        children(b.update_item("p", json!({ "title": "P" })).await),
        Ok(json!(["adopted", "k1", "k3", "moved"])),
    );
    t.expect("drop them all", children(b.update_item("p", json!({ "children": [] })).await), Ok(json!([])));
    for kid in ["adopted", "k1", "k3", "moved"] {
        t.expect(&format!("{kid} has no parent"), parent(b.get_item(kid).await), Ok(Value::Null));
    }
    t
}

#[tokio::test]
async fn a_has_many_update_clears_dropped_children() {
    parity!(has_many_drop);
}

async fn required_parent<B: Backend>(b: &B, mut t: Transcript) -> Transcript {
    for (id, parent) in [("book", "book"), ("ch2", "book"), ("ch1", "book"), ("appendix", "appendix")] {
        t.record(format!("create {id}"), &b.create_section(section(id, parent)).await).unwrap();
    }
    let before = b.list_sections().await;
    t.expect(
        "a root is not its own child",
        before.clone().map(|all| all.iter().map(|s| json!([s["id"], s["children"]])).collect::<Vec<_>>()),
        Ok(vec![json!(["appendix", []]), json!(["book", ["ch1", "ch2"]]), json!(["ch1", []]), json!(["ch2", []])]),
    );

    t.expect(
        "dropping ch2 is refused",
        b.update_section("book", json!({ "title": "Renamed", "children": ["ch1"] })).await,
        Err(StoreError::ParentRequired("Section", "ch2".into())),
    );
    t.expect("nothing was written, the record's own title included", b.list_sections().await, before);

    let children = |v: R<Value>| v.map(|r| r["children"].clone());
    t.expect(
        "an update that drops nothing may add a child",
        children(b.update_section("book", json!({ "children": ["ch2", "appendix", "ch1"] })).await),
        Ok(json!(["appendix", "ch1", "ch2"])),
    );
    // A root is its own parent through its belongs_to, never through its
    // own children list.
    let roots = b.list_sections().await;
    t.expect(
        "listing the record itself as a child is refused",
        b.update_section("book", json!({ "title": "Renamed", "children": ["book", "appendix", "ch1", "ch2"] })).await,
        Err(StoreError::ParentCycle("Section", "book".into())),
    );
    t.expect("the refused update wrote nothing", b.list_sections().await, roots);
    t.record("final state", &b.list_sections().await).unwrap();
    t
}

#[tokio::test]
async fn dropping_a_required_child_writes_nothing() {
    parity!(required_parent);
}

/// A listed child that does not exist fails the write before anything is
/// written, on create and on update (JSON:API wire contract §5.4).
async fn missing_children<B: Backend>(b: &B, mut t: Transcript) -> Transcript {
    t.record("create p", &b.create_item(item("p", json!({}))).await).unwrap();
    for kid in ["k1", "k2"] {
        t.record(format!("create {kid}"), &b.create_item(item(kid, json!({ "parent_id": "p" }))).await).unwrap();
    }
    t.record("create loose", &b.create_item(item("loose", json!({}))).await).unwrap();
    let before = b.list_items(&[], None, None).await;
    let not_found = |id: &str| Err(StoreError::NotFound("Item", id.into()));

    // Create: the first missing child in list order, and no record.
    t.expect(
        "create listing missing children",
        b.create_item(item("q", json!({ "children": ["loose", "ghost", "k1", "ghost-2"] }))).await,
        not_found("ghost"),
    );
    t.expect("the failed create wrote no record", b.get_item("q").await, not_found("q"));
    t.expect(
        "an id no record can have is missing too",
        b.create_item(item("q", json!({ "children": ["a/b"] }))).await,
        not_found("a/b"),
    );
    t.expect(
        "the missing child is reported before an id is derived",
        b.create_item(item("", json!({ "title": "!!!", "children": ["ghost"] }))).await,
        not_found("ghost"),
    );
    t.expect("nothing was written", b.list_items(&[], None, None).await, before.clone());

    // Update: the record and every child unchanged.
    t.expect(
        "update listing a missing child",
        b.update_item("p", json!({ "title": "Renamed", "children": ["k1", "loose", "ghost"] })).await,
        not_found("ghost"),
    );
    t.expect("nothing was updated", b.list_items(&[], None, None).await, before);

    // A child listed twice is not a missing one.
    let children = |v: R<Value>| v.map(|r| r["children"].clone());
    t.expect(
        "create listing a child twice",
        children(b.create_item(item("r", json!({ "children": ["loose", "loose"] }))).await),
        Ok(json!(["loose"])),
    );
    t.expect(
        "update listing a child twice",
        children(b.update_item("p", json!({ "children": ["k2", "k1", "k2"] })).await),
        Ok(json!(["k1", "k2"])),
    );

    // On a required foreign key the missing child is reported before a drop.
    for (id, parent) in [("book", "book"), ("ch1", "book"), ("ch2", "book")] {
        t.record(format!("create {id}"), &b.create_section(section(id, parent)).await).unwrap();
    }
    let sections = b.list_sections().await;
    t.expect(
        "create a section listing a missing child",
        b.create_section(section_with("other", "other", &["ch1", "ghost"])).await,
        Err(StoreError::NotFound("Section", "ghost".into())),
    );
    t.expect(
        "an update that drops ch2 and lists a missing child",
        b.update_section("book", json!({ "title": "Renamed", "children": ["ch1", "ghost"] })).await,
        Err(StoreError::NotFound("Section", "ghost".into())),
    );
    t.expect("no section changed", b.list_sections().await, sections);
    t
}

#[tokio::test]
async fn a_missing_child_is_not_found_and_writes_nothing() {
    parity!(missing_children);
}

/// A has_many list that names the record itself is refused before anything
/// is written, on create (an explicit id) and on update: a record cannot be
/// its own child (JSON:API wire contract §5.4).
async fn self_listing<B: Backend>(b: &B, mut t: Transcript) -> Transcript {
    t.record("create a", &b.create_item(item("a", json!({}))).await).unwrap();
    t.record("create k1", &b.create_item(item("k1", json!({ "parent_id": "a" }))).await).unwrap();
    t.record("create loose", &b.create_item(item("loose", json!({}))).await).unwrap();
    let before = b.list_items(&[], None, None).await;
    let cycle = |id: &str| Err(StoreError::ParentCycle("Item", id.into()));

    t.expect(
        "update listing the record itself",
        b.update_item("a", json!({ "title": "Renamed", "children": ["k1", "loose", "a"] })).await,
        cycle("a"),
    );
    t.expect("nothing was updated, a's own parent included", b.list_items(&[], None, None).await, before.clone());
    t.expect(
        "the self-listing is reported before a missing child",
        b.update_item("a", json!({ "children": ["ghost", "a"] })).await,
        cycle("a"),
    );
    t.expect(
        "create with an explicit id listing itself",
        b.create_item(item("b", json!({ "children": ["loose", "b"] }))).await,
        cycle("b"),
    );
    t.expect(
        "the refused create wrote no record",
        b.get_item("b").await,
        Err(StoreError::NotFound("Item", "b".into())),
    );
    t.expect("nothing was written", b.list_items(&[], None, None).await, before);

    for (id, parent) in [("book", "book"), ("ch1", "book")] {
        t.record(format!("create section {id}"), &b.create_section(section(id, parent)).await).unwrap();
    }
    let sections = b.list_sections().await;
    t.expect(
        "a section listing itself",
        b.update_section("book", json!({ "children": ["ch1", "book"] })).await,
        Err(StoreError::ParentCycle("Section", "book".into())),
    );
    t.expect(
        "a root created listing itself",
        b.create_section(section_with("root", "root", &["root"])).await,
        Err(StoreError::ParentCycle("Section", "root".into())),
    );
    t.expect("no section changed", b.list_sections().await, sections);
    t
}

#[tokio::test]
async fn a_has_many_list_naming_the_record_itself_is_refused() {
    parity!(self_listing);
}

// ─── many_to_many writes ────────────────────────────────────────────────────

/// A many_to_many id with no record behind it fails the create or update
/// before anything is written, on both backends: SQLite's junction foreign
/// key would refuse it, and the markdown store checks it the same way
/// (JSON:API wire contract §5.4).
async fn missing_tags<B: Backend>(b: &B, mut t: Transcript) -> Transcript {
    t.record("create tag t1", &b.create_tag(json!({ "id": "t1", "title": "t1" })).await).unwrap();
    t.record("create other", &b.create_item(item("other", json!({ "tags": ["t1"] }))).await).unwrap();
    let before = b.list_items(&[], None, None).await;
    fn missing<T>(id: &str) -> R<T> {
        Err(StoreError::NotFound("Tag", id.into()))
    }

    t.expect(
        "create listing a missing tag",
        b.create_item(item("q", json!({ "tags": ["t1", "ghost", "ghost-2"] }))).await,
        missing("ghost"),
    );
    t.expect("the failed create wrote no record", b.get_item("q").await, Err(StoreError::NotFound("Item", "q".into())));
    t.expect("nothing was written", b.list_items(&[], None, None).await, before.clone());

    t.expect(
        "update listing a missing tag",
        b.update_item("other", json!({ "title": "Renamed", "tags": ["ghost", "t1"] })).await,
        missing("ghost"),
    );
    t.expect("nothing was updated, the title included", b.list_items(&[], None, None).await, before.clone());

    // Relations are checked in declaration order: `children` before `tags`.
    t.expect(
        "a missing child and a missing tag",
        b.create_item(item("q", json!({ "children": ["lost"], "tags": ["ghost"] }))).await,
        Err(StoreError::NotFound("Item", "lost".into())),
    );
    t.expect(
        "the missing tag is reported before an id is derived",
        b.create_item(item("", json!({ "title": "!!!", "tags": ["ghost"] }))).await,
        missing("ghost"),
    );

    // A failed derived-id create leaves nothing behind for the retry to probe past.
    let created_id = |v: R<Value>| v.map(|r| r["id"].as_str().unwrap().to_string());
    t.expect(
        "a derived-id create with a missing tag",
        created_id(b.create_item(item("", json!({ "title": "Same", "tags": ["ghost"] }))).await),
        missing("ghost"),
    );
    t.expect(
        "the retry gets the base slug",
        created_id(b.create_item(item("", json!({ "title": "Same", "tags": ["t1"] }))).await),
        Ok("same".into()),
    );
    t.expect("listed", listed_ids(b.list_items(&[], None, None).await), ok_ids(&["other", "same"]));
    t
}

#[tokio::test]
async fn a_missing_many_to_many_target_is_not_found_and_writes_nothing() {
    parity!(missing_tags);
}

// ─── SeaORM: one transaction per write ──────────────────────────────────────

/// Makes every junction insert fail on SQLite, after the pre-checks have
/// passed and the record's own row is written.
async fn refuse_junction_inserts(b: &Sqlite) {
    use sea_orm::ConnectionTrait;
    // sqlite-only: RAISE in a trigger is SQLite's way to fail a statement on demand.
    let sql = "CREATE TRIGGER refuse_tags BEFORE INSERT ON item_tags BEGIN SELECT RAISE(ABORT, 'refused'); END";
    b.store.db().execute_unprepared(sql).await.expect("trigger");
}

/// SeaORM only, since the markdown store has no transactions: a create or
/// update whose junction write fails after the record's row is written
/// leaves nothing written.
#[tokio::test]
async fn a_failed_junction_write_rolls_back_the_whole_write() {
    use sea_orm::ConnectionTrait;
    let b = Sqlite::open().await;
    b.create_tag(json!({ "id": "t1", "title": "t1" })).await.expect("tag");
    b.create_tag(json!({ "id": "t2", "title": "t2" })).await.expect("tag");
    b.create_item(item("kept", json!({ "tags": ["t1"] }))).await.expect("kept");
    let kept = b.get_item("kept").await.expect("kept");
    refuse_junction_inserts(&b).await;

    let created = b.create_item(item("", json!({ "title": "Same", "tags": ["t2"] }))).await;
    assert!(matches!(created, Err(StoreError::Backend(_))), "{created:?}");
    assert_eq!(listed_ids(b.list_items(&[], None, None).await), ok_ids(&["kept"]), "the row was rolled back");

    let updated = b.update_item("kept", json!({ "title": "Renamed", "tags": ["t2"] })).await;
    assert!(matches!(updated, Err(StoreError::Backend(_))), "{updated:?}");
    assert_eq!(b.get_item("kept").await, Ok(kept), "the title and the old junction rows are back");

    b.store.db().execute_unprepared("DROP TRIGGER refuse_tags").await.expect("drop trigger");
    let retried = b.create_item(item("", json!({ "title": "Same", "tags": ["t2"] }))).await.expect("retry");
    assert_eq!(retried["id"], json!("same"), "no row was left for the retry to probe past");
}

/// SeaORM only: derived-id creates racing on a file-backed SQLite database
/// with a pool of several connections all succeed. SQLite answers a
/// transaction that read first and then wants the write lock another
/// connection holds with an immediate "database is locked", which the busy
/// timeout does not wait out; a transaction whose first statement is a write
/// waits for the lock instead. The pool keeps sqlx's 5 second busy timeout,
/// far longer than 80 small serialized writes take.
#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn concurrent_derived_id_creates_all_succeed_on_a_pooled_database() {
    use std::sync::Arc;
    const RACERS: usize = 40;
    let dir = tempfile::tempdir().expect("tempdir");
    // sqlite-only: a file-backed SQLite URL; `mode=rwc` creates the file.
    let url = format!("sqlite://{}?mode=rwc", dir.path().join("parity.db").display());
    let store = Arc::new(parity_seaorm::Store::open_url(&url, 8).await.expect("sqlite"));
    let start = Arc::new(tokio::sync::Barrier::new(2 * RACERS));

    let mut slugged = Vec::new();
    let mut stamped = Vec::new();
    for _ in 0..RACERS {
        let (items, gate) = (store.clone(), start.clone());
        slugged.push(tokio::spawn(async move {
            let record = serde_json::from_value(item("", json!({ "title": "Same" }))).expect("an Item");
            gate.wait().await;
            items.create_item(record).await.map(|created| created.id).map_err(|e| format!("{e:?}"))
        }));
        let (stampeds, gate) = (store.clone(), start.clone());
        stamped.push(tokio::spawn(async move {
            let record = serde_json::from_value(json!({ "id": "", "title": "Stamped" })).expect("a Stamped");
            gate.wait().await;
            stampeds.create_stamped(record).await.map(|created| created.id).map_err(|e| format!("{e:?}"))
        }));
    }

    let outcomes = |tasks: Vec<tokio::task::JoinHandle<Result<String, String>>>| async move {
        let mut ids = std::collections::BTreeSet::new();
        let mut failures = Vec::new();
        for task in tasks {
            match task.await.expect("task") {
                Ok(id) => assert!(ids.insert(id.clone()), "{id} was handed out twice"),
                Err(e) => failures.push(e),
            }
        }
        (ids, failures)
    };
    let (slug_ids, slug_failures) = outcomes(slugged).await;
    let (uuid_ids, uuid_failures) = outcomes(stamped).await;
    assert!(
        slug_failures.is_empty() && uuid_failures.is_empty(),
        "{}/{RACERS} slug and {}/{RACERS} uuid creates succeeded; the first failure: {:?}",
        slug_ids.len(),
        uuid_ids.len(),
        slug_failures.first().or(uuid_failures.first()),
    );
    let expected: std::collections::BTreeSet<String> =
        std::iter::once("same".to_string()).chain((2..=RACERS).map(|n| format!("same-{n}"))).collect();
    assert_eq!(slug_ids, expected, "the slug ids are the base and its first suffixes, none skipped");
    assert_eq!(uuid_ids.len(), RACERS);
    assert_eq!(store.count_items().await.expect("count"), RACERS as u64);
    assert_eq!(store.count_stampeds().await.expect("count"), RACERS as u64);
}

// ─── Names the generated code could collide with ────────────────────────────

/// `Doc` (the markdown store's own binding), `Order` (SeaORM's
/// `sea_query::Order`), `Match` (a keyword module) and the keyword
/// relationships `r#in` (an SQL keyword column too) and `r#loop` behave as
/// any other entity and relationship: the `has_many` tree writes its keyword
/// foreign key, and the `many_to_many` keeps its written order. Both refuse
/// a self-listing and a missing target as any other relationship does.
async fn hostile_names<B: Backend>(b: &B, mut t: Transcript) -> Transcript {
    let doc =
        |id: &str, parent: Option<&str>| json!({ "id": id, "title": id, "in": parent, "children": [], "body": "" });
    t.record("create root", &b.create_doc(doc("root", None)).await).ok();
    t.record("create a", &b.create_doc(doc("a", Some("root"))).await).ok();
    t.record("create b", &b.create_doc(doc("b", Some("root"))).await).ok();
    let children = |r: R<Value>| r.map(|d| d["children"].clone());
    t.expect("root lists its children", children(b.get_doc("root").await), Ok(json!(["a", "b"])));
    let updated = b.update_doc("root", json!({ "children": ["a"] })).await;
    t.expect("an update drops b", children(updated), Ok(json!(["a"])));
    t.expect("b has no parent", b.get_doc("b").await.map(|d| d["in"].clone()), Ok(Value::Null));

    for id in ["m1", "m2"] {
        t.record(format!("create {id}"), &b.create_match(json!({ "id": id, "title": id })).await).ok();
    }
    let looped = |r: R<Value>| r.map(|o| o["loop"].clone());
    let created = b.create_order(json!({ "id": "o", "title": "o", "loop": ["m2", "m1"] })).await;
    t.expect("an order keeps its matches in order", looped(created), Ok(json!(["m2", "m1"])));
    let updated = b.update_order("o", json!({ "loop": ["m1"] })).await;
    t.expect("an update replaces them", looped(updated), Ok(json!(["m1"])));
    t.expect(
        "a doc listing itself",
        b.update_doc("root", json!({ "children": ["a", "root"] })).await,
        Err(StoreError::ParentCycle("Doc", "root".into())),
    );
    t.expect(
        "an order listing a missing match",
        b.update_order("o", json!({ "loop": ["m2", "ghost"] })).await,
        Err(StoreError::NotFound("Match", "ghost".into())),
    );
    t.expect("the refused update kept the order's matches", looped(b.get_order("o").await), Ok(json!(["m1"])));
    t
}

#[tokio::test]
async fn entities_and_relationships_named_like_generated_code_work() {
    parity!(hostile_names);
}
