pub mod handlers;

use super::{Service, ServiceStatus};
use crate::server::router::SharedState;
use crate::AppState;
use async_trait::async_trait;
use axum::Router;
use std::sync::Arc;

pub struct McpService;

#[async_trait]
impl Service for McpService {
    fn id(&self) -> &'static str {
        "mcp"
    }
    fn name(&self) -> &'static str {
        "MCP Server"
    }
    fn description(&self) -> &'static str {
        "Model Context Protocol Server，对外暴露 RAG 工具（支持创建/更新/删除 RAG、上传/删除文档、导入源、构建索引、搜索、RAG问答）"
    }

    async fn status(&self, state: &Arc<AppState>) -> ServiceStatus {
        let pool = &state.db.pool;
        let kb_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM kb_knowledge_bases WHERE status = 1")
                .fetch_one(pool)
                .await
                .unwrap_or(0);
        let tools = handlers::get_tools()
            .into_iter()
            .map(|tool| {
                let name = tool
                    .get("name")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("unknown");
                let description = tool
                    .get("description")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("");
                serde_json::json!({
                    "name": name,
                    "label": name,
                    "desc": description,
                })
            })
            .collect::<Vec<_>>();

        ServiceStatus {
            id: self.id().to_string(),
            name: self.name().to_string(),
            description: self.description().to_string(),
            enabled: true,
            running: true,
            stats: serde_json::json!({
                "available_knowledge_bases": kb_count,
                "tools": tools,
            }),
        }
    }

    fn routes(&self, _state: Arc<AppState>) -> Router<SharedState> {
        Router::new()
            // Primary Streamable HTTP endpoint (POST = JSON-RPC, GET = SSE upgrade)
            .route(
                "/mcp",
                axum::routing::post(handlers::handle_mcp).get(handlers::handle_mcp_sse),
            )
            // Trailing-slash variant — some clients send /mcp/
            .route(
                "/mcp/",
                axum::routing::post(handlers::handle_mcp).get(handlers::handle_mcp_sse),
            )
            // Legacy SSE endpoint — keep for backwards compat
            .route(
                "/mcp/sse",
                axum::routing::get(handlers::handle_mcp_sse).post(handlers::handle_mcp),
            )
    }
}
