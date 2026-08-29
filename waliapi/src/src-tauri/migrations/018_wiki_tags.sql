-- Add tags column to wiki_pages for tag storage (JSON array string)
ALTER TABLE wiki_pages ADD COLUMN tags TEXT NOT NULL DEFAULT '[]';

-- Index for efficient tag queries
CREATE INDEX IF NOT EXISTS idx_wiki_pages_tags ON wiki_pages(project_id, status);
