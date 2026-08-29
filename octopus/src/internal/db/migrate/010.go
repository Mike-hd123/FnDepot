package migrate

import (
	"database/sql"
	"fmt"
	"strings"

	"github.com/bestruirui/octopus/internal/model"
	"gorm.io/gorm"
	"gorm.io/gorm/clause"
)

func init() {
	RegisterBeforeAutoMigration(Migration{
		Version: 10,
		Up:      migrateRepairGroupItemChannelModel,
	})
}

// migrateRepairGroupItemChannelModel 修补版本 8 未执行完成的数据库。
// group_items 仍保留 channel_id 和 model_name 时，补齐渠道模型记录，
// 就地回填 channel_model_id 并删除旧列，保留原有分组项主键和顺序。
func migrateRepairGroupItemChannelModel(db *gorm.DB) error {
	if db == nil {
		return fmt.Errorf("db is nil")
	}
	if !db.Migrator().HasTable("group_items") ||
		!hasPhysicalColumn(db, "group_items", "channel_id") ||
		!hasPhysicalColumn(db, "group_items", "model_name") {
		return nil
	}
	if !db.Migrator().HasTable("channels") {
		return fmt.Errorf("channels table not found")
	}

	return db.Transaction(func(tx *gorm.DB) error {
		if !tx.Migrator().HasTable("channel_models") {
			if err := tx.AutoMigrate(&model.ChannelModel{}); err != nil {
				return fmt.Errorf("failed to create channel_models: %w", err)
			}
		}

		type legacyGroupItem struct {
			ID        int    // 分组项主键。
			ChannelID int    // 旧渠道主键。
			ModelName string // 旧模型名称。
			GroupID   int    // 所属分组主键。
		}
		legacyItems := make([]legacyGroupItem, 0)
		if err := tx.Table("group_items").
			Select("id, channel_id, model_name, group_id").
			Order("id ASC").
			Find(&legacyItems).Error; err != nil {
			return fmt.Errorf("failed to read legacy group_items: %w", err)
		}

		type legacyChannel struct {
			ID          int            // 渠道主键。
			AutoSync    bool           // 是否自动同步模型。
			Models      sql.NullString `gorm:"column:model"`        // 旧渠道模型列表，列已删除时不读取。
			CustomModel sql.NullString `gorm:"column:custom_model"` // 旧手动模型列表，列已删除时不读取。
		}
		channelFields := []string{"id"}
		if hasPhysicalColumn(tx, "channels", "auto_sync") {
			channelFields = append(channelFields, "auto_sync")
		}
		if hasPhysicalColumn(tx, "channels", "model") {
			channelFields = append(channelFields, "model")
		}
		if hasPhysicalColumn(tx, "channels", "custom_model") {
			channelFields = append(channelFields, "custom_model")
		}
		channels := make([]legacyChannel, 0)
		if err := tx.Table("channels").Select(channelFields).Order("id ASC").Find(&channels).Error; err != nil {
			return fmt.Errorf("failed to read legacy channels: %w", err)
		}
		channelAutoSync := make(map[int]bool, len(channels))
		for _, channel := range channels {
			channelAutoSync[channel.ID] = channel.AutoSync
		}

		// 渠道模型优先按渠道和名称精确定位。TiDB、PostgreSQL 和 SQLite 默认区分大小写，
		// 仅大小写不同的模型是两条合法记录，只按小写名匹配会把它们合并并误删分组项；
		// MySQL 默认排序规则不区分大小写，库里可能只留下大小写不同的一行，精确匹配不到时再按小写名兜底。
		type channelModelIndexKey struct {
			channelID int    // 所属渠道主键。
			name      string // 模型名称，精确索引用原名，兜底索引用小写名。
		}
		channelModels := make([]model.ChannelModel, 0)
		if err := tx.Order("id ASC").Find(&channelModels).Error; err != nil {
			return fmt.Errorf("failed to read channel_models: %w", err)
		}
		idByName := make(map[channelModelIndexKey]int, len(channelModels))
		idByLowerName := make(map[channelModelIndexKey]int, len(channelModels))
		indexChannelModels := func() {
			for _, channelModel := range channelModels {
				name := strings.TrimSpace(channelModel.Name)
				exact := channelModelIndexKey{channelID: channelModel.ChannelID, name: name}
				if _, ok := idByName[exact]; !ok {
					idByName[exact] = channelModel.ID
				}
				lower := channelModelIndexKey{channelID: channelModel.ChannelID, name: strings.ToLower(name)}
				if _, ok := idByLowerName[lower]; !ok {
					idByLowerName[lower] = channelModel.ID
				}
			}
		}
		indexChannelModels()
		findModelID := func(channelID int, name string) (int, bool) {
			name = strings.TrimSpace(name)
			if id, ok := idByName[channelModelIndexKey{channelID: channelID, name: name}]; ok {
				return id, true
			}
			id, ok := idByLowerName[channelModelIndexKey{channelID: channelID, name: strings.ToLower(name)}]
			return id, ok
		}

		// 旧模型列表和旧分组项引用的模型统一补齐，同名只保留一行且手动来源优先。
		// 渠道已删除时旧分组项是孤立行，插入 channel_models 会违反外键，直接跳过。
		missing := make([]model.ChannelModel, 0)
		missingIndexByKey := make(map[channelModelIndexKey]int)
		addModel := func(channelID int, name string, source model.ChannelModelSource) {
			name = strings.TrimSpace(name)
			if name == "" {
				return
			}
			if _, ok := channelAutoSync[channelID]; !ok {
				return
			}
			if _, ok := findModelID(channelID, name); ok {
				return
			}
			key := channelModelIndexKey{channelID: channelID, name: name}
			if index, ok := missingIndexByKey[key]; ok {
				if source == model.ChannelModelSourceManual {
					missing[index].Source = model.ChannelModelSourceManual
				}
				return
			}
			missingIndexByKey[key] = len(missing)
			missing = append(missing, model.ChannelModel{ChannelID: channelID, Name: name, Source: source})
		}
		for _, channel := range channels {
			if channel.Models.Valid {
				for _, name := range strings.Split(channel.Models.String, ",") {
					addModel(channel.ID, name, model.ChannelModelSourceAuto)
				}
			}
			if channel.CustomModel.Valid {
				for _, name := range strings.Split(channel.CustomModel.String, ",") {
					addModel(channel.ID, name, model.ChannelModelSourceManual)
				}
			}
		}
		for _, item := range legacyItems {
			source := model.ChannelModelSourceManual
			if channelAutoSync[item.ChannelID] {
				source = model.ChannelModelSourceAuto
			}
			addModel(item.ChannelID, item.ModelName, source)
		}
		if len(missing) > 0 {
			if err := tx.Clauses(clause.OnConflict{DoNothing: true}).Create(&missing).Error; err != nil {
				return fmt.Errorf("failed to create channel_models: %w", err)
			}
			// 冲突跳过的行不会回写主键，统一重新读取以取得真实主键。
			channelModels = channelModels[:0]
			if err := tx.Order("id ASC").Find(&channelModels).Error; err != nil {
				return fmt.Errorf("failed to reload channel_models: %w", err)
			}
			indexChannelModels()
		}

		// SQLite 和 PostgreSQL 无法直接追加无默认值的非空列，先带默认值补列，再由 AutoMigrate 收敛类型。
		if !hasPhysicalColumn(tx, "group_items", "channel_model_id") {
			if err := tx.Exec("ALTER TABLE group_items ADD COLUMN channel_model_id integer NOT NULL DEFAULT 0").Error; err != nil {
				return fmt.Errorf("failed to add group_items.channel_model_id: %w", err)
			}
		}

		// 无法定位渠道模型或与同组已有分组项重复的旧行直接删除。
		invalidIDs := make([]int, 0)
		seen := make(map[[2]int]struct{}, len(legacyItems))
		for _, item := range legacyItems {
			channelModelID, ok := findModelID(item.ChannelID, item.ModelName)
			itemKey := [2]int{item.GroupID, channelModelID}
			if _, exists := seen[itemKey]; !ok || exists {
				invalidIDs = append(invalidIDs, item.ID)
				continue
			}
			seen[itemKey] = struct{}{}
			if err := tx.Model(&model.GroupItem{}).Where("id = ?", item.ID).
				Update("channel_model_id", channelModelID).Error; err != nil {
				return fmt.Errorf("failed to update group_item %d: %w", item.ID, err)
			}
		}
		if len(invalidIDs) > 0 {
			if err := tx.Where("id IN ?", invalidIDs).Delete(&model.GroupItem{}).Error; err != nil {
				return fmt.Errorf("failed to delete invalid group_items: %w", err)
			}
		}
		if hasPhysicalColumn(tx, "groups", "active_item_id") {
			if err := clearStaleActiveItems(tx); err != nil {
				return err
			}
		}

		// 旧唯一索引包含 channel_id 和 model_name，必须先删除索引再删列，否则会退化为分组唯一。
		// MySQL 和 TiDB 要求外键列上有索引，group_id 上只有这个旧索引，直接删会报 1553，
		// 必须先删 group_id 外键，之后由 AutoMigrate 依据 Group.Items 重建外键和新索引。
		// GORM 的 DropConstraint 只发 DROP CONSTRAINT，TiDB 对外键会静默忽略该语句，
		// 因此这里显式使用 DROP FOREIGN KEY。
		if tx.Migrator().HasIndex(&model.GroupItem{}, "idx_group_channel_model") {
			if tx.Dialector.Name() == "mysql" && tx.Migrator().HasConstraint(&model.Group{}, "fk_groups_items") {
				if err := tx.Exec("ALTER TABLE group_items DROP FOREIGN KEY fk_groups_items").Error; err != nil {
					return fmt.Errorf("failed to drop group_items foreign key: %w", err)
				}
			}
			if err := tx.Migrator().DropIndex(&model.GroupItem{}, "idx_group_channel_model"); err != nil {
				return fmt.Errorf("failed to drop legacy group_items index: %w", err)
			}
		}
		for _, column := range []string{"channel_id", "model_name"} {
			if err := dropColumnIfExists(tx, &model.GroupItem{}, "group_items", column); err != nil {
				return err
			}
		}
		return nil
	})
}
