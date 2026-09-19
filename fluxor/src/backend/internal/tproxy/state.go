package tproxy

import (
	"sync"
)

var (
	tproxyEnableState        bool
	tproxyMu                 sync.RWMutex
	exceptionsMu             sync.RWMutex
	tproxyProxyLocal         bool
	tproxyDstExceptionsCache []string
	tproxySrcExceptionsCache []string
)

// GetTproxyState 读取 TProxy 开关状态（并发安全）。
//
// 所有读取方都必须经此函数，不要直接访问 tproxyEnableState：
// 该变量由 SetTproxyEnabled / HandleTproxyState 在 tproxyMu 保护下写入，
// 无锁读取会构成数据竞争。
func GetTproxyState() bool {
	tproxyMu.RLock()
	defer tproxyMu.RUnlock()
	return tproxyEnableState
}
