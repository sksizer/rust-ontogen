//! Runtime support for ordered lists (ADR 0006).
//!
//! Generated stores order with these types, and every transport parses its
//! sort keys with [`parse_sort`], so HTTP, Tauri IPC and MCP accept the same
//! keys and reject the same mistakes. Both store backends order by
//! [`effective`], which ends on the id so the order is total and a page is
//! the same on either backend.

use std::cmp::Ordering;
use std::fmt;

/// The direction of one sort key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Asc,
    Desc,
}

impl Direction {
    /// `ord` for `Asc`, `ord.reverse()` for `Desc`. Reversing per key, rather
    /// than the whole comparison, keeps the later keys' directions intact.
    #[must_use]
    pub fn apply(self, ord: Ordering) -> Ordering {
        match self {
            Self::Asc => ord,
            Self::Desc => ord.reverse(),
        }
    }
}

/// One sort key: a field and its direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OrderBy<F> {
    pub field: F,
    pub direction: Direction,
}

impl<F> OrderBy<F> {
    #[must_use]
    pub const fn asc(field: F) -> Self {
        Self { field, direction: Direction::Asc }
    }

    #[must_use]
    pub const fn desc(field: F) -> Self {
        Self { field, direction: Direction::Desc }
    }
}

/// An entity's sortable fields. Generated; never implemented by hand.
pub trait SortField: Copy + Eq + 'static {
    /// Every sortable field, the id first.
    const ALL: &'static [Self];
    /// The `#[ontology(id)]` field, the final tie-break.
    const ID: Self;
    /// The field's sort key: its serialized name, `id` for the id field.
    fn name(self) -> &'static str;
    /// The field whose [`name`](Self::name) is `name`.
    fn from_name(name: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|f| f.name() == name)
    }
}

/// A sort key that names no sortable field, repeats one, or is empty.
/// A field name is carried without its `-`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SortError {
    UnknownField(String),
    DuplicateField(String),
    EmptyKey,
}

impl fmt::Display for SortError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownField(name) => write!(f, "unknown sort field `{name}`"),
            Self::DuplicateField(name) => write!(f, "sort field `{name}` is named twice"),
            Self::EmptyKey => f.write_str("empty sort key"),
        }
    }
}

impl std::error::Error for SortError {}

/// Parses sort keys the caller has already split: `name` is ascending,
/// `-name` descending.
///
/// A field named twice is an error even in opposite directions, since only
/// one of the two could take effect.
///
/// # Errors
///
/// [`SortError::EmptyKey`] for `""` or `"-"`, [`SortError::UnknownField`]
/// for a name `F` does not have, [`SortError::DuplicateField`] for a field
/// already named. The first bad key decides the error.
pub fn parse_sort<F: SortField>(keys: impl IntoIterator<Item = impl AsRef<str>>) -> Result<Vec<OrderBy<F>>, SortError> {
    let mut order: Vec<OrderBy<F>> = Vec::new();
    for key in keys {
        let key = key.as_ref();
        if key.is_empty() || key == "-" {
            return Err(SortError::EmptyKey);
        }
        let (name, direction) = match key.strip_prefix('-') {
            Some(name) => (name, Direction::Desc),
            None => (key, Direction::Asc),
        };
        let field = F::from_name(name).ok_or_else(|| SortError::UnknownField(name.to_string()))?;
        if order.iter().any(|o| o.field == field) {
            return Err(SortError::DuplicateField(name.to_string()));
        }
        order.push(OrderBy { field, direction });
    }
    Ok(order)
}

/// The keys both backends order by: `order` with each field's first
/// occurrence only, then `OrderBy::asc(F::ID)` unless `order` names the id.
/// The id tie-break makes the order total, so a page is well defined.
#[must_use]
pub fn effective<F: SortField>(order: &[OrderBy<F>]) -> Vec<OrderBy<F>> {
    let mut keys: Vec<OrderBy<F>> = Vec::with_capacity(order.len() + 1);
    for key in order {
        if !keys.iter().any(|k| k.field == key.field) {
            keys.push(*key);
        }
    }
    if !keys.iter().any(|k| k.field == F::ID) {
        keys.push(OrderBy::asc(F::ID));
    }
    keys
}

/// Float order shared by every generated comparator: -0.0 compares equal
/// to 0.0, then `f64::total_cmp`. (f32 fields widen with `f64::from`, which
/// is exact and order-preserving.)
///
/// `total_cmp` alone would put -0.0 before 0.0, which SQL does not; equal
/// zeros fall through to the next key on both backends. A NaN still has a
/// fixed place, though the stores refuse to write one.
#[must_use]
pub fn cmp_f64(a: f64, b: f64) -> Ordering {
    if a == 0.0 && b == 0.0 { Ordering::Equal } else { a.total_cmp(&b) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Field {
        Id,
        Title,
        Status,
    }

    impl SortField for Field {
        const ALL: &'static [Self] = &[Self::Id, Self::Title, Self::Status];
        const ID: Self = Self::Id;
        fn name(self) -> &'static str {
            match self {
                Self::Id => "id",
                Self::Title => "title",
                Self::Status => "status",
            }
        }
    }

    fn parse(keys: &[&str]) -> Result<Vec<OrderBy<Field>>, SortError> {
        parse_sort(keys.iter().copied())
    }

    #[test]
    fn from_name_finds_every_field_by_name() {
        for field in Field::ALL {
            assert_eq!(Field::from_name(field.name()), Some(*field));
        }
        assert_eq!(Field::from_name("Title"), None);
        assert_eq!(Field::from_name("-title"), None);
    }

    #[test]
    fn a_bare_key_is_ascending_and_a_dashed_key_descending() {
        assert_eq!(
            parse(&["title", "-status", "id"]),
            Ok(vec![OrderBy::asc(Field::Title), OrderBy::desc(Field::Status), OrderBy::asc(Field::Id)])
        );
    }

    #[test]
    fn no_keys_is_the_empty_order() {
        assert_eq!(parse(&[]), Ok(vec![]));
    }

    #[test]
    fn parse_takes_owned_strings() {
        let keys = vec![String::from("-title")];
        assert_eq!(parse_sort::<Field>(keys), Ok(vec![OrderBy::desc(Field::Title)]));
    }

    #[test]
    fn an_unknown_field_is_named_without_its_dash() {
        assert_eq!(parse(&["priority"]), Err(SortError::UnknownField("priority".into())));
        assert_eq!(parse(&["title", "-priority"]), Err(SortError::UnknownField("priority".into())));
    }

    #[test]
    fn only_one_dash_is_stripped() {
        assert_eq!(parse(&["--title"]), Err(SortError::UnknownField("-title".into())));
    }

    #[test]
    fn a_field_named_twice_is_refused_in_any_direction() {
        for keys in [["title", "title"], ["title", "-title"], ["-title", "title"], ["-title", "-title"]] {
            assert_eq!(parse(&keys), Err(SortError::DuplicateField("title".into())), "{keys:?}");
        }
        assert_eq!(parse(&["id", "status", "-id"]), Err(SortError::DuplicateField("id".into())));
    }

    #[test]
    fn an_empty_key_or_a_lone_dash_is_an_empty_key() {
        assert_eq!(parse(&[""]), Err(SortError::EmptyKey));
        assert_eq!(parse(&["-"]), Err(SortError::EmptyKey));
        assert_eq!(parse(&["title", ""]), Err(SortError::EmptyKey));
    }

    #[test]
    fn the_first_bad_key_decides_the_error() {
        assert_eq!(parse(&["nope", ""]), Err(SortError::UnknownField("nope".into())));
        assert_eq!(parse(&["", "nope"]), Err(SortError::EmptyKey));
    }

    #[test]
    fn errors_display_their_field() {
        assert_eq!(SortError::UnknownField("x".into()).to_string(), "unknown sort field `x`");
        assert_eq!(SortError::DuplicateField("x".into()).to_string(), "sort field `x` is named twice");
        assert_eq!(SortError::EmptyKey.to_string(), "empty sort key");
        let _: &dyn std::error::Error = &SortError::EmptyKey;
    }

    #[test]
    fn effective_appends_the_id_ascending() {
        assert_eq!(effective::<Field>(&[]), vec![OrderBy::asc(Field::Id)]);
        assert_eq!(
            effective(&[OrderBy::desc(Field::Title)]),
            vec![OrderBy::desc(Field::Title), OrderBy::asc(Field::Id)]
        );
    }

    #[test]
    fn effective_keeps_the_requested_id_direction_and_position() {
        assert_eq!(
            effective(&[OrderBy::desc(Field::Id), OrderBy::asc(Field::Title)]),
            vec![OrderBy::desc(Field::Id), OrderBy::asc(Field::Title)]
        );
    }

    #[test]
    fn effective_keeps_only_a_fields_first_occurrence() {
        assert_eq!(
            effective(&[
                OrderBy::asc(Field::Status),
                OrderBy::desc(Field::Status),
                OrderBy::desc(Field::Title),
                OrderBy::asc(Field::Title),
            ]),
            vec![OrderBy::asc(Field::Status), OrderBy::desc(Field::Title), OrderBy::asc(Field::Id)]
        );
    }

    #[test]
    fn apply_reverses_only_descending() {
        for ord in [Ordering::Less, Ordering::Equal, Ordering::Greater] {
            assert_eq!(Direction::Asc.apply(ord), ord);
            assert_eq!(Direction::Desc.apply(ord), ord.reverse());
        }
    }

    #[test]
    fn negative_zero_equals_zero() {
        assert_eq!(cmp_f64(-0.0, 0.0), Ordering::Equal);
        assert_eq!(cmp_f64(0.0, -0.0), Ordering::Equal);
        assert_eq!(cmp_f64(-0.0, -0.0), Ordering::Equal);
        assert_eq!(cmp_f64(-0.0, f64::MIN_POSITIVE), Ordering::Less);
        assert_eq!(cmp_f64(-f64::MIN_POSITIVE, 0.0), Ordering::Less);
    }

    #[test]
    fn floats_order_numerically() {
        assert_eq!(cmp_f64(1.5, 2.0), Ordering::Less);
        assert_eq!(cmp_f64(2.0, 1.5), Ordering::Greater);
        assert_eq!(cmp_f64(f64::NEG_INFINITY, f64::MIN), Ordering::Less);
        assert_eq!(cmp_f64(f64::MAX, f64::INFINITY), Ordering::Less);
        assert_eq!(cmp_f64(f64::from(0.1_f32), f64::from(0.2_f32)), Ordering::Less);
    }

    #[test]
    fn nan_has_a_fixed_place() {
        let nan = f64::NAN;
        let neg_nan = -f64::NAN;
        assert_eq!(cmp_f64(nan, nan), Ordering::Equal);
        assert_eq!(cmp_f64(nan, f64::INFINITY), Ordering::Greater);
        assert_eq!(cmp_f64(f64::INFINITY, nan), Ordering::Less);
        assert_eq!(cmp_f64(neg_nan, f64::NEG_INFINITY), Ordering::Less);
        assert_eq!(cmp_f64(nan, 0.0), Ordering::Greater);

        let mut values = [1.0, nan, -0.0, neg_nan, f64::NEG_INFINITY, 0.0, f64::INFINITY];
        values.sort_by(|a, b| cmp_f64(*a, *b));
        assert!(values[0].is_nan() && values[0].is_sign_negative());
        assert_eq!(values[1], f64::NEG_INFINITY);
        assert_eq!(&values[2..4], &[0.0, 0.0]);
        assert_eq!(values[4], 1.0);
        assert_eq!(values[5], f64::INFINITY);
        assert!(values[6].is_nan() && values[6].is_sign_positive());
    }
}
