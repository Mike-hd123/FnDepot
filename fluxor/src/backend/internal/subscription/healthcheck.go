package subscription

import (
	"fluxor/internal/config"
	"fluxor/internal/dashapi"
	"log"
	"sync"
	"time"
)

var (
	// healthCheckLifecycleMu 串行化健康检查定时器的启停，并保护
	// healthCheckTicker / healthCheckStop 这两个句柄不被并发读写。
	healthCheckLifecycleMu sync.Mutex
	healthCheckTicker      *time.Ticker
	healthCheckStop        chan struct{}
	lastHealthCheck        map[string]time.Time
	healthCheckMu          sync.RWMutex
)

// startHealthCheckTimer 启动健康检查定时器（仅在 switch 模式下生效）。
//
// activeSub 由调用方在锁内取好传入，本函数自身不再触碰 config.Mu：
// 若在此获取 config.Mu，会与调用方（StartAllTimers）可能持有的读锁形成
// 「读锁重入 + 排队写者」的死锁。
func startHealthCheckTimer(activeSub string) {
	healthCheckLifecycleMu.Lock()
	defer healthCheckLifecycleMu.Unlock()

	if healthCheckTicker != nil {
		return
	}

	// ticker 与 stop 先落局部变量，再由闭包按值捕获。
	// 绝不能让 goroutine 读取会被置 nil 的包级变量：select 每轮都会重新求值
	// case 表达式，一旦读到 nil 的 *time.Ticker 就是对 nil 解引用，
	// 该 goroutine 没有 recover，会直接终止整个进程。
	ticker := time.NewTicker(10 * time.Second) // 每10秒检查一次
	stop := make(chan struct{})
	healthCheckTicker = ticker
	healthCheckStop = stop

	healthCheckMu.Lock()
	lastHealthCheck = make(map[string]time.Time)
	// 将当前激活订阅的 lastHealthCheck 设为当前时间，避免启动后立即测速
	if activeSub != "" {
		lastHealthCheck[activeSub] = time.Now()
	}
	healthCheckMu.Unlock()

	go func() {
		defer ticker.Stop()
		for {
			select {
			case <-ticker.C:
				performHealthChecks()
			case <-stop:
				return
			}
		}
	}()
}

// stopHealthCheckTimer 停止健康检查定时器（幂等，可重复调用）
func stopHealthCheckTimer() {
	healthCheckLifecycleMu.Lock()
	defer healthCheckLifecycleMu.Unlock()

	if healthCheckTicker == nil && healthCheckStop == nil {
		return // 已停止，避免重复 close 已关闭的 channel
	}
	if healthCheckTicker != nil {
		healthCheckTicker.Stop()
		healthCheckTicker = nil
	}
	if healthCheckStop != nil {
		close(healthCheckStop)
		healthCheckStop = nil
	}

	healthCheckMu.Lock()
	lastHealthCheck = nil
	healthCheckMu.Unlock()
}

// performHealthChecks 执行健康检查（仅在 switch 模式下，对当前激活订阅测速）
func performHealthChecks() {
	config.Mu.RLock()
	cfg := config.Current
	config.Mu.RUnlock()

	if cfg.Mode != "switch" || len(cfg.Subscriptions) == 0 || cfg.ActiveSubscription == "" {
		return
	}

	var activeSub *config.Subscription
	for i := range cfg.Subscriptions {
		if cfg.Subscriptions[i].Name == cfg.ActiveSubscription {
			activeSub = &cfg.Subscriptions[i]
			break
		}
	}
	if activeSub == nil {
		return
	}

	interval := activeSub.HealthInterval
	if interval <= 0 {
		interval = 600
	}

	now := time.Now()
	healthCheckMu.RLock()
	last, ok := lastHealthCheck[activeSub.Name]
	healthCheckMu.RUnlock()
	if ok && now.Sub(last) < time.Duration(interval)*time.Second {
		return
	}

	groups, err := dashapi.GetAllProxyGroups()
	if err != nil {
		log.Printf("[HealthCheck] 获取策略组失败: %v", err)
		return
	}
	if len(groups) == 0 {
		return
	}

	// 并发测速（限制并发数5）
	var wg sync.WaitGroup
	sem := make(chan struct{}, 5)
	for _, g := range groups {
		wg.Add(1)
		go func(name string) {
			defer wg.Done()
			sem <- struct{}{}
			defer func() { <-sem }()
			dashapi.TestGroupDelay(name)
		}(g)
	}
	wg.Wait()

	healthCheckMu.Lock()
	// 定时器可能已在本函数执行期间被 stop（lastHealthCheck 被置 nil）。
	// 向 nil map 写入会 panic，故必须再判一次。
	if lastHealthCheck != nil {
		lastHealthCheck[activeSub.Name] = time.Now()
	}
	healthCheckMu.Unlock()
}
