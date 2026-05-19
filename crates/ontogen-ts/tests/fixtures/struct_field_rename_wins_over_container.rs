#[serde(rename_all = "camelCase")]
pub struct MixedRenames {
    #[serde(rename = "_internal_id")]
    pub user_id: u64,
    pub display_name: String,
}
