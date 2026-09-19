package config

import (
	"sync"
)

var (
	// Current 当前生效的订阅配置快照，读写必须通过 Mu 保护。
	Current SubscribeConfig
	// Mu 保护 Current 的读写锁。
	Mu sync.RWMutex
)
