package download

import (
	"fluxor/internal/config"
	"fluxor/internal/core"
	"log"
)

// DownloadSubscriptionFile 下载单个订阅的节点文件，返回元数据
// 优先尝试直接 HTTP 下载，失败则回退到临时内核方式
func DownloadSubscriptionFile(sub config.Subscription, index int, targetFile string) (updatedAt string, subInfo map[string]interface{}, err error) {
	// 1. 尝试直接下载
	directUpdatedAt, directSubInfo, directErr := tryDirectDownload(sub, targetFile)
	if directErr == nil {
		return directUpdatedAt, NormalizeMapKeys(directSubInfo), nil
	}
	// 直接下载失败，记录日志并回退到临时内核
	if core.CoreLogger != nil {
		core.CoreLogger.Printf("[DOWNLOAD] 订阅 %s 直连下载失败: %v，回退到临时内核", sub.Name, directErr)
	} else {
		log.Printf("[DOWNLOAD] 订阅 %s 直连下载失败: %v，回退到临时内核", sub.Name, directErr)
	}

	// 2. 回退到原有临时内核流程
	updatedAt, subInfo, err = core.DownloadWithTempCore(sub, index, targetFile)
	if err != nil {
		return "", nil, err
	}
	return updatedAt, NormalizeMapKeys(subInfo), nil
}
