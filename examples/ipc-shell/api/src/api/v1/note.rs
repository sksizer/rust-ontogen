use crate::schema::{CreateNoteInput, Note, UpdateNoteInput};
use crate::store::Store;

pub async fn list(_store: &Store) -> Result<Vec<Note>, String> {
    Ok(Vec::new())
}

pub async fn get_by_id(_store: &Store, id: &str) -> Result<Note, String> {
    Ok(Note { id: id.to_string(), title: String::new() })
}

pub async fn create(_store: &Store, input: CreateNoteInput) -> Result<Note, String> {
    Ok(Note { id: "1".to_string(), title: input.title })
}

pub async fn update(_store: &Store, id: &str, input: UpdateNoteInput) -> Result<Note, String> {
    Ok(Note { id: id.to_string(), title: input.title.unwrap_or_default() })
}

pub async fn delete(_store: &Store, _id: &str) -> Result<(), String> {
    Ok(())
}
