//! End-to-end proof that the GENERATED `UpdateTaskInput` (not a hand
//! reimplementation of the idiom) deserializes the double-Option shape
//! correctly: `Task.parent_id` is a `belongs_to` relation of type
//! `Option<String>`, so its Update DTO field is `Option<Option<String>>`
//! carrying `#[serde(default, deserialize_with = "double_option")]`
//! (see `ontogen::persistence::dto::generate_double_option_fn`).
//!
//! Before that attribute was emitted, serde's default handling of
//! `Option<Option<T>>` collapsed a JSON `null` into the OUTER `None`
//! ("field not provided"), making `parent_id` impossible to clear over
//! JSON — a client sending `{"parent_id": null}` got a silent no-op.

use markdown_pilot::schema::UpdateTaskInput;

#[test]
fn update_task_input_distinguishes_absent_null_and_present_parent_id() {
    let absent: UpdateTaskInput = serde_json::from_str("{}").expect("absent key parses");
    assert_eq!(absent.parent_id, None, "an absent key must leave parent_id untouched");

    let cleared: UpdateTaskInput = serde_json::from_str(r#"{"parent_id": null}"#).expect("null parent_id parses");
    assert_eq!(cleared.parent_id, Some(None), "JSON null must clear parent_id: Some(None)");

    let reparented: UpdateTaskInput =
        serde_json::from_str(r#"{"parent_id": "some-other-task"}"#).expect("string parent_id parses");
    assert_eq!(reparented.parent_id, Some(Some("some-other-task".to_string())), "a provided id must be Some(Some(id))");
}
