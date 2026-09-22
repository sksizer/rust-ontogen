pub struct Merged {
    #[serde(flatten)]
    pub base: Base,
    #[serde(flatten)]
    pub audit: AuditFields,
}
