package download

import (
	"fluxor/internal/config"
	"fmt"
	"io"
	"net/http"
	"os"
	"strconv"
	"strings"
	"time"
)

// directDownloadTimeout 直连下载的单次超时。
//
// 下载链路为「直连 → 失败则回退临时内核」，两阶段各以 15 秒为上限，
// 且任一阶段都不做内部重试：超时或失败即如实报错，由用户决定是否再次手动更新。
const directDownloadTimeout = 15 * time.Second

// tryDirectDownload 尝试直接 HTTP 下载订阅。
//
// 仅接受原样即为 Clash 明文 YAML（含 proxies / proxy-providers / proxy-groups）的响应。
// 其它形态（Base64 编码的 URI 列表、裸 URI 列表、Base64 包裹的 YAML、age 加密等）一律
// 返回错误，由调用方回退到临时内核——解码与格式识别属于内核能力，Fluxor 不再自行实现。
func tryDirectDownload(sub config.Subscription, targetFile string) (updatedAt string, subInfo map[string]interface{}, err error) {
	client := &http.Client{
		Timeout: directDownloadTimeout,
		CheckRedirect: func(req *http.Request, via []*http.Request) error {
			if len(via) >= 10 {
				return fmt.Errorf("too many redirects")
			}
			return nil
		},
	}

	req, err := http.NewRequest("GET", sub.URL, nil)
	if err != nil {
		return "", nil, err
	}
	req.Header.Set("User-Agent", "clash.meta")
	req.Header.Set("Accept", "text/plain, application/json, */*")

	resp, err := client.Do(req)
	if err != nil {
		return "", nil, err
	}
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusOK {
		return "", nil, fmt.Errorf("HTTP 状态码: %d", resp.StatusCode)
	}

	// 读取响应体
	bodyBytes, err := io.ReadAll(resp.Body)
	if err != nil {
		return "", nil, err
	}

	// 校验是否为可被内核加载的 Clash 配置；不是则交由临时内核处理
	content := string(bodyBytes)
	if err := validateSubscriptionContent(content); err != nil {
		return "", nil, fmt.Errorf("非 Clash YAML 订阅内容，交由临时内核处理: %w", err)
	}

	// 写入文件
	if err := os.WriteFile(targetFile, []byte(content), 0644); err != nil {
		return "", nil, err
	}

	// 解析 subscription-userinfo 头
	subInfo = parseSubscriptionUserinfo(resp.Header.Get("subscription-userinfo"))
	updatedAt = time.Now().Format(time.RFC3339)

	return updatedAt, subInfo, nil
}

// parseSubscriptionUserinfo 解析 subscription-userinfo 头
func parseSubscriptionUserinfo(header string) map[string]interface{} {
	result := make(map[string]interface{})
	if header == "" {
		return result
	}
	parts := strings.Split(header, ";")
	for _, part := range parts {
		part = strings.TrimSpace(part)
		kv := strings.SplitN(part, "=", 2)
		if len(kv) != 2 {
			continue
		}
		key := strings.TrimSpace(kv[0])
		val := strings.TrimSpace(kv[1])
		switch key {
		case "upload", "download", "total":
			if v, err := strconv.ParseInt(val, 10, 64); err == nil {
				result[key] = v
			}
		case "expire":
			if v, err := strconv.ParseInt(val, 10, 64); err == nil {
				result[key] = v
			}
		default:
			result[key] = val
		}
	}
	return result
}
