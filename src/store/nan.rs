//! The NaN write check both backends emit into `create_*` and `update_*`
//! (ADR 0006 §3).
//!
//! No transport can send a NaN (JSON has none), so one that reaches a write
//! came from a hook or a direct store caller. SQLite reads a stored NaN back
//! as `NULL`, which a non-`Option` field cannot decode, so no order could
//! make the backends agree on it: both refuse it before storage, through
//! their own catch-all error. Only the fields a write sets are checked, so a
//! NaN hand-written into a vault file does not fail every later update.

use crate::schema::model::{EntityDef, FieldDef, FieldRole, FieldType};

/// Where [`emit_nan_checks`] reads the values.
pub(crate) enum FloatSource<'a> {
    /// Every float field of the record being created, bound to this variable.
    Record(&'a str),
    /// The float fields an `{Entity}Update` named `updates` sets. It has no
    /// `#[ontology(skip)]` field, so an update never sets one.
    Updates,
}

/// Whether a backend stores `#[ontology(skip)]` fields: SeaORM gives each a
/// column, markdown writes none to the file.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Skipped {
    Stored,
    NotStored,
}

/// The float fields `source` sets that the backend stores: `f32`, `f64`
/// and their `Option` forms, with the scalar type and whether the field is
/// optional.
fn float_fields<'a>(
    entity: &'a EntityDef,
    source: &FloatSource<'_>,
    skipped: Skipped,
) -> impl Iterator<Item = (&'a FieldDef, &'static str, bool)> {
    let skip_set = matches!(source, FloatSource::Record(_)) && skipped == Skipped::Stored;
    entity
        .fields
        .iter()
        .filter(move |f| match f.role {
            FieldRole::Plain | FieldRole::EnumField => true,
            FieldRole::Skip => skip_set,
            _ => false,
        })
        .filter_map(|f| match f.field_type {
            FieldType::F32 => Some((f, "f32", false)),
            FieldType::F64 => Some((f, "f64", false)),
            FieldType::OptionF32 => Some((f, "f32", true)),
            FieldType::OptionF64 => Some((f, "f64", true)),
            _ => None,
        })
}

/// Emit one refusal per stored float field `source` sets. `refuse` turns
/// the message expression (a `String`) into the backend's error expression.
pub(crate) fn emit_nan_checks(
    code: &mut String,
    entity: &EntityDef,
    source: FloatSource<'_>,
    skipped: Skipped,
    refuse: impl Fn(&str) -> String,
) {
    let mut any = false;
    for (field, ty, optional) in float_fields(entity, &source, skipped) {
        let f = &field.name;
        let value = match (&source, optional) {
            (FloatSource::Record(var), false) => format!("{var}.{f}.is_nan()"),
            (FloatSource::Record(var), true) => format!("{var}.{f}.is_some_and({ty}::is_nan)"),
            (FloatSource::Updates, false) => format!("updates.{f}.is_some_and({ty}::is_nan)"),
            (FloatSource::Updates, true) => format!("updates.{f}.flatten().is_some_and({ty}::is_nan)"),
        };
        let message = format!("{:?}.to_string()", format!("{}.{f}: NaN cannot be stored", entity.name));
        code.push_str(&format!("        if {value} {{\n"));
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

    fn entity() -> EntityDef {
        EntityDef {
            name: "Reading".to_string(),
            directory: "readings".to_string(),
            table: "readings".to_string(),
            type_name: "reading".to_string(),
            prefix: "reading".to_string(),
            id_strategy: None,
            fields: vec![
                FieldDef::new("id", FieldType::String, FieldRole::Id),
                FieldDef::new("level", FieldType::F32, FieldRole::Plain),
                FieldDef::new("mean", FieldType::F64, FieldRole::Plain),
                FieldDef::new("low", FieldType::OptionF32, FieldRole::Plain),
                FieldDef::new("high", FieldType::OptionF64, FieldRole::Plain),
                FieldDef::new("cached", FieldType::F64, FieldRole::Skip),
                FieldDef::new("count", FieldType::I32, FieldRole::Plain),
            ],
            doc: String::new(),
        }
    }

    fn checks(source: FloatSource<'_>, skipped: Skipped) -> String {
        let mut code = String::new();
        emit_nan_checks(&mut code, &entity(), source, skipped, |m| format!("E({m})"));
        code
    }

    #[test]
    fn a_create_checks_every_stored_float() {
        let code = checks(FloatSource::Record("reading"), Skipped::NotStored);
        for expected in [
            "if reading.level.is_nan() {",
            "if reading.mean.is_nan() {",
            "if reading.low.is_some_and(f32::is_nan) {",
            "if reading.high.is_some_and(f64::is_nan) {",
            "return Err(E(\"Reading.level: NaN cannot be stored\".to_string()));",
        ] {
            assert!(code.contains(expected), "missing `{expected}`:\n{code}");
        }
        assert!(!code.contains("cached") && !code.contains("count"), "{code}");
    }

    /// SeaORM stores a skipped field in its column, so a create checks a
    /// skipped float there too; an update cannot set one.
    #[test]
    fn a_backend_that_stores_skipped_fields_checks_their_floats_on_create() {
        let code = checks(FloatSource::Record("reading"), Skipped::Stored);
        assert!(code.contains("if reading.cached.is_nan() {"), "{code}");
        assert!(code.contains("return Err(E(\"Reading.cached: NaN cannot be stored\".to_string()));"), "{code}");
        assert!(code.contains("if reading.level.is_nan() {") && !code.contains("count"), "{code}");
        for skipped in [Skipped::Stored, Skipped::NotStored] {
            let code = checks(FloatSource::Updates, skipped);
            assert!(!code.contains("cached"), "{code}");
        }
    }

    #[test]
    fn an_update_checks_only_the_floats_it_sets() {
        let code = checks(FloatSource::Updates, Skipped::Stored);
        for expected in [
            "if updates.level.is_some_and(f32::is_nan) {",
            "if updates.mean.is_some_and(f64::is_nan) {",
            "if updates.low.flatten().is_some_and(f32::is_nan) {",
            "if updates.high.flatten().is_some_and(f64::is_nan) {",
        ] {
            assert!(code.contains(expected), "missing `{expected}`:\n{code}");
        }
    }

    #[test]
    fn an_entity_without_floats_gets_nothing() {
        let mut entity = entity();
        entity.fields.retain(|f| {
            !matches!(f.field_type, FieldType::F32 | FieldType::F64 | FieldType::OptionF32 | FieldType::OptionF64)
                || f.role == FieldRole::Skip
        });
        for source in [FloatSource::Updates, FloatSource::Record("reading")] {
            let mut code = String::new();
            emit_nan_checks(&mut code, &entity, source, Skipped::NotStored, |m| m.to_string());
            assert!(code.is_empty(), "{code}");
        }
    }
}
