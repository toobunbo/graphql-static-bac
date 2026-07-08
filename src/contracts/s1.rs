use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SinksData {
    pub selected_types: Vec<SelectedType>,
    pub sink_refs: BTreeMap<String, SinkRef>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SelectedType {
    pub type_id: String,
    pub type_name: String,
    pub sink_ref_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SinkRef {
    pub sink_ref_id: String,
    pub type_id: String,
    pub field_id: Option<String>,
}
