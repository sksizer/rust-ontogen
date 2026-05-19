#[serde(rename_all = "snake_case")]
pub enum Color {
    #[serde(rename = "ROUGE")]
    Red,
    Green,
    Blue,
}
