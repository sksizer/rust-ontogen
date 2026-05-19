#[serde(rename_all = "camelCase")]
pub struct UserProfile {
    pub display_name: String,
    pub age_years: u32,
    pub last_login_ts: i64,
}
