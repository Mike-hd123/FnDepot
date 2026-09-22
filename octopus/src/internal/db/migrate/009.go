package migrate

import (
	"fmt"

	"github.com/bestruirui/octopus/internal/model"
	"gorm.io/gorm"
)

func init() {
	RegisterAfterAutoMigration(Migration{
		Version: 9,
		Up:      migrateDropLegacyChannelSchema,
	})
}

// migrateDropLegacyChannelSchema 删除遗留的渠道多地址字段和转发日志表。
func migrateDropLegacyChannelSchema(db *gorm.DB) error {
	if db == nil {
		return fmt.Errorf("db is nil")
	}
	if db.Migrator().HasTable("channels") {
		if err := dropColumnIfExists(db, &model.Channel{}, "channels", "base_urls"); err != nil {
			return err
		}
	}
	if db.Migrator().HasTable("relay_logs") {
		if err := db.Migrator().DropTable("relay_logs"); err != nil {
			return fmt.Errorf("failed to drop relay_logs: %w", err)
		}
	}
	return nil
}
