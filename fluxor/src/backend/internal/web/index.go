package web

import (
	"fluxor/internal/config"
	"html/template"
	"net/http"
	"strings"
)

// HandleIndex 渲染主页单页应用
func HandleIndex(w http.ResponseWriter, r *http.Request) {
	expected := config.BaseURL
	if expected == "" {
		expected = "/"
	}
	if r.URL.Path != expected && r.URL.Path != expected+"/" {
		if !(expected == "/" && r.URL.Path == "/") {
			http.NotFound(w, r)
			return
		}
	}
	if IndexTmpl == nil {
		http.Error(w, "主页模板未加载", http.StatusInternalServerError)
		return
	}

	// 构造 baseHref（用于 <base> 标签，保证以 / 开头和结尾）
	baseHref := config.BaseURL
	if baseHref == "" {
		baseHref = "/"
	} else {
		if !strings.HasPrefix(baseHref, "/") {
			baseHref = "/" + baseHref
		}
		if !strings.HasSuffix(baseHref, "/") {
			baseHref += "/"
		}
	}
	// rawBase 保留原始值，用于 window.BASE_URL
	rawBase := config.OriginalBaseURL
	// 经 fnOS 网关进来的请求：main 的前缀剥离中间件已把 /app/fluxor 从路径中剥掉，
	// 这里把它补回注入值，浏览器后续子资源与 API 才会继续带前缀走网关。
	if gp := GatewayPrefixFromContext(r.Context()); gp != "" {
		if baseHref == "/" {
			baseHref = gp + "/"
		} else {
			baseHref = strings.TrimSuffix(gp, "/") + baseHref
		}
		rawBase = strings.TrimSuffix(gp, "/") + strings.TrimSuffix(rawBase, "/")
	}

	data := map[string]string{
		"BaseHref": baseHref,
		"RawBase":  rawBase,
	}
	IndexTmpl.Execute(w, data)
}

// IndexTmpl 由 main 在启动时从内嵌文件系统解析的 index.html 模板。
var IndexTmpl *template.Template
