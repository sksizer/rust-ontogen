#[serde(rename_all = "camelCase")]
pub enum Event {
    ToolCall { prompt_template: String },
}
