//! A note's tags as a junction relationship whose list answers ids: the
//! memberships live in `AppState`, not in the note, so the note entity has
//! no tag field and the relationship has no linkage of its own.

pub use super::generated::note::*;

use crate::AppState;
use crate::schema::AppError;

/// The ids of the tags on a note, in the order they were added. A tag
/// deleted since is still listed: nothing here watches the store.
pub async fn list_tags(state: &AppState, id: &str) -> Result<Vec<String>, AppError> {
    Ok(state.note_tags.lock().expect("note tags lock").get(id).cloned().unwrap_or_default())
}

// It does not look for the tag first: the HTTP relationship endpoint reads
// `list_tags` before calling it, so a repeated POST adds the tag once
// whatever this does, and the router tests show it.
/// Tag a note. Tagging it twice lists the tag twice.
pub async fn add_tag(state: &AppState, id: &str, tag_id: &str) -> Result<(), AppError> {
    state.note_tags.lock().expect("note tags lock").entry(id.to_string()).or_default().push(tag_id.to_string());
    Ok(())
}

/// Take a tag off a note.
pub async fn remove_tag(state: &AppState, id: &str, tag_id: &str) -> Result<(), AppError> {
    if let Some(tags) = state.note_tags.lock().expect("note tags lock").get_mut(id) {
        tags.retain(|t| t != tag_id);
    }
    Ok(())
}
