use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct WikiProject {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub status: i64,
    pub schema_text: Option<String>,
    pub wiki_dir: String,
    pub ingest_model: Option<String>,
    pub chat_model: Option<String>,
    pub ingest_channel_id: Option<String>,
    pub chat_channel_id: Option<String>,
    pub mcp_enabled: i64,
    pub source_count: i64,
    pub page_count: i64,
    pub last_ingest_at: Option<String>,
    pub last_lint_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateProjectInput {
    pub name: String,
    pub description: Option<String>,
    pub ingest_model: Option<String>,
    pub chat_model: Option<String>,
    pub ingest_channel_id: Option<String>,
    pub chat_channel_id: Option<String>,
    pub schema_text: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateProjectInput {
    pub name: Option<String>,
    pub description: Option<String>,
    pub status: Option<i64>,
    pub schema_text: Option<String>,
    pub ingest_model: Option<String>,
    pub chat_model: Option<String>,
    pub ingest_channel_id: Option<String>,
    pub chat_channel_id: Option<String>,
    pub mcp_enabled: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct WikiPage {
    pub id: String,
    pub project_id: String,
    pub path: String,
    pub title: String,
    pub page_type: String,
    pub content_hash: String,
    pub token_count: i64,
    pub wikilinks: String,
    pub frontmatter: String,
    pub tags: String,
    pub status: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WikiPageWithContent {
    #[serde(flatten)]
    pub page: WikiPage,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct WikiSource {
    pub id: String,
    pub project_id: String,
    pub source_type: String,
    pub filename: String,
    pub file_path: Option<String>,
    pub source_url: Option<String>,
    pub content_hash: Option<String>,
    pub file_size: i64,
    pub status: String,
    pub page_count: i64,
    pub error_message: Option<String>,
    pub created_at: String,
    pub ingested_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddSourceInput {
    pub source_type: String,
    pub filename: String,
    pub file_path: Option<String>,
    pub source_url: Option<String>,
    pub content: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct WikiIngestTask {
    pub id: String,
    pub project_id: String,
    pub source_id: Option<String>,
    pub task_type: String,
    pub status: String,
    pub progress: i64,
    pub total_steps: i64,
    pub done_steps: i64,
    pub result_json: Option<String>,
    pub error_message: Option<String>,
    pub created_at: String,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct WikiSession {
    pub id: String,
    pub project_id: String,
    pub role: String,
    pub content: String,
    pub sources_json: Option<String>,
    pub model: Option<String>,
    pub tokens_used: i64,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WikiSearchResult {
    pub page_id: String,
    pub path: String,
    pub title: String,
    pub score: f64,
    pub snippet: String,
    pub page_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WikiAskInput {
    pub question: String,
    pub top_k: Option<usize>,
    pub model: Option<String>,
    pub history: Option<Vec<ConversationMessage>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WikiAskResult {
    pub answer: String,
    pub sources: Vec<WikiAnswerSource>,
    pub usage: Option<UsageInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WikiAnswerSource {
    pub path: String,
    pub title: String,
    pub score: f64,
    pub snippet: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageInfo {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphNode {
    pub id: String,
    pub label: String,
    pub path: Option<String>,
    pub node_type: String,
    pub link_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphEdge {
    pub source: String,
    pub target: String,
    pub edge_type: String,
    pub weight: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphData {
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WikiTag {
    pub word: String,
    pub count: usize,
}
