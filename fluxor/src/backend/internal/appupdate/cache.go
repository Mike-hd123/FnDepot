package appupdate

import (
	"sync"
	"time"
)

var (
	latestVersionCache       string
	latestVersionCacheTime   time.Time
	cacheMutex               sync.RWMutex
	cacheTTL                 = 10 * time.Minute
	latestReleaseCache       *githubRelease
	latestReleaseCacheTime   time.Time
	releaseCacheMutex        sync.RWMutex
	latestAlphaHashCache     string
	latestAlphaHashCacheTime time.Time
	alphaCacheMutex          sync.RWMutex
	alphaCacheTTL            = 10 * time.Minute
)
