//! The shapes the hand-written training ops take and return. Nothing here is an
//! entity: each is a read assembled from several, or a write that spans them.

use serde::{Deserialize, Serialize};

use crate::schema::{Intent, Note, Session};

/// One set as a person logs it. It becomes an effort plus its two facets.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogSetInput {
    /// Shared by the effort and both facets.
    pub id: String,
    pub person_id: String,
    pub session_id: Option<String>,
    pub exercise_id: String,
    pub kind_id: String,
    pub started_at: String,
    pub position: Option<i32>,
    pub intent: Intent,
    pub load_grams: Option<i32>,
    pub reps: i32,
    pub rpe: Option<f64>,
}

/// One set, joined across its effort, its facets and its exercise.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SetLine {
    pub effort_id: String,
    pub exercise_id: String,
    pub exercise_name: String,
    pub position: Option<i32>,
    pub started_at: String,
    pub intent: Intent,
    pub load_grams: Option<i32>,
    pub reps: i32,
    pub rpe: Option<f64>,
    /// Epley's estimate, for a set with a mass on it.
    pub e1rm_grams: Option<i32>,
}

/// A session with what it holds.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionDetail {
    pub session: Session,
    pub location_name: Option<String>,
    pub tag_names: Vec<String>,
    pub sets: Vec<SetLine>,
    pub notes: Vec<Note>,
}

/// Work on one muscle: load × reps × that muscle's share, summed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MuscleVolume {
    pub muscle_id: String,
    pub muscle_name: String,
    pub volume_kg: f64,
    pub sets: i32,
}

/// A person's strongest set of one exercise, by estimated max.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BestSet {
    pub exercise_id: String,
    pub exercise_name: String,
    pub load_grams: i32,
    pub reps: i32,
    pub e1rm_grams: i32,
    pub achieved_at: String,
}
