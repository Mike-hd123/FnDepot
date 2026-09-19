package subscription

import (
	"fluxor/internal/config"
	"fluxor/internal/subscription/download"
	"fmt"
	"log"
	"os"
	"path/filepath"
)

// subscriptionSnapshot 参数，用于在锁外执行下载
type subscriptionSnapshot struct {
	sub        config.Subscription
	idx        int
	mode       string
	isActive   bool
	proxiesDir string
	// cfg 是锁内对 config.Current 的值拷贝，供锁外打补丁时读取标量字段
	// （端口/密钥/面板等）。注意其 Subscriptions 与全局共享同一底层数组，
	// 因此锁外只允许读取标量字段，不得遍历该切片。
	cfg config.SubscribeConfig
}

// takeSubscriptionSnapshot 在锁内取一份订阅快照（供锁外下载使用）。
//
// 锁只用于读取内存字段，不覆盖任何网络 IO。
func takeSubscriptionSnapshot(subName string) (subscriptionSnapshot, bool) {
	config.Mu.RLock()
	defer config.Mu.RUnlock()

	snap := subscriptionSnapshot{
		idx:        -1,
		mode:       config.Current.Mode,
		proxiesDir: filepath.Join(config.CoreWorkDir, "proxies"),
		cfg:        config.Current,
	}
	for i := range config.Current.Subscriptions {
		if config.Current.Subscriptions[i].Name == subName {
			snap.idx = i
			snap.sub = config.Current.Subscriptions[i]
			break
		}
	}
	if snap.idx == -1 {
		return snap, false
	}
	snap.isActive = snap.mode == "switch" && config.Current.ActiveSubscription == subName
	return snap, true
}

// applySubscriptionMetadata 在锁内写回下载得到的元数据（仅字段赋值，无 IO）。
func applySubscriptionMetadata(subName, updatedAt string, subInfo map[string]interface{}) {
	config.Mu.Lock()
	defer config.Mu.Unlock()
	for i := range config.Current.Subscriptions {
		if config.Current.Subscriptions[i].Name == subName {
			config.Current.Subscriptions[i].UpdatedAt = updatedAt
			config.Current.Subscriptions[i].SubscriptionInfo = subInfo
			return
		}
	}
}

// fetchAndPatchSubscription 在【锁外】执行下载与打补丁，返回元数据。
//
// 下载链路为「直连 → 失败则回退临时内核」，两阶段各以 15 秒为上限且不重试，
// 即单次更新最长约 30 秒。此前调用方是「持 config.Mu 写锁」调用本流程，
// 而该锁被 core.CoreRequest、wsproxy、quality、tproxy、healthcheck 等 10 处
// 读取点共用——等于一次订阅更新就会阻塞整个面板。故拆分为：
// 锁内取快照 → 锁外下载 → 锁内写回。
func fetchAndPatchSubscription(snap subscriptionSnapshot, subName string) (updatedAt string, subInfo map[string]interface{}, targetFile string, err error) {
	targetFile = filepath.Join(snap.proxiesDir, config.SanitizeSubscriptionFileName(subName))

	// 强制删除已有文件（确保重新下载）
	if err := os.Remove(targetFile); err != nil && !os.IsNotExist(err) {
		return "", nil, targetFile, fmt.Errorf("删除旧文件失败: %w", err)
	}

	log.Printf("[UPDATE] 开始下载订阅 %s，目标文件: %s", subName, targetFile)

	updatedAt, subInfo, err = download.DownloadSubscriptionFile(snap.sub, snap.idx, targetFile)
	if err != nil {
		log.Printf("[UPDATE] 下载订阅 %s 失败: %v", subName, err)
		return "", nil, targetFile, fmt.Errorf("下载失败: %w", err)
	}
	log.Printf("[UPDATE] 元数据已更新: updatedAt=%s", updatedAt)

	// 打补丁。补丁只依赖快照中的标量配置，无需（也不应）持有全局锁。
	log.Printf("[UPDATE] 开始打补丁: %s", targetFile)
	if err := patchSubscriptionFile(targetFile, snap.cfg); err != nil {
		log.Printf("[UPDATE] 打补丁失败: %v", err)
		return "", nil, targetFile, fmt.Errorf("打补丁失败: %w", err)
	}
	log.Printf("[UPDATE] 补丁完成")
	return updatedAt, subInfo, targetFile, nil
}

// updateSubscriptionInSwitchMode 切换模式下的订阅更新逻辑，返回 needsReload 表示是否需要重载内核。
//
// 注意：本函数会执行网络下载，调用方【不得】持有 config.Mu。
// 内部自行在锁内取快照、锁外下载、锁内写回元数据。
func updateSubscriptionInSwitchMode(subName string) (needsReload bool, err error) {
	snap, ok := takeSubscriptionSnapshot(subName)
	if !ok {
		return false, fmt.Errorf("订阅 %s 不存在", subName)
	}

	updatedAt, subInfo, targetFile, err := fetchAndPatchSubscription(snap, subName)
	if err != nil {
		return false, err
	}
	applySubscriptionMetadata(subName, updatedAt, subInfo)

	// 如果该订阅是当前激活的订阅，则复制到 configTarget，并标记需要重载
	if snap.isActive {
		log.Printf("[UPDATE] 当前订阅为激活订阅，开始复制配置文件到 %s", config.ConfigTarget)
		if err := copyFile(targetFile, config.ConfigTarget); err != nil {
			log.Printf("[UPDATE] 复制失败: %v", err)
			return false, fmt.Errorf("复制配置文件失败: %w", err)
		}
		log.Printf("[UPDATE] 复制完成")
		return true, nil // 需要重载
	}

	log.Printf("[UPDATE] 当前订阅非激活订阅，跳过复制和重载")
	return false, nil
}
