package netinfo

import (
	"context"
	"encoding/json"
	"io"
	"net/http"
	"sync"
	"time"
)

// geoCacheTTL 单条缓存的有效期
const geoCacheTTL = 10 * time.Minute

// geoCacheMaxEntries 缓存条目上限。
//
// 缓存以 IP 为键（查询对象是用户的出口 IP 与代理节点出口 IP），正常使用下
// 条目极少。但键来自外部、且过期条目此前不会被移除，长期运行只增不减；
// 一旦 IP 频繁变化（例如反复切换节点的出口），map 会持续占用内存。
// 这里给定上限，超限时先清理已过期条目，仍然超限则整体重置。
const geoCacheMaxEntries = 256

var geoCache = struct {
	sync.RWMutex
	data map[string]geoInfo
}{data: make(map[string]geoInfo)}

type geoInfo struct {
	Country string
	Region  string
	Isp     string
	Expire  time.Time
}

// geoCacheStore 写入缓存并维持容量上限（调用方需持有写锁）。
//
// 先惰性清理过期条目；若仍达到上限，则整体重置，而不是按插入顺序淘汰——
// 缓存内容可再生（重新查询即可），且本接口的调用频率极低，整体重置足够，
// 无需引入 LRU 的复杂度。
func geoCacheStore(ip string, info geoInfo) {
	if len(geoCache.data) >= geoCacheMaxEntries {
		now := time.Now()
		for k, v := range geoCache.data {
			if now.After(v.Expire) {
				delete(geoCache.data, k)
			}
		}
		if len(geoCache.data) >= geoCacheMaxEntries {
			geoCache.data = make(map[string]geoInfo)
		}
	}
	geoCache.data[ip] = info
}

// fetchGeoInfo 查询 IP 地理信息，返回 country, region, isp（带缓存和重试）。
//
// ctx 用于在客户端断开时立即放弃查询；命中缓存时不产生任何网络连接。
func fetchGeoInfo(ctx context.Context, ip string) (string, string, string) {
	if ip == "" {
		return "", "", ""
	}

	// 1. 检查缓存
	geoCache.RLock()
	if cached, ok := geoCache.data[ip]; ok && time.Now().Before(cached.Expire) {
		geoCache.RUnlock()
		return cached.Country, cached.Region, cached.Isp
	}
	geoCache.RUnlock()

	// 一次性查询，禁用 keep-alive，读完即断开（详见 newLookupClient 说明）
	client := newLookupClient("", 5*time.Second)
	// 使用 ip-api.com 作为主 API（免费版，无 token，但需遵守使用条款）
	// 返回 JSON: {"country":"...", "regionName":"...", "isp":"..."}
	// 注意：ip-api.com 对非商用有限制，但通常可用
	urls := []string{
		"http://ip-api.com/json/" + ip + "?fields=country,regionName,isp",
		"https://api.ip.sb/geoip/" + ip,
	}

	for attempt := 0; attempt < 3; attempt++ {
		if attempt > 0 {
			time.Sleep(300 * time.Millisecond)
		}
		for _, url := range urls {
			req, err := http.NewRequestWithContext(ctx, "GET", url, nil)
			if err != nil {
				continue
			}
			req.Header.Set("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/91.0.4472.124 Safari/537.36")
			resp, err := client.Do(req)
			if err != nil {
				continue
			}
			if resp.StatusCode != http.StatusOK {
				resp.Body.Close()
				continue
			}
			var country, region, isp string
			// 尝试解析 JSON
			var data struct {
				Country    string `json:"country"`
				Region     string `json:"region"`
				RegionName string `json:"regionName"`
				Isp        string `json:"isp"`
			}
			bodyBytes, _ := io.ReadAll(resp.Body)
			resp.Body.Close()
			if err := json.Unmarshal(bodyBytes, &data); err == nil {
				country = data.Country
				if data.RegionName != "" {
					region = data.RegionName
				} else {
					region = data.Region
				}
				isp = data.Isp
			} else {
				// 尝试解析 api.ip.sb 格式
				var data2 struct {
					Country string `json:"country"`
					Region  string `json:"region"`
					Isp     string `json:"isp"`
				}
				if err := json.Unmarshal(bodyBytes, &data2); err == nil {
					country = data2.Country
					region = data2.Region
					isp = data2.Isp
				}
			}
			if country != "" || region != "" || isp != "" {
				// 缓存结果，有效期 10 分钟（写入时顺带维持容量上限）
				geoCache.Lock()
				geoCacheStore(ip, geoInfo{
					Country: country,
					Region:  region,
					Isp:     isp,
					Expire:  time.Now().Add(geoCacheTTL),
				})
				geoCache.Unlock()
				return country, region, isp
			}
		}
	}
	return "", "", ""
}
