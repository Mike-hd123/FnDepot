package delaytest

import (
	"context"
	"fluxor/internal/netinfo"
	"fmt"
	"net/http"
	"net/url"
	"time"
)

// testDelayThroughProxy 通过代理测试目标URL的延迟（HEAD请求），返回毫秒。
//
// ctx 为发起请求的 HTTP 请求上下文：客户端断开时立即放弃探测，
// 避免请求已无人接收却仍占用 goroutine 与代理连接。
func testDelayThroughProxy(ctx context.Context, targetURL string, timeout time.Duration) (int, error) {
	proxyPort := netinfo.GetProxyPortFromConfig()
	if proxyPort == 0 {
		return 0, fmt.Errorf("no proxy port available")
	}
	proxyAddr := fmt.Sprintf("http://127.0.0.1:%d", proxyPort)
	proxyURL, err := url.Parse(proxyAddr)
	if err != nil {
		return 0, err
	}
	transport := &http.Transport{Proxy: http.ProxyURL(proxyURL)}
	client := &http.Client{
		Transport: transport,
		Timeout:   timeout,
		// 禁止重定向，测速只需要首包响应即可
		CheckRedirect: func(req *http.Request, via []*http.Request) error {
			return http.ErrUseLastResponse
		},
	}
	req, err := http.NewRequestWithContext(ctx, "HEAD", targetURL, nil)
	if err != nil {
		return 0, err
	}
	// 强制通知代理与源站发送完报头后立即关闭连接，防止因无结束标记挂起超时
	req.Close = true
	// 附带标准的浏览器 UA，防止被防火墙或 WAF 丢包防爬拦截导致误报超时
	req.Header.Set("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36")

	start := time.Now()
	resp, err := client.Do(req)
	if err != nil {
		return 0, err
	}
	defer resp.Body.Close()
	// 即使状态码不是200，也认为连接成功，只要建立连接即可
	elapsed := time.Since(start).Milliseconds()
	return int(elapsed), nil
}
