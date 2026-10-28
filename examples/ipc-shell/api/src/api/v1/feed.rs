use tokio::sync::broadcast;

use crate::AppState;
use crate::schema::Note;

/// Every change to a note. A parameterless event op: the IPC file gets
/// `note_changed_subscribe`/`_unsubscribe` and `start_event_forwarding`, whose
/// service path and state type must name the API crate from the shell.
pub fn note_changed(state: &AppState) -> broadcast::Receiver<Note> {
    state.notes.subscribe()
}
