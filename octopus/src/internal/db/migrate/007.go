package migrate

import (
	"encoding/json"
	"fmt"

	"github.com/bestruirui/octopus/internal/model"
	"gorm.io/gorm"
)

func init() {
	RegisterAfterAutoMigration(Migration{
		Version: 7,
		Up:      migrateGroupRouting,
	})
}

// migrateGroupRouting 将上一版分组迁移为根级模式和 Relay JSON 配置，并删除旧重试间隔列。
func migrateGroupRouting(db *gorm.DB) error {
	if db == nil {
		return fmt.Errorf("db is nil")
	}
	if !db.Migrator().HasTable("groups") {
		return nil
	}
	if !db.Migrator().HasColumn("groups", "relay_config") {
		return fmt.Errorf("groups.relay_config not found")
	}
	if !db.Migrator().HasColumn("groups", "mode") {
		return fmt.Errorf("groups.mode not found")
	}
	if !db.Migrator().HasColumn("groups", "retry_interval") {
		return nil
	}

	type legacyGroup struct {
		ID            int // 分组主键。
		RetryInterval int // 旧分组重试间隔秒数。
	}
	groups := make([]legacyGroup, 0)
	if err := db.Table("groups").Select("id, retry_interval").Find(&groups).Error; err != nil {
		return fmt.Errorf("failed to read groups.retry_interval: %w", err)
	}
	for _, group := range groups {
		config := model.DefaultGroupRelayConfig()
		if group.RetryInterval >= 1 {
			config.MemberRetryIntervalSeconds = group.RetryInterval
		}
		payload, err := json.Marshal(config)
		if err != nil {
			return fmt.Errorf("failed to encode relay config for group %d: %w", group.ID, err)
		}
		if err := db.Table("groups").Where("id = ?", group.ID).Updates(map[string]interface{}{
			"mode":         model.GroupModeManual,
			"relay_config": string(payload),
		}).Error; err != nil {
			return fmt.Errorf("failed to migrate routing config for group %d: %w", group.ID, err)
		}
	}

	return dropColumnIfExists(db, &model.Group{}, "groups", "retry_interval")
}
