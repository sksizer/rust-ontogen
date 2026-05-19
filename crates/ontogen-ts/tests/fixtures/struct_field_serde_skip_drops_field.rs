pub struct Hidden {
    pub visible: u32,
    #[serde(skip)]
    pub hidden: u32,
}
