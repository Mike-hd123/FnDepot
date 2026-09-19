package netinfo

import (
	"context"
	"encoding/json"
	"fmt"
	"io"
	"net"
	"net/http"
	"net/url"
	"regexp"
	"strings"
	"time"
)

// newLookupClient 构造用于「一次性外部查询」的 HTTP 客户端。
//
// 统一禁用 keep-alive：响应读完后连接立即断开。
//
// 原因：这些查询是低频的一次性操作（打开概览页才触发），保留空闲连接毫无收益，
// 却会让连接长期停留在 ESTABLISHED —— 未指定 Transport 时会回落到
// http.DefaultTransport，其 IdleConnTimeout 达 90 秒。表现就是查询早已结束，
// 而面板「连接」页里那条连接一直挂着。
func newLookupClient(proxyAddr string, timeout time.Duration) *http.Client {
	transport := &http.Transport{DisableKeepAlives: true}
	if proxyAddr != "" {
		if proxyURL, err := url.Parse(proxyAddr); err == nil {
			transport.Proxy = http.ProxyURL(proxyURL)
		}
	}
	return &http.Client{Transport: transport, Timeout: timeout}
}

// fetchPublicIP 支持通过代理获取 IP，兼容 JSON 与纯文本，带正则表达式提取和校验。
//
// ctx 用于在客户端断开时立即放弃查询，避免无人接收却仍占用连接与 goroutine。
func fetchPublicIP(ctx context.Context, apiURL, proxyAddr string) (string, error) {
	client := newLookupClient(proxyAddr, 5*time.Second)
	req, err := http.NewRequestWithContext(ctx, "GET", apiURL, nil)
	if err != nil {
		return "", err
	}
	resp, err := client.Do(req)
	if err != nil {
		return "", err
	}
	defer resp.Body.Close()
	if resp.StatusCode != http.StatusOK {
		return "", fmt.Errorf("HTTP %d", resp.StatusCode)
	}

	bodyBytes, err := io.ReadAll(resp.Body)
	if err != nil {
		return "", err
	}

	bodyStr := strings.TrimSpace(string(bodyBytes))

	// 1. 尝试解析为 JSON
	var data struct {
		IP    string `json:"ip"`
		Query string `json:"query"`
	}
	if err := json.Unmarshal(bodyBytes, &data); err == nil {
		if data.IP != "" {
			return data.IP, nil
		}
		if data.Query != "" {
			return data.Query, nil
		}
	}

	// 2. 正则从响应内容中搜索首个合法的 IPv4/IPv6 地址并验证
	ipRegex := regexp.MustCompile(`((?:[0-9]{1,3}\.){3}[0-9]{1,3})|((?:[0-9a-fA-F]{1,4}:){7}[0-9a-fA-F]{1,4})`)
	matches := ipRegex.FindAllString(bodyStr, -1)
	for _, match := range matches {
		if net.ParseIP(match) != nil {
			return match, nil
		}
	}

	// 3. 直接验证去除空白后的全文
	if net.ParseIP(bodyStr) != nil {
		return bodyStr, nil
	}

	return "", fmt.Errorf("no valid IP address found in response: %s", bodyStr)
}

// fetchPublicIPWithFallback 依次尝试一组 URL，返回首个成功获取到的 IP 地址
func fetchPublicIPWithFallback(ctx context.Context, urls []string, proxyAddr string) (string, error) {
	var lastErr error
	for _, u := range urls {
		ip, err := fetchPublicIP(ctx, u, proxyAddr)
		if err == nil && ip != "" {
			return ip, nil
		}
		lastErr = err
	}
	if lastErr != nil {
		return "", lastErr
	}
	return "", fmt.Errorf("URL 列表为空")
}
