package rhttp

import (
	"fmt"
	"net"
	"net/http"
	"net/url"
	"sync"
	"time"

	"github.com/bestruirui/octopus/internal/model"
	"github.com/bestruirui/octopus/internal/op"
	"golang.org/x/net/proxy"
)

var (
	directClient   *http.Client // directClient 不经代理的共享客户端。
	proxyClient    *http.Client // proxyClient 按应用设置代理地址构建的共享客户端。
	proxyClientURL string       // proxyClientURL proxyClient 当前所用的代理地址, 设置变更时据此重建。
	clientLock     sync.RWMutex
)

// New 返回按指定代理地址新建的客户端, 不缓存连接池。
// 调用方用完必须 CloseIdleConnections: 否则本次请求留下的空闲连接会随被丢弃的 Transport 残留到 IdleConnTimeout 到期。
// 地址支持 http, https, socks5, socks5h。
func New(proxyURL string) (*http.Client, error) {
	if proxyURL == "" {
		return nil, fmt.Errorf("proxy url is empty")
	}
	return newCustomProxyClient(proxyURL)
}

// Proxy 返回按应用设置(setting key: proxy_url)代理地址构建的共享客户端。
// 设置未填代理地址时报错而不静默直连: 调用方要的就是走代理, 直连需求由 Direct 表达。
func Proxy() (*http.Client, error) {
	currentProxyURL, err := op.SettingGetString(model.SettingKeyProxyURL)
	if err != nil {
		return nil, err
	}
	if currentProxyURL == "" {
		return nil, fmt.Errorf("proxy url is empty")
	}

	clientLock.RLock()
	if proxyClient != nil && proxyClientURL == currentProxyURL {
		clientLock.RUnlock()
		return proxyClient, nil
	}
	clientLock.RUnlock()

	clientLock.Lock()
	defer clientLock.Unlock()

	// 取写锁期间设置可能已被其他协程重建, 故再核对一次地址。
	if proxyClient != nil && proxyClientURL == currentProxyURL {
		return proxyClient, nil
	}

	client, err := newCustomProxyClient(currentProxyURL)
	if err != nil {
		return nil, err
	}
	// 代理地址变更后旧客户端不再被引用, 关掉空闲连接免得连接池残留。
	if proxyClient != nil {
		proxyClient.CloseIdleConnections()
	}
	proxyClient = client
	proxyClientURL = currentProxyURL
	return proxyClient, nil
}

// Direct 返回不经任何代理的共享客户端。
func Direct() (*http.Client, error) {
	clientLock.RLock()
	if directClient != nil {
		clientLock.RUnlock()
		return directClient, nil
	}
	clientLock.RUnlock()

	clientLock.Lock()
	defer clientLock.Unlock()

	if directClient != nil {
		return directClient, nil
	}
	cloned, err := clonedDefaultTransport()
	if err != nil {
		return nil, err
	}
	// 克隆自 http.DefaultTransport 会带上 ProxyFromEnvironment, 直连须显式清掉。
	cloned.Proxy = nil
	directClient = &http.Client{Transport: cloned}
	return directClient, nil
}

func clonedDefaultTransport() (*http.Transport, error) {
	transport, ok := http.DefaultTransport.(*http.Transport)
	if !ok {
		return nil, fmt.Errorf("default transport is not *http.Transport")
	}
	return transport.Clone(), nil
}

func newCustomProxyClient(proxyURLStr string) (*http.Client, error) {
	cloned, err := clonedDefaultTransport()
	if err != nil {
		return nil, err
	}

	proxyURL, err := url.Parse(proxyURLStr)
	if err != nil {
		return nil, fmt.Errorf("invalid proxy url: %w", err)
	}

	switch proxyURL.Scheme {
	case "http", "https":
		cloned.Proxy = http.ProxyURL(proxyURL)
	case "socks5", "socks5h":
		// forward 必须显式给出带 Timeout 的 Dialer: proxy.Direct 用的是零值 net.Dialer, 拨到 SOCKS 服务器这一跳没有超时,
		// 而替换 DialContext 又会覆盖掉 DefaultTransport 自带的 30s 拨号超时。
		socksDialer, err := proxy.FromURL(proxyURL, &net.Dialer{Timeout: 30 * time.Second, KeepAlive: 30 * time.Second})
		if err != nil {
			return nil, fmt.Errorf("invalid socks proxy: %w", err)
		}
		// SOCKS 握手要走 ctx 才能被上层的超时和取消打断; Dialer.Dial 内部把 ctx 写死成 Background 且已被标记弃用。
		contextDialer, ok := socksDialer.(proxy.ContextDialer)
		if !ok {
			return nil, fmt.Errorf("socks proxy dialer does not support context")
		}
		// SOCKS 代理无法通过 Transport.Proxy 表达, 只能改拨号入口。
		cloned.Proxy = nil
		cloned.DialContext = contextDialer.DialContext
	default:
		return nil, fmt.Errorf("unsupported proxy scheme: %s", proxyURL.Scheme)
	}

	return &http.Client{Transport: cloned}, nil
}
