package helper

import (
	"net/http"
	"strings"

	"github.com/bestruirui/octopus/internal/client"
	"github.com/bestruirui/octopus/internal/model"
)

// ChannelHttpClient 根据渠道代理配置创建 HTTP 客户端。
func ChannelHttpClient(channel *model.Channel) (*http.Client, error) {
	if !channel.Proxy {
		return client.GetHTTPClientSystemProxy(false)
	}
	if channel.ChannelProxy == nil || strings.TrimSpace(*channel.ChannelProxy) == "" {
		return client.GetHTTPClientSystemProxy(true)
	}
	return client.GetHTTPClientCustomProxy(strings.TrimSpace(*channel.ChannelProxy))
}
