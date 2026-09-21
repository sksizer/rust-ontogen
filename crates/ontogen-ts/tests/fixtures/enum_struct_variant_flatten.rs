pub enum Event {
    Idle,
    Move {
        #[serde(flatten)]
        origin: Point,
        label: String,
    },
}
