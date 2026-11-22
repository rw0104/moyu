use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DialogueInput {
    pub text: String,
    pub session_id: String,
    pub peer_language: String,
    pub direction: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DialogueRoute {
    pub source_language: String,
    pub target_language: String,
    pub peer_language: String,
    pub direction: String,
    pub used_fallback: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureStatus {
    pub shortcut: String,
    pub active: bool,
    pub capturing: bool,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub model: String,
    pub style: String,
    pub use_memory: bool,
    pub use_context: bool,
    pub save_history: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            model: "deepseek-flash".into(),
            style: "natural".into(),
            use_memory: true,
            use_context: true,
            save_history: true,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TranslationRequest {
    pub text: String,
    pub source_language: String,
    pub target_language: String,
    pub style: String,
    pub session_id: String,
    #[serde(default)]
    pub batch_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_image: Option<SourceImage>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TranslationPlan {
    pub segments: Vec<String>,
    pub estimated_input_tokens: usize,
    pub output_token_budget: usize,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TranslationResult {
    pub text: String,
    pub source_language: String,
    pub target_language: String,
    pub style: String,
    pub session_id: String,
    pub id: String,
    pub translation: String,
    pub detected_language: String,
    pub origin: String,
    pub verified: bool,
    pub created_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_image: Option<SourceImage>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SourceImage {
    pub id: String,
    pub name: String,
    pub mime_type: String,
    pub thumbnail_data_url: String,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ImageInput {
    pub data_url: String,
    pub name: String,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageExtraction {
    pub text: String,
    pub source_image: SourceImage,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationSummary {
    pub id: String,
    pub title: String,
    pub preview: String,
    pub turn_count: usize,
    pub created_at: String,
    pub updated_at: String,
    pub mode: String,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationPage {
    pub conversations: Vec<ConversationSummary>,
    pub has_more: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MemoryInput {
    pub source_text: String,
    pub translation: String,
    pub source_language: String,
    pub target_language: String,
    pub style: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MemoryEntry {
    pub source_text: String,
    pub translation: String,
    pub source_language: String,
    pub target_language: String,
    pub style: String,
    pub id: String,
    pub created_at: String,
    pub model: String,
    pub glossary_revision: i64,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TermInput {
    pub source: String,
    pub target: String,
    pub source_language: String,
    pub target_language: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Term {
    pub source: String,
    pub target: String,
    pub source_language: String,
    pub target_language: String,
    pub id: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Bootstrap {
    pub workspace_mode: String,
    pub dialogue_preferences: DialoguePreferences,
    pub settings: Settings,
    pub has_api_key: bool,
    pub history: Vec<TranslationResult>,
    pub session_turns: Vec<TranslationResult>,
    pub conversations: Vec<ConversationSummary>,
    pub has_more_conversations: bool,
    pub memories: Vec<MemoryEntry>,
    pub terms: Vec<Term>,
    pub data_directory: String,
    pub session_id: String,
    pub context_id: String,
    pub context_limits: ContextLimits,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", default)]
pub struct DialoguePreferences {
    pub peer_language: String,
    pub last_peer_language: String,
    pub last_direction: String,
}
impl Default for DialoguePreferences {
    fn default() -> Self {
        Self {
            peer_language: "auto".into(),
            last_peer_language: "en".into(),
            last_direction: "inbound".into(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextLimits {
    pub max_turns: usize,
    pub max_characters: usize,
    pub transcript_limit: usize,
}
