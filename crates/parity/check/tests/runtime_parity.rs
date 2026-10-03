//! One schema, two backends, identical results (ADR 0006 §8, the phase 1a
//! cases; JSON:API wire contract §5.4 and §8.2).
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
    Backend(String),
}

impl PartialEq for StoreError {
    fn eq(&self, other: &Self) -> bool {
        use StoreError::*;
        match (self, other) {
            (NotFound(a, x), NotFound(b, y))
            | (IdRequired(a, x), IdRequired(b, y))
            | (AlreadyExists(a, x), AlreadyExists(b, y))
            | (ParentRequired(a, x), ParentRequired(b, y)) => a == b && x == y,
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
    async fn list_items(&self, limit: Option<u64>, offset: Option<u64>) -> R<Vec<Value>>;
    async fn count_items(&self) -> R<u64>;

    async fn create_tag(&self, tag: Value) -> R<Value>;

    async fn create_section(&self, section: Value) -> R<Value>;
    async fn update_section(&self, id: &str, patch: Value) -> R<Value>;
    async fn list_sections(&self) -> R<Vec<Value>>;

    async fn create_fixed(&self, fixed: Value) -> R<Value>;
    async fn get_fixed(&self, id: &str) -> R<Value>;
    async fn count_fixeds(&self) -> R<u64>;

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
        async fn list_items(&self, limit: Option<u64>, offset: Option<u64>) -> R<Vec<Value>> {
            self.store.list_items(limit, offset).await.map(|v| v.into_iter().map(to_json).collect()).map_err(err)
        }
        async fn count_items(&self) -> R<u64> {
            self.store.count_items().await.map_err(err)
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
            self.store.list_sections(None, None).await.map(|v| v.into_iter().map(to_json).collect()).map_err(err)
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
    };
}

macro_rules! error_mappers {
    ($krate:ident, $other:ident) => {
        fn err(e: $krate::schema::AppError) -> StoreError {
            use $krate::schema::AppError as E;
            match e {
                E::FixedNotFound(id) => StoreError::NotFound("Fixed", id),
                E::FixedIdRequired(r) => StoreError::IdRequired("Fixed", r),
                E::FixedAlreadyExists(id) => StoreError::AlreadyExists("Fixed", id),
                E::ItemNotFound(id) => StoreError::NotFound("Item", id),
                E::ItemIdRequired(r) => StoreError::IdRequired("Item", r),
                E::ItemAlreadyExists(id) => StoreError::AlreadyExists("Item", id),
                E::SectionNotFound(id) => StoreError::NotFound("Section", id),
                E::SectionIdRequired(r) => StoreError::IdRequired("Section", r),
                E::SectionAlreadyExists(id) => StoreError::AlreadyExists("Section", id),
                E::SectionParentRequired(id) => StoreError::ParentRequired("Section", id),
                E::TagNotFound(id) => StoreError::NotFound("Tag", id),
                E::TagIdRequired(r) => StoreError::IdRequired("Tag", r),
                E::TagAlreadyExists(id) => StoreError::AlreadyExists("Tag", id),
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
        store: parity_seaorm::Store,
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
        store: parity_markdown::Store,
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

    t.expect("list", listed_ids(b.list_items(None, None).await), ok_ids(&expected));
    t.expect("count", b.count_items().await, Ok(expected.len() as u64));

    let n = expected.len() as u64;
    for limit in [0, 1, 2, 3, 5, n - 1, n, n + 5] {
        let mut offset = 0;
        loop {
            let start = (offset as usize).min(expected.len());
            let end = (start + limit as usize).min(expected.len());
            t.expect(
                &format!("page limit={limit} offset={offset}"),
                listed_ids(b.list_items(Some(limit), Some(offset)).await),
                ok_ids(&expected[start..end]),
            );
            if offset > n || limit == 0 {
                break;
            }
            offset += limit;
        }
    }
    t.expect("offset past the end", listed_ids(b.list_items(None, Some(n + 1)).await), ok_ids(&[]));
    t.expect("offset only", listed_ids(b.list_items(None, Some(n - 2)).await), ok_ids(&expected[expected.len() - 2..]));
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
            listed_ids(b.list_items(limit, offset).await),
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
    t.expect("list", b.list_items(None, None).await, Ok(vec![max.clone(), none.clone(), wide.clone()]));

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
        .list_items(None, None)
        .await
        .map(|all| all.into_iter().map(|r| json!([r["id"], r["children"], r["tags"]])).collect::<Vec<_>>());
    t.record("every record's linkage, listed", &listed).unwrap();
    t
}

#[tokio::test]
async fn has_many_is_id_ascending_and_many_to_many_is_written_order() {
    parity!(linkage_order);
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
        listed_ids(b.list_items(None, None).await),
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
    let listed = listed_ids(b.list_items(None, None).await);
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

    for id in ["con", "nul.x", "com1.backup", "lpt9", "CON", "Aux.md"] {
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
    for id in ["console", "con-2x", "xcon", "a.con", "com10"] {
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
    t.expect(
        "only the valid creates were stored",
        listed_ids(b.list_items(None, None).await),
        ok_ids(&["a.con", "com10", "con-2", "con-2x", "con-3", "console", "lpt1-2", "nul-x", "xcon"]),
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
    t.expect("nothing was removed", listed_ids(b.list_items(None, None).await), ok_ids(&[cafe, "kept"]));
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
    t.expect(
        "listing the record itself as a child keeps it out of the list",
        children(b.update_section("book", json!({ "children": ["book", "appendix", "ch1", "ch2"] })).await),
        Ok(json!(["appendix", "ch1", "ch2"])),
    );
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
    let before = b.list_items(None, None).await;
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
    t.expect("nothing was written", b.list_items(None, None).await, before.clone());

    // Update: the record and every child unchanged.
    t.expect(
        "update listing a missing child",
        b.update_item("p", json!({ "title": "Renamed", "children": ["k1", "loose", "ghost"] })).await,
        not_found("ghost"),
    );
    t.expect("nothing was updated", b.list_items(None, None).await, before);

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
