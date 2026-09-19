package appupdate

import (
	"encoding/json"
	"fluxor/internal/buildinfo"
	"fluxor/internal/httpx"
	"net/http"
)

// HandleCheckUpdate 检查 Fluxor 自身是否有新版本。
//
// 当前版本取自编译期注入的 buildinfo.Version，前端无需再通过 ?current= 传入。
// 版本未知（本地未注入时）不以「有新版本」误导用户，而是明确回报可比较状态。
func HandleCheckUpdate(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodGet {
		http.Error(w, "Method Not Allowed", http.StatusMethodNotAllowed)
		return
	}

	current := stripVersionSuffix(buildinfo.Name())

	// 本地未注入版本号（如直接 go run）：无法与远端 tag 做有意义比较，
	// 如实返回 versionUnknown，避免前端弹出「发现新版本」。
	if !buildinfo.IsKnown() {
		w.Header().Set("Content-Type", "application/json")
		json.NewEncoder(w).Encode(map[string]interface{}{
			"hasUpdate":      false,
			"versionUnknown": true,
			"current":        current,
		})
		return
	}

	// 尝试获取完整 Release 信息（含更新日志）
	rel, err := getLatestReleaseInfo()
	if err != nil {
		// 降级方案：仅获取版本号（不返回 releaseNotes）
		latest, err2 := getLatestVersion()
		if err2 != nil {
			httpx.WriteJSONError(w, http.StatusServiceUnavailable, "failed to check update: "+err2.Error())
			return
		}
		hasUpdate := compareVersions(latest, current) > 0
		w.Header().Set("Content-Type", "application/json")
		json.NewEncoder(w).Encode(map[string]interface{}{
			"hasUpdate": hasUpdate,
			"latest":    latest,
			"current":   current,
			// 无 releaseNotes 字段
		})
		return
	}

	hasUpdate := compareVersions(rel.TagName, current) > 0
	w.Header().Set("Content-Type", "application/json")
	json.NewEncoder(w).Encode(map[string]interface{}{
		"hasUpdate":    hasUpdate,
		"latest":       rel.TagName,
		"current":      current,
		"releaseNotes": rel.Body,
	})
}
