pub struct Envelope {
    pub id: String,
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}
