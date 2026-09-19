package subscription

import (
	"fluxor/internal/config"
	"fluxor/internal/subscription/download"
	"fmt"
	"log"
	"os"
	"path/filepath"
	"sync"
)

// ensureSubscriptionFiles 在切换模式下，确保所有订阅的本地文件已下载
// 若模式为 "merge" 或无订阅，则直接返回 nil
func ensureSubscriptionFiles(cfg *config.SubscribeConfig) error {
	if cfg.Mode != "switch" {
		return nil
	}
	if len(cfg.Subscriptions) == 0 {
		return nil
	}

	proxiesDir := filepath.Join(config.CoreWorkDir, "proxies")
	if err := os.MkdirAll(proxiesDir, 0755); err != nil {
		return fmt.Errorf("创建 proxies 目录失败: %w", err)
	}

	var wg sync.WaitGroup
	errCh := make(chan error, len(cfg.Subscriptions))
	sem := make(chan struct{}, 5)

	for i := range cfg.Subscriptions {
		wg.Add(1)
		go func(idx int) {
			defer wg.Done()
			sem <- struct{}{}
			defer func() { <-sem }()

			s := cfg.Subscriptions[idx]
			targetFile := filepath.Join(proxiesDir, config.SanitizeSubscriptionFileName(s.Name))

			// 检查文件是否存在以及是否有元数据
			needDownload := false
			if info, err := os.Stat(targetFile); err != nil || info.Size() == 0 {
				needDownload = true
			} else {
				// 文件存在且非空，检查元数据是否缺失
				if len(cfg.Subscriptions[idx].SubscriptionInfo) == 0 {
					needDownload = true
					// 为保险，先删除旧文件，确保重新下载
					if err := os.Remove(targetFile); err != nil && !os.IsNotExist(err) {
						errCh <- fmt.Errorf("订阅 %s 删除旧文件失败: %w", s.Name, err)
						return
					}
				}
			}

			if needDownload {
				// 下载文件并获取元数据
				updatedAt, subInfo, err := download.DownloadSubscriptionFile(s, idx, targetFile)
				if err != nil {
					errCh <- fmt.Errorf("订阅 %s 下载失败: %w", s.Name, err)
					return
				}
				cfg.Subscriptions[idx].UpdatedAt = updatedAt
				cfg.Subscriptions[idx].SubscriptionInfo = subInfo
				log.Printf("[ensure] 已下载并更新订阅 %s 元数据", s.Name)
				// 下载成功后打补丁
				if err := patchSubscriptionFile(targetFile, *cfg); err != nil {
					errCh <- fmt.Errorf("订阅 %s 打补丁失败: %w", s.Name, err)
				}
			} else {
				// 文件已存在且元数据完整，只打补丁
				if err := patchSubscriptionFile(targetFile, *cfg); err != nil {
					errCh <- fmt.Errorf("订阅 %s 打补丁失败: %w", s.Name, err)
				}
			}
		}(i)
	}

	wg.Wait()
	close(errCh)

	var errs []error
	for err := range errCh {
		errs = append(errs, err)
	}
	if len(errs) > 0 {
		return fmt.Errorf("部分操作失败: %v", errs)
	}
	return nil
}
