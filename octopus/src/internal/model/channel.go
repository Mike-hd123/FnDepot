package model

// 渠道使用的上游服务提供方。
type ChannelProvider string

const (
	ChannelProviderOpenAI          ChannelProvider = "openai"
	ChannelProviderOpenAIResponses ChannelProvider = "openai_responses"
	ChannelProviderAnthropic       ChannelProvider = "anthropic"
	ChannelProviderGemini          ChannelProvider = "gemini"
	ChannelProviderVolcengine      ChannelProvider = "volcengine"
)

// 渠道模型的来源类型。
type ChannelModelSource string

const (
	ChannelModelSourceAuto   ChannelModelSource = "auto"   // 通过上游接口自动获取。
	ChannelModelSourceManual ChannelModelSource = "manual" // 管理员手动配置。
)

// 单个上游渠道的连接和转发配置。
type Channel struct {
	ID            int             `json:"id" gorm:"primaryKey"`                                                       // 渠道主键。
	Name          string          `json:"name" gorm:"unique;not null"`                                                // 渠道名称。
	Type          ChannelProvider `json:"type"`                                                                       // 上游服务提供方。
	Enabled       bool            `json:"enabled" gorm:"default:true"`                                                // 渠道是否可用。
	BaseURL       string          `json:"base_url"`                                                                    // 唯一的上游基础地址。
	Key           string          `json:"key"`                                                                         // 唯一的上游访问凭据。
	Models        []ChannelModel  `json:"models,omitempty" gorm:"foreignKey:ChannelID;constraint:OnDelete:CASCADE"` // 渠道提供的模型。
	Proxy         bool            `json:"proxy" gorm:"default:false"`                                                 // 是否使用代理。
	AutoSync      bool            `json:"auto_sync" gorm:"default:false"`                                            // 是否自动同步模型。
	CustomHeader  []CustomHeader  `json:"custom_header" gorm:"serializer:json"`                                      // 追加到上游请求的 Header。
	ParamOverride *string         `json:"param_override"`                                                             // 请求参数覆盖配置。
	ChannelProxy  *string         `json:"channel_proxy"`                                                              // 渠道专用代理地址。
	MatchRegex    *string         `json:"match_regex"`                                                                // 模型同步过滤表达式。
	StatsMetrics                                                                                                        // 渠道累计统计信息。
}

// 渠道提供的单个上游模型。
type ChannelModel struct {
	ID        int                `json:"id" gorm:"primaryKey"`                                            // 渠道模型主键。
	ChannelID int                `json:"channel_id" gorm:"not null;index:idx_channel_model_name,unique"` // 所属渠道 ID。
	Name      string             `json:"name" gorm:"not null;index:idx_channel_model_name,unique"`       // 上游模型名称。
	Source    ChannelModelSource `json:"source" gorm:"not null;default:auto"`                             // 模型来源。
	StatsMetrics                                                                                              // 渠道模型统计信息。
}

// 追加到上游请求的单个 Header。
type CustomHeader struct {
	HeaderKey   string `json:"header_key"`   // Header 名称。
	HeaderValue string `json:"header_value"` // Header 值。
}

// ChannelUpdateRequest 渠道更新请求 - 仅包含变更的数据。
type ChannelUpdateRequest struct {
	ID            int              `json:"id" binding:"required"`    // 待更新渠道的主键。
	Name          *string          `json:"name,omitempty"`           // 新的渠道名称。
	Type          *ChannelProvider `json:"type,omitempty"`           // 新的上游服务提供方。
	Enabled       *bool            `json:"enabled,omitempty"`        // 新的启用状态。
	BaseURL       *string          `json:"base_url,omitempty"`       // 新的上游基础地址。
	Key           *string          `json:"key,omitempty"`            // 新的上游访问凭据。
	Models        *[]ChannelModel  `json:"models,omitempty"`         // 新的渠道模型集合。
	Proxy         *bool            `json:"proxy,omitempty"`          // 新的代理开关。
	AutoSync      *bool            `json:"auto_sync,omitempty"`      // 新的自动同步开关。
	CustomHeader  *[]CustomHeader  `json:"custom_header,omitempty"`  // 新的自定义 Header。
	ChannelProxy  *string          `json:"channel_proxy,omitempty"`  // 新的渠道代理地址。
	ParamOverride *string          `json:"param_override,omitempty"` // 新的参数覆盖配置。
	MatchRegex    *string          `json:"match_regex,omitempty"`    // 新的模型过滤表达式。
}
