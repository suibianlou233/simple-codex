use serde::Deserialize;

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MemoryResetMode {
    #[default]
    GeneratedOnly,
    ExcludePreviousSources,
}
