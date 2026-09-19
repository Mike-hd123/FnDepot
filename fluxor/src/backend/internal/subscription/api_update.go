package subscription

import (
	"fluxor/internal/config"
	"fluxor/internal/core"
	"fluxor/internal/httpx"
	"log"
	"net/http"
	"net/url"
	"strings"
)

// HandleSubscribeUpdate 处理 POST /subscribe/update/{name}
func HandleSubscribeUpdate(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodPost {
		http.Error(w, "Method Not Allowed", http.StatusMethodNotAllowed)
		return
	}

	path := strings.TrimPrefix(r.URL.Path, config.BaseURL+"/subscribe/update/")
	if path == "" {
		httpx.WriteJSONError(w, http.StatusBadRequest, "缺少订阅名称")
		return
	}
	name, err := url.QueryUnescape(path)
	if err != nil {
		httpx.WriteJSONError(w, http.StatusBadRequest, "无效的订阅名称")
		return
	}

	log.Printf("[UPDATE] 收到更新请求: %s", name)

	config.Mu.RLock()
	mode := config.Current.Mode
	config.Mu.RUnlock()

	var targetSub config.Subscription
	var found bool

	if mode == "merge" {
		// 先在主线程快速判断订阅是否存在，保证基本参数合法
		config.Mu.RLock()
		for _, s := range config.Current.Subscriptions {
			if s.Name == name {
				found = true
				break
			}
		}
		config.Mu.RUnlock()

		if !found {
			httpx.WriteJSONError(w, http.StatusNotFound, "未找到该订阅")
			return
		}

		// 启动后台协程异步调用内核更新并拉取元数据，避免阻塞 HTTP 主线程导致 504
		go func(subName string) {
			log.Printf("[ASYNC-UPDATE] 后台启动更新订阅: %s", subName)
			encoded := url.QueryEscape(subName)
			resp, err := core.CoreRequest("PUT", "/providers/proxies/"+encoded, nil)
			if err != nil {
				log.Printf("[ASYNC-UPDATE][ERROR] 调用内核更新失败 %s: %v", subName, err)
				return
			}
			resp.Body.Close()

			// 从主内核拉取最新的元数据
			updatedAt, subInfo, err := fetchSubscriptionMetadataFromCore(subName)
			if err != nil {
				log.Printf("[ASYNC-UPDATE][ERROR] 获取订阅元数据失败 %s: %v", subName, err)
				return
			}

			// 更新内存配置数据
			config.Mu.Lock()
			for i := range config.Current.Subscriptions {
				if config.Current.Subscriptions[i].Name == subName {
					config.Current.Subscriptions[i].UpdatedAt = updatedAt
					config.Current.Subscriptions[i].SubscriptionInfo = subInfo
					break
				}
			}
			config.Mu.Unlock()

			// 持久化保存到 subscribe.json
			if err := config.SaveSubscribeConfig(); err != nil {
				log.Printf("[ASYNC-UPDATE][ERROR] 保存订阅配置失败 %s: %v", subName, err)
			} else {
				log.Printf("[ASYNC-UPDATE] 订阅 %s 后台更新并保存元数据成功", subName)
			}
		}(name)

		// 立即向前端回传 processing 状态
		httpx.RespondJSON(w, http.StatusOK, map[string]string{
			"status":  "processing",
			"message": "订阅更新已在后台启动",
		})
		return
	} else {
		// 切换模式：启动HTTP下载或临时内核下载
		//
		// 注意：此处【不能】持有 config.Mu——updateSubscriptionInSwitchMode 内部
		// 会发起最长数十秒的网络下载，而 config.Mu 被 core.CoreRequest、wsproxy、
		// quality、tproxy 等 10 处读取点共用，持锁下载会把整个面板阻塞住。
		needsReload, err2 := updateSubscriptionInSwitchMode(name)

		// 在锁内读取供响应使用的目标订阅元数据
		config.Mu.RLock()
		for _, s := range config.Current.Subscriptions {
			if s.Name == name {
				targetSub = s
				found = true
				break
			}
		}
		config.Mu.RUnlock()

		if err2 != nil {
			httpx.WriteJSONError(w, http.StatusInternalServerError, "更新失败: "+err2.Error())
			return
		}
		if !found {
			httpx.WriteJSONError(w, http.StatusNotFound, "未找到该订阅")
			return
		}

		// 如果需要重载，在锁外调用
		if needsReload {
			log.Printf("[UPDATE] 开始重载内核")
			if err := core.ReloadCore(); err != nil {
				log.Printf("[UPDATE] 重载内核失败: %v", err)
			}
		}

		// 重置定时器
		StopAllTimers()
		StartAllTimers()

		// 立即持久化（避免统一保存被绕过或失败时前端未知）
		if err := config.SaveSubscribeConfig(); err != nil {
			log.Printf("[UPDATE] 保存订阅配置失败: %v", err)
			httpx.WriteJSONError(w, http.StatusInternalServerError, "保存配置失败: "+err.Error())
			return
		}
	}

	var info interface{}
	if targetSub.SubscriptionInfo != nil {
		info = map[string]interface{}{
			"upload":    targetSub.SubscriptionInfo["upload"],
			"download":  targetSub.SubscriptionInfo["download"],
			"total":     targetSub.SubscriptionInfo["total"],
			"expire":    targetSub.SubscriptionInfo["expire"],
			"updatedAt": targetSub.UpdatedAt,
		}
	}

	httpx.RespondJSON(w, http.StatusOK, map[string]interface{}{
		"status":  "ok",
		"message": "订阅 " + name + " 更新成功",
		"info":    info,
	})
}
