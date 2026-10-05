//! The integer range check both backends emit into `create_*` and
//! `update_*`, next to the NaN check and before an id is derived.
//!
//! Every integer field is stored in an `i64` column on SeaORM (ADR 0006
//! §4), so a `u64`, `usize`, `u128`, `isize` or `i128` value outside `i64`
//! cannot be stored there. A vault could hold it, but both backends refuse
//! it through their own catch-all, at the same point, so a write that has
//! more than one thing wrong with it gets the same error on either.
//!
//! The value can arrive over IPC or MCP, whose payloads deserialize any
//! `u64`; HTTP refuses it earlier, in the body check.

use crate::persistence::seaorm::gen_entity::wide_integer_type;
use crate::schema::model::{EntityDef, FieldDef, FieldRole};
use crate::store::nan::Skipped;

/// Where [`emit_integer_range_checks`] reads the values.
pub(crate) enum IntegerSource<'a> {
    /// Every stored integer field of the record being created, bound to this
    /// variable.
    Record(&'a str),
    /// The integer fields an `{Entity}Update` named `updates` sets. It has no
    /// `#[ontology(skip)]` field, so an update never sets one.
    Updates,
}

/// The stored integer fields whose `i64::try_from` can fail, and whether
/// each is an `Option`.
fn wide_integer_fields<'a>(
    entity: &'a EntityDef,
    source: &IntegerSource<'_>,
    skipped: Skipped,
) -> impl Iterator<Item = (&'a FieldDef, bool)> {
    let skip_set = matches!(source, IntegerSource::Record(_)) && skipped == Skipped::Stored;
    entity
        .fields
        .iter()
        .filter(move |f| match f.role {
            FieldRole::Plain | FieldRole::EnumField => true,
            FieldRole::Skip => skip_set,
            _ => false,
        })
        .filter_map(|f| wide_integer_type(&f.field_type).map(|optional| (f, optional)))
}

/// Emit one refusal per wide integer field `source` sets. `refuse` turns the
/// message expression (a `String`) into the backend's error expression. The
/// message is the one SeaORM's column conversion gives.
pub(crate) fn emit_integer_range_checks(
    code: &mut String,
    entity: &EntityDef,
    source: IntegerSource<'_>,
    skipped: Skipped,
    refuse: impl Fn(&str) -> String,
) {
    let mut any = false;
    for (field, optional) in wide_integer_fields(entity, &source, skipped) {
        let f = &field.name;
        let message = format!("format!(\"{}.{f}: value {{v}} is out of range for i64\")", entity.name);
        let value = match (&source, optional) {
            (IntegerSource::Record(var), false) => format!("Some({var}.{f})"),
            (IntegerSource::Record(var), true) => format!("{var}.{f}"),
            (IntegerSource::Updates, false) => format!("updates.{f}"),
            (IntegerSource::Updates, true) => format!("updates.{f}.flatten()"),
        };
        code.push_str(&format!("        if let Some(v) = {value}.filter(|v| i64::try_from(*v).is_err()) {{\n"));
        code.push_str(&format!("            return Err({});\n", refuse(&message)));
        code.push_str("        }\n");
        any = true;
    }
    if any {
        code.push('\n');
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::model::FieldType;

    fn entity() -> EntityDef {
        EntityDef {
            name: "Counter".to_string(),
            directory: "counters".to_string(),
            table: "counters".to_string(),
            type_name: "counter".to_string(),
            prefix: "counter".to_string(),
            id_strategy: None,
            fields: vec![
                FieldDef::new("id", FieldType::String, FieldRole::Id),
                FieldDef::new("seq", FieldType::Other("u64".into()), FieldRole::Plain),
                FieldDef::new("cap", FieldType::OptionEnum("u128".into()), FieldRole::Plain),
                FieldDef::new("small", FieldType::Other("u32".into()), FieldRole::Plain),
                FieldDef::new("hidden", FieldType::Other("usize".into()), FieldRole::Skip),
            ],
            doc: String::new(),
        }
    }

    fn checks(source: IntegerSource<'_>, skipped: Skipped) -> String {
        let mut code = String::new();
        emit_integer_range_checks(&mut code, &entity(), source, skipped, |m| format!("E({m})"));
        code
    }

    #[test]
    fn a_create_checks_every_wide_integer_the_backend_stores() {
        let code = checks(IntegerSource::Record("counter"), Skipped::Stored);
        for expected in [
            "if let Some(v) = Some(counter.seq).filter(|v| i64::try_from(*v).is_err()) {",
            "if let Some(v) = counter.cap.filter(|v| i64::try_from(*v).is_err()) {",
            "if let Some(v) = Some(counter.hidden).filter(|v| i64::try_from(*v).is_err()) {",
            r#"return Err(E(format!("Counter.seq: value {v} is out of range for i64")));"#,
        ] {
            assert!(code.contains(expected), "missing `{expected}`:\n{code}");
        }
        assert!(!code.contains("small"), "a u32 always fits: {code}");
        let markdown = checks(IntegerSource::Record("counter"), Skipped::NotStored);
        assert!(!markdown.contains("hidden"), "a skipped field is not in the file: {markdown}");
    }

    #[test]
    fn an_update_checks_only_what_it_sets() {
        let code = checks(IntegerSource::Updates, Skipped::Stored);
        assert!(code.contains("if let Some(v) = updates.seq.filter(|v| i64::try_from(*v).is_err()) {"), "{code}");
        assert!(code.contains("if let Some(v) = updates.cap.flatten().filter(|v| i64::try_from(*v).is_err()) {"));
        assert!(!code.contains("hidden"), "an update cannot set a skipped field: {code}");
    }
}
