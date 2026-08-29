-- Add mcp_enabled column to kb_knowledge_bases
-- Controls whether a knowledge base is exposed via MCP server
ALTER TABLE kb_knowledge_bases ADD COLUMN mcp_enabled INTEGER NOT NULL DEFAULT 1;
