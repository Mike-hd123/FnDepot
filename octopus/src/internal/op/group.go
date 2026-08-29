package op

import (
	"context"
	"fmt"
	"slices"
	"sort"
	"strings"

	"github.com/bestruirui/octopus/internal/db"
	"github.com/bestruirui/octopus/internal/model"
	"github.com/bestruirui/octopus/internal/utils/cache"
	"gorm.io/gorm"
)

var (
	groupCache     = cache.New[int, model.Group](16) // 按主键保存完整分组配置。
	groupNameIndex = cache.New[string, int](16)      // 客户端模型名对应的分组主键。
)

// GroupList 返回缓存中的全部分组。
func GroupList() []model.Group {
	groups := make([]model.Group, 0, groupCache.Len())
	for _, group := range groupCache.GetAll() {
		groups = append(groups, groupSnapshot(group))
	}
	return groups
}

// GroupListModel 返回缓存中的全部分组模型名。
func GroupListModel() []string {
	models := make([]string, 0, groupCache.Len())
	for _, group := range groupCache.GetAll() {
		models = append(models, group.Name)
	}
	return models
}

// GroupGetByName 返回客户端模型名称对应的完整分组配置。
func GroupGetByName(name string) (model.Group, error) {
	groupID, ok := groupNameIndex.Get(name)
	if !ok {
		return model.Group{}, fmt.Errorf("group not found")
	}
	group, ok := groupCache.Get(groupID)
	if !ok {
		return model.Group{}, fmt.Errorf("group not found")
	}
	return groupSnapshot(group), nil
}

// GroupCreate 创建分组及其成员并刷新缓存。
func GroupCreate(group *model.Group, ctx context.Context) error {
	if group == nil {
		return fmt.Errorf("group is required")
	}
	group.ID = 0
	group.Name = strings.TrimSpace(group.Name)
	if group.Name == "" {
		return fmt.Errorf("group name is required")
	}
	group.ActiveItemID = 0
	if group.Mode == "" {
		group.Mode = model.GroupModeManual
	}
	model.NormalizeGroupRelayConfig(&group.RelayConfig)
	for i := range group.Items {
		group.Items[i].ID = 0
		group.Items[i].GroupID = 0
		group.Items[i].ChannelModel = nil
	}
	if err := db.GetDB().WithContext(ctx).Create(group).Error; err != nil {
		return err
	}
	sortGroupItems(group.Items)
	groupCache.Set(group.ID, groupSnapshot(*group))
	groupNameIndex.Set(group.Name, group.ID)
	return nil
}

// GroupUpdate 更新分组配置和成员，并返回刷新后的分组。
func GroupUpdate(req *model.GroupUpdateRequest, ctx context.Context) (*model.Group, error) {
	oldGroup, ok := groupCache.Get(req.ID)
	if !ok {
		return nil, fmt.Errorf("group not found")
	}
	oldName := oldGroup.Name

	var selectFields []string
	updates := model.Group{ID: req.ID}
	if req.Name != nil {
		name := strings.TrimSpace(*req.Name)
		if name == "" {
			return nil, fmt.Errorf("group name is required")
		}
		selectFields = append(selectFields, "name")
		updates.Name = name
	}
	if req.Mode != nil {
		selectFields = append(selectFields, "mode")
		updates.Mode = *req.Mode
	}
	if req.RelayConfig != nil {
		config := *req.RelayConfig
		model.NormalizeGroupRelayConfig(&config)
		selectFields = append(selectFields, "relay_config")
		updates.RelayConfig = config
	}

	newItems := make([]model.GroupItem, len(req.ItemsToAdd))
	for i, item := range req.ItemsToAdd {
		newItems[i] = model.GroupItem{
			GroupID:        req.ID,
			ChannelModelID: item.ChannelModelID,
			Priority:       item.Priority,
		}
	}
	var group model.Group
	err := db.GetDB().WithContext(ctx).Transaction(func(tx *gorm.DB) error {
		if len(selectFields) > 0 {
			if err := tx.Model(&model.Group{}).Where("id = ?", req.ID).Select(selectFields).Updates(&updates).Error; err != nil {
				return fmt.Errorf("failed to update group: %w", err)
			}
		}

		if len(req.ItemsToDelete) > 0 {
			var deletedIDs []int
			if err := tx.Model(&model.GroupItem{}).
				Where("id IN ? AND group_id = ?", req.ItemsToDelete, req.ID).
				Pluck("id", &deletedIDs).Error; err != nil {
				return fmt.Errorf("failed to find deleted items: %w", err)
			}
			if len(deletedIDs) > 0 {
				if err := tx.Model(&model.Group{}).
					Where("id = ? AND active_item_id IN ?", req.ID, deletedIDs).
					Update("active_item_id", 0).Error; err != nil {
					return fmt.Errorf("failed to clear active item: %w", err)
				}
				if err := tx.Where("id IN ?", deletedIDs).Delete(&model.GroupItem{}).Error; err != nil {
					return fmt.Errorf("failed to delete items: %w", err)
				}
			}
		}

		if len(req.ItemsToUpdate) > 0 {
			ids := make([]int, len(req.ItemsToUpdate))
			priorityCase := "CASE id"
			for i, item := range req.ItemsToUpdate {
				ids[i] = item.ID
				priorityCase += fmt.Sprintf(" WHEN %d THEN %d", item.ID, item.Priority)
			}
			priorityCase += " END"
			if err := tx.Model(&model.GroupItem{}).
				Where("id IN ? AND group_id = ?", ids, req.ID).
				Updates(map[string]interface{}{"priority": gorm.Expr(priorityCase)}).Error; err != nil {
				return fmt.Errorf("failed to update items: %w", err)
			}
		}

		if len(newItems) > 0 {
			if err := tx.Create(&newItems).Error; err != nil {
				return fmt.Errorf("failed to create items: %w", err)
			}
		}

		if err := tx.Preload("Items").First(&group, req.ID).Error; err != nil {
			return fmt.Errorf("failed to load updated group: %w", err)
		}
		return nil
	})
	if err != nil {
		return nil, err
	}

	sortGroupItems(group.Items)
	snapshot := groupSnapshot(group)
	groupCache.Set(group.ID, snapshot)
	groupNameIndex.Set(group.Name, group.ID)
	if oldName != group.Name {
		groupNameIndex.Del(oldName)
	}
	return &snapshot, nil
}

// GroupActiveItemUpdate 更新或清空分组当前手动指定的成员。
func GroupActiveItemUpdate(groupID int, req *model.GroupActiveItemUpdateRequest, ctx context.Context) (*model.Group, error) {
	group, ok := groupCache.Get(groupID)
	if !ok {
		return nil, fmt.Errorf("group not found")
	}
	itemID := 0
	if req.ItemID != nil && *req.ItemID != 0 {
		itemID = *req.ItemID
		found := false
		for _, item := range group.Items {
			if item.ID == itemID {
				found = true
				break
			}
		}
		if !found {
			return nil, fmt.Errorf("group item not found")
		}
	}
	if err := db.GetDB().WithContext(ctx).Model(&model.Group{}).Where("id = ?", groupID).Update("active_item_id", itemID).Error; err != nil {
		return nil, fmt.Errorf("failed to update active item: %w", err)
	}
	group.ActiveItemID = itemID
	snapshot := groupSnapshot(group)
	groupCache.Set(group.ID, snapshot)
	return &snapshot, nil
}

// GroupDel 删除分组及其成员，成员删除不会影响被其他分组引用的渠道模型。
func GroupDel(id int, ctx context.Context) error {
	group, ok := groupCache.Get(id)
	if !ok {
		return fmt.Errorf("group not found")
	}
	if err := db.GetDB().WithContext(ctx).Delete(&model.Group{}, id).Error; err != nil {
		return fmt.Errorf("failed to delete group: %w", err)
	}
	groupCache.Del(id)
	groupNameIndex.Del(group.Name)
	return nil
}

// groupRefreshCache 从数据库刷新完整分组缓存和名称索引。
func groupRefreshCache(ctx context.Context) error {
	groups := []model.Group{}
	if err := db.GetDB().WithContext(ctx).
		Preload("Items").
		Find(&groups).Error; err != nil {
		return err
	}
	groupCache.Clear()
	groupNameIndex.Clear()
	for _, group := range groups {
		sortGroupItems(group.Items)
		groupCache.Set(group.ID, groupSnapshot(group))
		groupNameIndex.Set(group.Name, group.ID)
	}
	return nil
}

// sortGroupItems 按优先级和主键生成稳定的成员顺序。
func sortGroupItems(items []model.GroupItem) {
	sort.Slice(items, func(i, j int) bool {
		if items[i].Priority != items[j].Priority {
			return items[i].Priority < items[j].Priority
		}
		return items[i].ID < items[j].ID
	})
}

// groupSnapshot 从渠道模型缓存补齐成员关联对象。
func groupSnapshot(group model.Group) model.Group {
	group.Items = slices.Clone(group.Items)
	for i := range group.Items {
		channelModel, err := ChannelModelGet(group.Items[i].ChannelModelID)
		if err != nil {
			group.Items[i].ChannelModel = nil
			continue
		}
		group.Items[i].ChannelModel = &channelModel
	}
	return group
}
