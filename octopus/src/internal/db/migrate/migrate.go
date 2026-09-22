package migrate

import (
	"fmt"
	"sort"
	"strings"

	"gorm.io/gorm"
	"gorm.io/gorm/clause"
)

type Migration struct {
	Version int
	Up      func(db *gorm.DB) error
}

type MigrationRecordStatus int

const (
	MigrationRecordStatusSuccess MigrationRecordStatus = iota + 1 // 迁移已执行成功，后续启动跳过。
	MigrationRecordStatusFailed                                   // 迁移执行失败，后续启动重试。
)

type MigrationRecord struct {
	Version int `gorm:"primaryKey"`
	Status  MigrationRecordStatus
}

var beforeAutoMigrations = make([]Migration, 0)
var afterAutoMigrations = make([]Migration, 0)

func RegisterBeforeAutoMigration(m Migration) {
	beforeAutoMigrations = append(beforeAutoMigrations, m)
}

func RegisterAfterAutoMigration(m Migration) {
	afterAutoMigrations = append(afterAutoMigrations, m)
}

func BeforeAutoMigrate(db *gorm.DB) error {
	if err := runMigrationsWithRecord(db, beforeAutoMigrations); err != nil {
		return err
	}
	beforeAutoMigrations = nil
	return nil
}

func AfterAutoMigrate(db *gorm.DB) error {
	if err := runMigrationsWithRecord(db, afterAutoMigrations); err != nil {
		return err
	}
	afterAutoMigrations = nil
	return nil
}

func runMigrationsWithRecord(db *gorm.DB, migrations []Migration) error {
	if len(migrations) == 0 {
		return nil
	}
	if db == nil {
		return fmt.Errorf("db is nil")
	}
	if err := ensureMigrationRecordTable(db); err != nil {
		return err
	}

	// 排序
	sort.Slice(migrations, func(i, j int) bool {
		return migrations[i].Version < migrations[j].Version
	})

	// check duplicated versions
	seen := make(map[int]struct{}, len(migrations))
	versions := make([]int, 0, len(migrations))
	for _, m := range migrations {
		if _, ok := seen[m.Version]; ok {
			return fmt.Errorf("duplicated migration version: %d", m.Version)
		}
		seen[m.Version] = struct{}{}
		versions = append(versions, m.Version)
	}

	// load records in batch to avoid N queries
	existing := make([]MigrationRecord, 0)
	if err := db.Where("version IN ?", versions).Find(&existing).Error; err != nil {
		return fmt.Errorf("failed to query migration records: %w", err)
	}
	statusByVersion := make(map[int]MigrationRecordStatus, len(existing))
	for _, r := range existing {
		statusByVersion[r.Version] = r.Status
	}

	for _, m := range migrations {
		if m.Up == nil {
			return fmt.Errorf("migration %d has nil Up", m.Version)
		}

		// 已成功则跳过
		if st, ok := statusByVersion[m.Version]; ok && st == MigrationRecordStatusSuccess {
			continue
		}

		// 执行迁移
		if err := m.Up(db); err != nil {
			upsertMigrationRecord(db, m.Version, MigrationRecordStatusFailed)
			statusByVersion[m.Version] = MigrationRecordStatusFailed
			return fmt.Errorf("failed to run migration %d: %w", m.Version, err)
		}

		// 记录成功
		if err := upsertMigrationRecord(db, m.Version, MigrationRecordStatusSuccess); err != nil {
			return fmt.Errorf("failed to set migration %d success: %w", m.Version, err)
		}
		statusByVersion[m.Version] = MigrationRecordStatusSuccess
	}
	return nil
}

func ensureMigrationRecordTable(db *gorm.DB) error {
	if db.Migrator().HasTable(&MigrationRecord{}) {
		return nil
	}
	// For BeforeAutoMigrate: the record table may not exist yet.
	if err := db.AutoMigrate(&MigrationRecord{}); err != nil {
		return fmt.Errorf("failed to auto migrate MigrationRecord: %w", err)
	}
	return nil
}

func upsertMigrationRecord(db *gorm.DB, version int, status MigrationRecordStatus) error {
	rec := MigrationRecord{Version: version, Status: status}
	return db.Clauses(clause.OnConflict{
		Columns:   []clause.Column{{Name: "version"}},
		DoUpdates: clause.AssignmentColumns([]string{"status"}),
	}).Create(&rec).Error
}

// hasPhysicalColumn 判断数据库表中是否存在指定列，不依赖当前模型是否仍声明该字段。
func hasPhysicalColumn(db *gorm.DB, table, column string) bool {
	columnTypes, err := db.Migrator().ColumnTypes(table)
	if err != nil {
		return false
	}
	for _, columnType := range columnTypes {
		if strings.EqualFold(columnType.Name(), column) {
			return true
		}
	}
	return false
}

// dropColumnIfExists 删除指定表中存在的列，兼容 SQLite、MySQL 和 PostgreSQL。
func dropColumnIfExists(db *gorm.DB, model interface{}, table, column string) error {
	if !hasPhysicalColumn(db, table, column) {
		return nil
	}
	if db.Dialector.Name() == "sqlite" {
		if err := db.Exec(fmt.Sprintf(`ALTER TABLE %q DROP COLUMN %q`, table, column)).Error; err != nil {
			return fmt.Errorf("failed to drop %s.%s: %w", table, column, err)
		}
		return nil
	}
	if err := db.Migrator().DropColumn(model, column); err != nil {
		return fmt.Errorf("failed to drop %s.%s: %w", table, column, err)
	}
	return nil
}
