pub struct Settings {
    pub name: String,
    #[serde(default)]
    pub auto_start: bool,
    #[serde(default)]
    pub notes: Option<String>,
    pub explicit: Option<u32>,
    #[serde(default = "defaults::retries")]
    pub retries: u32,
}
