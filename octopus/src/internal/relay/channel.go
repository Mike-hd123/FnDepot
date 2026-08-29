package relay

import (
	"encoding/json"
	"fmt"
	"slices"
	"strings"

	"github.com/bestruirui/octopus/internal/model"
	"github.com/looplj/axonhub/llm"
	"github.com/looplj/axonhub/llm/auth"
	"github.com/looplj/axonhub/llm/httpclient"
	"github.com/looplj/axonhub/llm/transformer"
	"github.com/looplj/axonhub/llm/transformer/anthropic"
	"github.com/looplj/axonhub/llm/transformer/doubao"
	"github.com/looplj/axonhub/llm/transformer/gemini"
	"github.com/looplj/axonhub/llm/transformer/openai"
	"github.com/looplj/axonhub/llm/transformer/openai/responses"
	"github.com/tidwall/sjson"
)
// buildOutbound 按渠道协议构造出站转换器, 并判断客户端请求能否直接透传。
func buildOutbound(channel model.Channel, format llm.APIFormat) (transformer.Outbound, bool, error) {
	key := auth.NewStaticKeyProvider(channel.Key)
	switch channel.Type {
	case model.ChannelProviderOpenAI:
		outbound, err := openai.NewOutboundTransformerWithConfig(&openai.Config{PlatformType: openai.PlatformOpenAI, BaseURL: channel.BaseURL, APIKeyProvider: key})
		return outbound, format == llm.APIFormatOpenAIChatCompletion, err
	case model.ChannelProviderOpenAIResponses:
		outbound, err := responses.NewOutboundTransformerWithConfig(&responses.Config{BaseURL: channel.BaseURL, APIKeyProvider: key})
		return outbound, format == llm.APIFormatOpenAIResponse, err
	case model.ChannelProviderAnthropic:
		outbound, err := anthropic.NewOutboundTransformerWithConfig(&anthropic.Config{Type: anthropic.PlatformDirect, BaseURL: channel.BaseURL, APIKeyProvider: key})
		return outbound, format == llm.APIFormatAnthropicMessage, err
	case model.ChannelProviderGemini:
		outbound, err := gemini.NewOutboundTransformerWithConfig(gemini.Config{BaseURL: channel.BaseURL, APIKeyProvider: key})
		return outbound, false, err
	case model.ChannelProviderVolcengine:
		outbound, err := doubao.NewOutboundTransformerWithConfig(&doubao.Config{BaseURL: channel.BaseURL, APIKeyProvider: key})
		return outbound, false, err
	default:
		return nil, false, fmt.Errorf("unsupported channel provider: %s", channel.Type)
	}
}

// applyChannelConfig 按渠道配置覆盖上游请求的参数并追加自定义 Header; model 与 stream 由转发流程决定, 不允许覆盖。
func applyChannelConfig(channel model.Channel, request *httpclient.Request) error {
	if channel.ParamOverride != nil && *channel.ParamOverride != "" {
		var overrides map[string]json.RawMessage
		if err := json.Unmarshal([]byte(*channel.ParamOverride), &overrides); err != nil {
			return fmt.Errorf("invalid channel parameter override: %w", err)
		}
		body := request.Body
		// 覆盖键可能自带点号或冒号, 转义后再作为 sjson 路径使用, 避免被解析成嵌套路径。
		escape := strings.NewReplacer("\\", "\\\\", ".", "\\.", ":", "\\:")
		for key, value := range overrides {
			if key == "model" || key == "stream" {
				continue
			}
			next, err := sjson.SetRawBytes(body, ":"+escape.Replace(key), value)
			if err != nil {
				return fmt.Errorf("apply channel parameter %q: %w", key, err)
			}
			body = next
		}
		request.Body = body
		if len(request.JSONBody) > 0 {
			request.JSONBody = slices.Clone(body)
		}
	}

	// 转换器已经写入的认证等敏感 Header 不允许被自定义配置覆盖。
	for _, header := range channel.CustomHeader {
		if request.Headers.Get(header.HeaderKey) != "" && httpclient.IsSensitiveHeader(header.HeaderKey) {
			continue
		}
		request.Headers.Set(header.HeaderKey, header.HeaderValue)
	}
	return nil
}
