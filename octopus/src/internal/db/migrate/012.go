package migrate

import (
	"encoding/json"
	"fmt"
	"strings"

	"gorm.io/gorm"
)

func init() {
	RegisterBeforeAutoMigration(Migration{
		Version: 12,
		Up:      migrateAPIKeySupportedModels,
	})
}

// migrateAPIKeySupportedModels 把 api_keys.supported_models 由逗号分隔字符串改写为 JSON 数组。
// 列类型两侧都是文本, AutoMigrate 不会动它, 故只需原地改写取值:
// 改写后由 GORM 的 json 序列化器读写, 前后端都不必再拆分隔符。
// 已是 JSON 数组的行跳过, 使中途失败可重跑; 空值改写为空数组以对齐"不限制"的语义。
func migrateAPIKeySupportedModels(db *gorm.DB) error {
	if db == nil {
		return fmt.Errorf("db is nil")
	}
	if !db.Migrator().HasTable("api_keys") || !hasPhysicalColumn(db, "api_keys", "supported_models") {
		return nil
	}

	type apiKeyRow struct {
		ID              int    // API Key 主键。
		SupportedModels string // 迁移前的逗号分隔模型名称。
	}
	rows := make([]apiKeyRow, 0)
	if err := db.Table("api_keys").Select("id, supported_models").Find(&rows).Error; err != nil {
		return fmt.Errorf("failed to read api_keys: %w", err)
	}

	for _, row := range rows {
		value := strings.TrimSpace(row.SupportedModels)
		if strings.HasPrefix(value, "[") {
			continue
		}
		names := make([]string, 0)
		for _, name := range strings.Split(value, ",") {
			if name = strings.TrimSpace(name); name != "" {
				names = append(names, name)
			}
		}
		encoded, err := json.Marshal(names)
		if err != nil {
			return fmt.Errorf("failed to encode api_key %d supported_models: %w", row.ID, err)
		}
		if err := db.Table("api_keys").Where("id = ?", row.ID).
			Update("supported_models", string(encoded)).Error; err != nil {
			return fmt.Errorf("failed to update api_key %d supported_models: %w", row.ID, err)
		}
	}
	return nil
}
