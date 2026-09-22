pub struct Step {
    #[serde(flatten)]
    pub meta: StepMeta,
    pub program: String,
}
