#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ErrorInformation {
    pub error: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}
