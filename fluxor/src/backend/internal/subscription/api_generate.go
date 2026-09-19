package subscription

import (
	"encoding/json"
	"fluxor/internal/config"
	"fluxor/internal/configgen"
	"fluxor/internal/core"
	"fluxor/internal/httpx"
	"log"
	"net/http"
	"os"
	"path/filepath"
)

// HandleGenerateConfig 处理 POST /subscribe/generate：
// 保存配置、按当前模式生成或复制 config.yaml，并热重载内核。
func HandleGenerateConfig(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodPost {
		http.Error(w, "Method Not Allowed", http.StatusMethodNotAllowed)
		return
	}
	var cfg config.SubscribeConfig
	if err := json.NewDecoder(r.Body).Decode(&cfg); err != nil {
		httpx.WriteJSONError(w, http.StatusBadRequest, "无效的请求格式: "+err.Error())
		return
	}

	if cfg.MetaBackendURL != "" && !httpx.BackendURLRegex.MatchString(cfg.MetaBackendURL) {
		httpx.WriteJSONError(w, http.StatusBadRequest, "外部面板后端地址格式不正确")
		return
	}

	// 订阅名会用作节点文件名与 provider 键，必须在此拦截非法字符
	if err := config.ValidateSubscriptionNames(cfg.Subscriptions); err != nil {
		httpx.WriteJSONError(w, http.StatusBadRequest, err.Error())
		return
	}

	// 物理清理标记删除的配置文件，使用 filepath.Base 防范路径穿越
	if len(cfg.DeletePhysical) > 0 {
		for _, name := range cfg.DeletePhysical {
			// 与写入侧使用同一套文件名规则，避免删错文件或漏删
			fileName := config.SanitizeSubscriptionFileName(name)
			if fileName == ".yaml" {
				continue
			}
			targetFile := filepath.Join(config.CoreWorkDir, "proxies", fileName)
			if _, err := os.Stat(targetFile); err == nil {
				if err := os.Remove(targetFile); err != nil {
					log.Printf("[DELETE] 物理删除配置文件失败 %s: %v", targetFile, err)
				} else {
					log.Printf("[DELETE] 成功物理删除配置文件: %s", targetFile)
				}
			}
		}
	}
	cfg.DeletePhysical = nil // 清空临时字段避免持久化

	// 请求体与当前生效配置做增量合并，而不是整份替换。
	//
	// 此接口由前端「保存并应用」发起，也常被直接以 POST /subscribe/generate
	// -d '{}' 触发（用户想看内核重新跑一次）。若把请求体当作全量配置，一个
	// 空 body 就会把内存与磁盘上的订阅列表、端口、规则集全部清空并持久化——
	// 随后 GenerateConfig 收到 0 个订阅，退化生成只有 8 行、端口全 0 的空壳
	// config.yaml，内核热重载后无任何代理可用（1.4.0-1 的现网故障根因）。
	// 前端表单提交的字段一律非零/非空，因此合并后行为不变。
	cfg = mergeSubscribeConfig(cfg)

	// 端口、规则集、面板兜底：上面合并若仍未取得有效值（例如磁盘配置
	// 已被清空后重启），退回出厂默认，避免生成端口全 0 的不可用配置。
	cfg = cfg.WithDefaults()

	// active_subscription 以订阅名为键。为空但订阅列表非空时默认取第一个：
	// 用户装完不点「保存」就重启（或订阅在别处导入）后，此字段一直是 ""，
	// 冷启动又不会触发 generate，导致面板显示「运行中」实际是空壳。
	// 这里兜底让 generate 始终能合入真实节点，无需用户手动再点一次保存。
	if cfg.ActiveSubscription == "" && len(cfg.Subscriptions) > 0 {
		cfg.ActiveSubscription = cfg.Subscriptions[0].Name
		log.Printf("[generate] active_subscription 为空，默认选用首个订阅 %q", cfg.ActiveSubscription)
	}

	// 切换模式
	if cfg.Mode == "switch" {
		// 如果订阅列表为空，生成基础配置，清除选中状态，保存并重载
		if len(cfg.Subscriptions) == 0 {
			// 生成基础配置文件
			if err := configgen.GenerateBaseConfig(cfg); err != nil {
				httpx.WriteJSONError(w, http.StatusInternalServerError, "生成基础配置失败: "+err.Error())
				return
			}
			// 清除选中的订阅
			cfg.ActiveSubscription = ""
			// 保存配置到全局并持久化
			config.Mu.Lock()
			config.Current = cfg
			config.Mu.Unlock()
			if err := config.SaveSubscribeConfig(); err != nil {
				log.Printf("保存订阅配置失败: %v", err)
			}
			// 重置定时器（无订阅时需停止所有定时器）
			StopAllTimers()
			StartAllTimers() // 会检查模式，切换模式且无订阅时会跳过启动
			// 重载内核
			if err := core.ReloadCore(); err != nil {
				httpx.RespondJSON(w, http.StatusOK, map[string]string{
					"status":  "warning",
					"message": "基础配置已生成，但重载内核失败: " + err.Error(),
				})
				return
			}
			httpx.RespondJSON(w, http.StatusOK, map[string]string{
				"status":  "ok",
				"message": "已清除订阅，切换到基础配置",
			})
			return
		}

		// 有订阅时，确保所有订阅文件已下载
		if err := ensureSubscriptionFiles(&cfg); err != nil {
			httpx.WriteJSONError(w, http.StatusInternalServerError, "下载订阅文件失败: "+err.Error())
			return
		}
		// 检查是否选中了订阅
		if cfg.ActiveSubscription == "" {
			httpx.WriteJSONError(w, http.StatusBadRequest, "切换模式下请先选择一个订阅")
			return
		}
		// 选中名必须是当前订阅列表中的真实成员。
		// active_subscription 以订阅名为键，改名/删除后客户端可能仍持有旧名；
		// 只校验空字符串会让旧名一路走到下面，按旧名复制旧订阅文件，
		// 出现「界面显示新名字、实际生效旧配置」的静默错配。
		if !activeSubscriptionExists(cfg) {
			httpx.WriteJSONError(w, http.StatusBadRequest, "选中的订阅不存在: "+cfg.ActiveSubscription)
			return
		}
		// 构建源文件路径
		srcFile := filepath.Join(config.CoreWorkDir, "proxies", config.SanitizeSubscriptionFileName(cfg.ActiveSubscription))
		if _, err := os.Stat(srcFile); err != nil {
			httpx.WriteJSONError(w, http.StatusInternalServerError, "选中的订阅文件不存在: "+err.Error())
			return
		}
		// 复制文件到 configTarget
		if err := copyFile(srcFile, config.ConfigTarget); err != nil {
			httpx.WriteJSONError(w, http.StatusInternalServerError, "复制配置文件失败: "+err.Error())
			return
		}
		// 保存配置到 subscribe.json
		config.Mu.Lock()
		config.Current = cfg
		config.Mu.Unlock()
		if err := config.SaveSubscribeConfig(); err != nil {
			log.Printf("保存订阅配置失败: %v", err)
		}
		// 重置定时器
		StopAllTimers()
		StartAllTimers()
		// 重载内核
		if err := core.ReloadCore(); err != nil {
			httpx.RespondJSON(w, http.StatusOK, map[string]string{
				"status":  "warning",
				"message": "配置文件已复制，但重载内核失败: " + err.Error(),
			})
			return
		}
		httpx.RespondJSON(w, http.StatusOK, map[string]string{
			"status":  "ok",
			"message": "已切换到订阅 " + cfg.ActiveSubscription + " 的配置",
		})
		return
	}

	// ---------- 融合模式（原有逻辑） ----------
	// 不再预先删除旧 config.yaml：GenerateConfig 会在同一路径上直接生成并覆盖
	// （writeConfigTarget 用 os.WriteFile 整份覆写，且从不读取旧文件）。
	// 预删除只会留出一个「文件不存在」的窗口：若随后生成失败，内核热重载或
	// 重启就会因缺少配置而失败——此前的实现正是如此。

	config.Mu.Lock()
	config.Current = cfg
	config.Mu.Unlock()
	if err := config.SaveSubscribeConfig(); err != nil {
		httpx.WriteJSONError(w, http.StatusInternalServerError, "保存配置失败: "+err.Error())
		return
	}
	// 重置定时器
	StopAllTimers()
	StartAllTimers()

	if err := configgen.GenerateConfig(cfg); err != nil {
		httpx.WriteJSONError(w, http.StatusInternalServerError, "生成配置文件失败: "+err.Error())
		return
	}

	if cfg.MetaBackendURL != "" {
		if err := modifyMetaConfig(cfg.MetaBackendURL); err != nil {
			log.Printf("[WARN] 修改 MetaCubeXD 后端地址失败: %v", err)
		}
	}

	if err := core.ReloadCore(); err != nil {
		httpx.RespondJSON(w, http.StatusOK, map[string]string{
			"status":  "warning",
			"message": "配置文件已生成，但重载内核失败: " + err.Error(),
		})
		return
	}

	updateAllSubscriptionsMetadata(&cfg)
	// 将更新后的 cfg 保存到全局并持久化
	config.Mu.Lock()
	config.Current = cfg
	config.Mu.Unlock()
	if err := config.SaveSubscribeConfig(); err != nil {
		log.Printf("保存订阅配置失败: %v", err)
	}

	httpx.RespondJSON(w, http.StatusOK, map[string]string{
		"status":  "ok",
		"message": "配置文件已生成并成功重载内核",
	})
}

// mergeSubscribeConfig 把请求体 in 增量合并到当前生效配置上，返回合并结果。
//
// 规则：in 的标量字段为零值（0 / ""）时视为「本次未提交」，沿用 config.Current
// 的同名值；非零值则采用 in 的值。Subscriptions 是列表型字段，例外地用 in 全量
// 替换（前端始终提交完整列表；空列表代表用户确实删光了订阅，不能被旧值顶回）。
//
// 必要性：HandleGenerateConfig 的调用方既有前端「保存并应用」，也有运维直接用
// POST /subscribe/generate -d '{}' 让内核重跑一次。若把请求体当全量配置，空 body
// 会把订阅列表、端口、规则集整份清空并持久化，随后 GenerateConfig 收到 0 个订阅
// 退化成 8 行端口全 0 的空壳 config.yaml（1.4.0-1 现网故障根因）。
func mergeSubscribeConfig(in config.SubscribeConfig) config.SubscribeConfig {
	config.Mu.RLock()
	base := config.Current
	config.Mu.RUnlock()

	if in.ProxyPort == 0 {
		in.ProxyPort = base.ProxyPort
	}
	if in.PanelPort == 0 {
		in.PanelPort = base.PanelPort
	}
	if in.TproxyPort == 0 {
		in.TproxyPort = base.TproxyPort
	}
	if in.RuleGroup == "" {
		in.RuleGroup = base.RuleGroup
	}
	if in.UIPanel == "" {
		in.UIPanel = base.UIPanel
	}
	if in.MetaBackendURL == "" {
		in.MetaBackendURL = base.MetaBackendURL
	}
	if in.PanelSecret == "" {
		in.PanelSecret = base.PanelSecret
	}
	if in.Mode == "" {
		in.Mode = base.Mode
	}
	// ActiveSubscription / Subscriptions 是身份字段，请求体未提交（空串 / nil）
	// 时必须沿用内存态。注意 nil 切片无法与「用户确实删光了订阅」区分——
	// 但前端删除订阅时会提交 []Subscription{}（非 nil 空切片）并带 delete_physical，
	// 只有裸 {} 才会传 nil，因此 nil 一律按「未提交」处理。
	if in.ActiveSubscription == "" {
		in.ActiveSubscription = base.ActiveSubscription
	}
	if in.Subscriptions == nil {
		in.Subscriptions = base.Subscriptions
	}
	return in
}
