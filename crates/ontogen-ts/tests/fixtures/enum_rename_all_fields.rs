#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Event {
    ToolCall { prompt_template: String },
    ToolResult { exit_code: u32 },
}
