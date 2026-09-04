package migrate

import (
	"fmt"

	"gorm.io/gorm"
)

func init() {
	RegisterBeforeAutoMigration(Migration{
		Version: 4,
		Up:      migrateChannelTypeToProvider,
	})
}

// migrateChannelTypeToProvider 将历史 API 格式渠道类型迁移为渠道提供方。
func migrateChannelTypeToProvider(db *gorm.DB) error {
	if db == nil {
		return fmt.Errorf("db is nil")
	}
	if !db.Migrator().HasTable("channels") || !db.Migrator().HasColumn("channels", "type") {
		return nil
	}

	typeColumn := `"type"`
	typeExpr := `CAST("type" AS TEXT)`
	switch db.Dialector.Name() {
	case "mysql":
		typeColumn = "`type`"
		typeExpr = "CAST(`type` AS CHAR)"
	case "postgres":
		typeExpr = `"type"::text`
	}

	if err := db.Exec(fmt.Sprintf(`
UPDATE channels
SET %s = CASE %s
	WHEN 'openai/chat_completions' THEN 'openai'
	WHEN 'openai/responses' THEN 'openai_responses'
	WHEN 'anthropic/messages' THEN 'anthropic'
	WHEN 'gemini/contents' THEN 'gemini'
	WHEN 'doubao' THEN 'volcengine'
	WHEN 'openai/embeddings' THEN 'openai'
	ELSE %s
END
`, typeColumn, typeExpr, typeColumn)).Error; err != nil {
		return fmt.Errorf("failed to migrate channels.type to provider: %w", err)
	}
	return nil
}
