package subscription

import (
	"encoding/json"
	"fluxor/internal/config"
	"fluxor/internal/core"
	"fluxor/internal/subscription/download"
	"fmt"
	"io"
	"log"
	"net/http"
	"net/url"
	"time"
)

// fetchSubscriptionMetadataFromCore 从主内核获取订阅元数据
func fetchSubscriptionMetadataFromCore(subName string) (updatedAt string, subInfo map[string]interface{}, err error) {
	encoded := url.QueryEscape(subName)
	resp, err := core.CoreRequest("GET", "/providers/proxies/"+encoded, nil)
	if err != nil {
		return "", nil, err
	}
	defer resp.Body.Close()
	if resp.StatusCode != http.StatusOK {
		return "", nil, fmt.Errorf("状态码: %d", resp.StatusCode)
	}
	body, err := io.ReadAll(resp.Body)
	if err != nil {
		return "", nil, err
	}
	var data map[string]interface{}
	if err := json.Unmarshal(body, &data); err != nil {
		return "", nil, err
	}
	updatedAtVal, _ := data["updatedAt"].(string)
	subInfoVal, _ := data["subscriptionInfo"].(map[string]interface{})
	// 规范化键为小写
	subInfoVal = download.NormalizeMapKeys(subInfoVal)
	return updatedAtVal, subInfoVal, nil
}

// updateAllSubscriptionsMetadata 更新所有订阅的元数据（仅用于融合模式）。
//
// 采用「锁外抓取、锁内写回」：抓取阶段含重试与 500ms 退避，绝不能持
// config.Mu 进行（会长时间阻塞 /subscribe/config 等读者）；但写回阶段必须
// 持锁——cfg.Subscriptions 与 config.Current.Subscriptions 通常是同一份底层
// 数组，无锁写入会与 RLock 下的读取构成数据竞争。
func updateAllSubscriptionsMetadata(cfg *config.SubscribeConfig) {
	// 1. 锁内备份旧元数据（用于抓取失败时保留）
	config.Mu.RLock()
	oldSubs := make(map[string]config.Subscription, len(config.Current.Subscriptions))
	for _, s := range config.Current.Subscriptions {
		oldSubs[s.Name] = s
	}
	config.Mu.RUnlock()

	// 2. 锁外抓取
	type metaResult struct {
		name      string
		updatedAt string
		subInfo   map[string]interface{}
	}
	results := make([]metaResult, 0, len(cfg.Subscriptions))

	for i := range cfg.Subscriptions {
		name := cfg.Subscriptions[i].Name
		var updatedAt string
		var subInfo map[string]interface{}
		var err error

		// 3. 重试机制：最多尝试 3 次，每次间隔 500ms
		for attempt := 0; attempt < 3; attempt++ {
			if attempt > 0 {
				time.Sleep(500 * time.Millisecond)
			}
			updatedAt, subInfo, err = fetchSubscriptionMetadataFromCore(name)
			if err == nil {
				break
			}
			log.Printf("获取订阅 %s 元数据失败 (尝试 %d/%d): %v", name, attempt+1, 3, err)
		}

		if err != nil {
			// 获取失败：尝试保留旧数据
			if old, ok := oldSubs[name]; ok {
				updatedAt, subInfo = old.UpdatedAt, old.SubscriptionInfo
				log.Printf("保留订阅 %s 的旧元数据（获取失败）", name)
			} else {
				updatedAt, subInfo = "", nil
				log.Printf("订阅 %s 无历史元数据，保留为空", name)
			}
		} else {
			log.Printf("更新订阅 %s 元数据成功", name)
		}

		results = append(results, metaResult{name: name, updatedAt: updatedAt, subInfo: subInfo})
	}

	// 4. 锁内一次性写回（临界区只做字段赋值）
	config.Mu.Lock()
	defer config.Mu.Unlock()
	for _, r := range results {
		for i := range cfg.Subscriptions {
			if cfg.Subscriptions[i].Name == r.name {
				cfg.Subscriptions[i].UpdatedAt = r.updatedAt
				cfg.Subscriptions[i].SubscriptionInfo = r.subInfo
				break
			}
		}
		// 同时写入当前生效配置：二者通常共享底层数组，但若期间 Current 已被
		// 其它请求替换，则需保证全局状态也能拿到最新元数据。
		for i := range config.Current.Subscriptions {
			if config.Current.Subscriptions[i].Name == r.name {
				config.Current.Subscriptions[i].UpdatedAt = r.updatedAt
				config.Current.Subscriptions[i].SubscriptionInfo = r.subInfo
				break
			}
		}
	}
}
