#[serde(rename_all = "kebab-case")]
pub struct KebabUser {
    pub display_name: String,
    pub age_years: u32,
}
