#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Event {
    #[serde(rename_all = "SCREAMING_SNAKE_CASE")]
    ToolCall {
        prompt_template: String,
    },
    ToolResult {
        exit_code: u32,
    },
}
