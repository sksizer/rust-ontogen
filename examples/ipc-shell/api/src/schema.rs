use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize)]
pub struct Note {
    pub id: String,
    pub title: String,
}

#[derive(Deserialize)]
pub struct CreateNoteInput {
    pub title: String,
}

#[derive(Deserialize)]
pub struct UpdateNoteInput {
    pub title: Option<String>,
}
