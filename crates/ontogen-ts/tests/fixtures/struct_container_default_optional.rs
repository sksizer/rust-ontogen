#[serde(default)]
pub struct Settings {
    pub name: String,
    pub retries: u32,
    pub notes: Option<String>,
}
