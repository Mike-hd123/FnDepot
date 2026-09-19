package web

import "context"

type gatewayPrefixKey struct{}

// WithGatewayPrefix 由 main 的前缀剥离中间件写入：记录本次请求被剥掉的
// fnOS 网关前缀（如 /app/fluxor），供 HandleIndex 注入正确的 <base href>
// 与 window.BASE_URL。不带前缀（TCP 直连）的请求不会设置该值。
func WithGatewayPrefix(ctx context.Context, prefix string) context.Context {
	return context.WithValue(ctx, gatewayPrefixKey{}, prefix)
}

// GatewayPrefixFromContext 读取网关前缀；不存在返回空串。
func GatewayPrefixFromContext(ctx context.Context) string {
	if v, ok := ctx.Value(gatewayPrefixKey{}).(string); ok {
		return v
	}
	return ""
}
