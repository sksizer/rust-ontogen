use serde::Serialize;

/// One change to the workout log, as pushed to event subscribers.
#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct Activity {
    /// Monotonic position in the feed; a client resumes after it.
    pub seq: u64,
    pub kind: String,
    pub id: String,
}

impl ontogen_core::events::EventSeq for Activity {
    fn event_id(&self) -> String {
        self.seq.to_string()
    }
}
