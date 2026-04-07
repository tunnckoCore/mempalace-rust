#[derive(Debug, Clone)]
pub struct DrawerInputOwned {
    pub id: String,
    pub wing: String,
    pub room: String,
    pub source_file: String,
    pub chunk_index: i64,
    pub added_by: String,
    pub content: String,
    pub hall: Option<String>,
    pub date: Option<String>,
    pub drawer_type: String,
    pub source_hash: Option<String>,
    pub importance: Option<f64>,
    pub emotional_weight: Option<f64>,
    pub weight: Option<f64>,
}

#[derive(Debug, Clone)]
pub struct SourceRefreshPlanOwned {
    pub source_file: String,
    pub source_hash: String,
    pub drawers: Vec<DrawerInputOwned>,
}
