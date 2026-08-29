package task

import (
	"context"
	"fmt"
	"strings"
	"sync"
	"time"

	"github.com/bestruirui/octopus/internal/helper"
	"github.com/bestruirui/octopus/internal/model"
	"github.com/bestruirui/octopus/internal/op"
	"github.com/bestruirui/octopus/internal/price"
	"github.com/charmbracelet/log"
)

var (
	syncModelsMu        sync.Mutex   // 保证同一时间只有一个模型同步任务运行。
	lastSyncModelsTimeMu sync.RWMutex // 最近同步时间的读写锁。
	lastSyncModelsTime   = time.Now() // 最近一次模型同步任务结束时间。
)

// SyncModelsTask 同步渠道模型并清理失效关联，返回本次同步遇到的首个错误。
func SyncModelsTask() error {
	if !syncModelsMu.TryLock() {
		return fmt.Errorf("model sync already running")
	}
	defer syncModelsMu.Unlock()

	log.Debugf("sync models task started")
	startTime := time.Now()
	defer func() {
		log.Debugf("sync models task finished, sync time: %s", time.Since(startTime))
	}()
	defer func() {
		lastSyncModelsTimeMu.Lock()
		lastSyncModelsTime = time.Now()
		lastSyncModelsTimeMu.Unlock()
	}()
	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Minute)
	defer cancel()
	channels := op.ChannelList()
	var syncErr error
	for _, channel := range channels {
		if !channel.AutoSync {
			continue
		}
		fetchModels, err := helper.FetchModels(ctx, channel)
		if err != nil {
			log.Warnf("failed to sync models for channel %s: %v", channel.Name, err)
			if syncErr == nil {
				syncErr = fmt.Errorf("failed to fetch models for channel %s: %w", channel.Name, err)
			}
			continue
		}

		manualNames := make(map[string]struct{})
		oldAutoNames := make(map[string]struct{})
		models := make([]model.ChannelModel, 0, len(channel.Models)+len(fetchModels))
		for _, channelModel := range channel.Models {
			switch channelModel.Source {
			case model.ChannelModelSourceManual:
				manualNames[channelModel.Name] = struct{}{}
				models = append(models, channelModel)
			case model.ChannelModelSourceAuto:
				oldAutoNames[channelModel.Name] = struct{}{}
			}
		}
		// 外部返回的模型名只在进入内部流程时清洗一次，并由手动模型优先占用重复名称。
		seen := make(map[string]struct{}, len(fetchModels))
		autoModels := make([]model.ChannelModel, 0, len(fetchModels))
		for _, modelName := range fetchModels {
			modelName = strings.TrimSpace(modelName)
			if modelName == "" {
				continue
			}
			if _, ok := manualNames[modelName]; ok {
				continue
			}
			if _, ok := seen[modelName]; ok {
				continue
			}
			seen[modelName] = struct{}{}
			autoModels = append(autoModels, model.ChannelModel{Name: modelName, Source: model.ChannelModelSourceAuto})
		}
		addedModels := make([]string, 0)
		newAutoNames := make(map[string]struct{}, len(autoModels))
		for _, channelModel := range autoModels {
			newAutoNames[channelModel.Name] = struct{}{}
			if _, ok := oldAutoNames[channelModel.Name]; !ok {
				addedModels = append(addedModels, channelModel.Name)
			}
		}
		deletedModels := make([]string, 0)
		for name := range oldAutoNames {
			if _, ok := newAutoNames[name]; !ok {
				deletedModels = append(deletedModels, name)
			}
		}
		if len(deletedModels) == 0 && len(addedModels) == 0 {
			continue
		}
		models = append(models, autoModels...)

		if len(addedModels) > 0 {
			llmInfos := make([]model.LLMInfo, 0, len(addedModels))
			// 渠道模型保留大小写，写入价格表时按小写主键去重。
			seenPriceNames := make(map[string]struct{}, len(addedModels))
			for _, modelName := range addedModels {
				modelName = strings.ToLower(modelName)
				if _, ok := seenPriceNames[modelName]; ok {
					continue
				}
				seenPriceNames[modelName] = struct{}{}
				llmInfo := model.LLMInfo{Name: modelName}
				if modelPrice := price.GetLLMPrice(llmInfo.Name); modelPrice != nil {
					llmInfo.LLMPrice = *modelPrice
				}
				llmInfos = append(llmInfos, llmInfo)
			}
			if err := op.LLMBatchCreate(llmInfos, ctx); err != nil {
				log.Warnf("failed to sync models for channel %s: %v", channel.Name, err)
				if syncErr == nil {
					syncErr = fmt.Errorf("failed to save model prices for channel %s: %w", channel.Name, err)
				}
				continue
			}
		}
		if _, err := op.ChannelUpdate(&model.ChannelUpdateRequest{
			ID:    channel.ID,
			Models: &models,
		}, ctx); err != nil {
			log.Warnf("failed to sync models for channel %s: %v", channel.Name, err)
			if syncErr == nil {
				syncErr = fmt.Errorf("failed to update channel %s models: %w", channel.Name, err)
			}
			continue
		}
		if len(deletedModels) > 0 {
			log.Infof("deleted channel %s models: %v", channel.Name, deletedModels)
		}
	}
	if err := op.LLMCleanupGhosts(ctx); err != nil {
		log.Errorf("failed to clean ghost model prices: %v", err)
		if syncErr == nil {
			syncErr = fmt.Errorf("failed to clean ghost model prices: %w", err)
		}
	}
	return syncErr
}

// GetLastSyncModelsTime 返回最近一次模型同步任务结束时间。
func GetLastSyncModelsTime() time.Time {
	lastSyncModelsTimeMu.RLock()
	defer lastSyncModelsTimeMu.RUnlock()
	return lastSyncModelsTime
}
