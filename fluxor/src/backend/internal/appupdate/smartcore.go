package appupdate

// smartcore.go — Smart 内核（vernesong/mihomo alpha-smart）专用升级通道。
//
// 背景（fork 改造红线）：内核自带的 POST /upgrade 会去 MetaCubeX 官方源拉
// 裸 alpha 覆盖本地文件，导致 smart 被静默降级回裸 alpha。本文件提供
// POST /core/smart-update：
//  1. 读本地内核版本（/version via unix socket），非 smart 内核直接 400；
//  2. 从 vernesong/mihomo Prerelease-Alpha release 选
//     mihomo-linux-<arch>-alpha-smart-<hash>.gz 资产；
//  3. 下载 → gzip 解压 → 临时文件 → 备份旧内核 → 原子 rename 替换；
//  4. StopCore + StartCore 换进程（smart 内核启动时自行加载/下载
//     LightGBM Model.bin，无需面板干预）。

import (
	"compress/gzip"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"os"
	"os/exec"
	"path/filepath"
	"regexp"
	"runtime"
	"strings"
	"time"

	"fluxor/internal/config"
	"fluxor/internal/core"
	"fluxor/internal/httpx"
	"fluxor/internal/netinfo"
)

const smartCoreReleaseURL = "https://api.github.com/repos/vernesong/mihomo/releases/tags/Prerelease-Alpha"

type smartReleaseAsset struct {
	Name               string `json:"name"`
	BrowserDownloadURL string `json:"browser_download_url"`
}

type smartRelease struct {
	Assets []smartReleaseAsset `json:"assets"`
}

var smartReleaseCache struct {
	data      *smartRelease
	fetchedAt time.Time
}

// fetchSmartRelease 拉取 vernesong Prerelease-Alpha release（带 10 分钟缓存）。
// API 请求失败时依次尝试 gh-proxy 加速镜像（release API 响应 JSON 不变，仅换主机）。
func fetchSmartRelease() (*smartRelease, error) {
	if smartReleaseCache.data != nil && time.Since(smartReleaseCache.fetchedAt) < 10*time.Minute {
		return smartReleaseCache.data, nil
	}
	var lastErr error
	for _, host := range []string{"", "https://gh-proxy.org/", "https://gh-proxy.com/"} {
		apiURL := host + smartCoreReleaseURL
		req, err := http.NewRequest(http.MethodGet, apiURL, nil)
		if err != nil {
			return nil, err
		}
		req.Header.Set("Accept", "application/vnd.github+json")
		resp, err := http.DefaultClient.Do(req)
		if err != nil {
			lastErr = err
			continue
		}
		if resp.StatusCode != http.StatusOK {
			resp.Body.Close()
			lastErr = fmt.Errorf("GitHub API 返回 %d", resp.StatusCode)
			continue
		}
		var rel smartRelease
		if err := json.NewDecoder(resp.Body).Decode(&rel); err != nil {
			resp.Body.Close()
			lastErr = err
			continue
		}
		resp.Body.Close()
		if len(rel.Assets) == 0 {
			lastErr = fmt.Errorf("release 无资产（镜像可能不支持 API 转发）")
			continue
		}
		smartReleaseCache.data = &rel
		smartReleaseCache.fetchedAt = time.Now()
		return &rel, nil
	}
	return nil, fmt.Errorf("获取 release 失败: %w", lastErr)
}

// smartAssetFor 精确匹配主 arch 的 smart gz 资产。
// 排除 compatible / v1 / v2 / v3 / go12x 等变体。
func smartAssetFor(rel *smartRelease, arch string) (*smartReleaseAsset, string, error) {
	// 例：^mihomo-linux-amd64-alpha-smart-([0-9a-f]{7,40})\.gz$
	re := regexp.MustCompile(`^mihomo-linux-` + regexp.QuoteMeta(arch) + `-alpha-smart-([0-9a-f]+)\.gz$`)
	for i := range rel.Assets {
		if m := re.FindStringSubmatch(rel.Assets[i].Name); m != nil {
			return &rel.Assets[i], m[1], nil
		}
	}
	return nil, "", fmt.Errorf("release 中未找到 %s 的 alpha-smart gz 资产", arch)
}

// isLocalCoreSmart 读取本地内核版本，返回 (version, isSmart)
func isLocalCoreSmart() (string, bool, error) {
	ver, err := getLocalCoreVersion()
	if err != nil {
		return "", false, err
	}
	return ver, strings.Contains(strings.ToLower(ver), "-smart-"), nil
}

// HandleSmartCoreUpdate 升级 smart 内核（POST /core/smart-update）
func HandleSmartCoreUpdate(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodPost {
		httpx.WriteJSONError(w, http.StatusMethodNotAllowed, "Method Not Allowed")
		return
	}

	localVer, isSmart, err := isLocalCoreSmart()
	if err != nil {
		httpx.WriteJSONError(w, http.StatusServiceUnavailable, "获取本地内核版本失败（内核可能在重启）: "+err.Error())
		return
	}
	if !isSmart {
		// 红线：非 smart 内核不走本通道，避免把裸 alpha“升级”成 smart 之外还搞混备份链
		httpx.WriteJSONError(w, http.StatusBadRequest,
			"当前内核不是 alpha-smart（版本: "+localVer+"），请使用普通 /upgrade 通道；smart 内核升级仅面向 smart 变体")
		return
	}

	rel, err := fetchSmartRelease()
	if err != nil {
		httpx.WriteJSONError(w, http.StatusBadGateway, "获取 smart 内核发布信息失败: "+err.Error())
		return
	}
	arch := runtime.GOARCH
	asset, remoteHash, err := smartAssetFor(rel, arch)
	if err != nil {
		httpx.WriteJSONError(w, http.StatusNotFound, err.Error())
		return
	}

	localHash := ""
	if idx := strings.LastIndex(localVer, "-"); idx >= 0 {
		localHash = localVer[idx+1:]
	}
	if localHash == remoteHash {
		w.Header().Set("Content-Type", "application/json")
		json.NewEncoder(w).Encode(map[string]interface{}{
			"status": "ok", "updated": false,
			"message":   "已是最新 smart 内核: alpha-smart-" + remoteHash,
			"local":     localVer,
			"remote":    "alpha-smart-" + remoteHash,
			"variant":   "smart",
		})
		return
	}

	// 下载（复用 selfupdate 的代理+gh-proxy 加速源回退链）
	tmp, err := os.CreateTemp(filepath.Dir(config.CoreBin), "mihomo-smart-*")
	if err != nil {
		httpx.WriteJSONError(w, http.StatusInternalServerError, "创建临时文件失败: "+err.Error())
		return
	}
	tmpPath := tmp.Name()
	defer os.Remove(tmpPath)

	proxyPort := netinfo.GetProxyPortFromConfig()
	proxyAddr := ""
	if proxyPort > 0 {
		proxyAddr = fmt.Sprintf("http://127.0.0.1:%d", proxyPort)
	}
	if err := downloadWithFallback(tmp, asset.BrowserDownloadURL, proxyAddr, ghProxyAccelerators); err != nil {
		tmp.Close()
		httpx.WriteJSONError(w, http.StatusBadGateway, "下载失败: "+err.Error())
		return
	}
	tmp.Close()

	// gzip 解压到相邻临时文件
	gzPath := tmpPath
	rawPath := tmpPath + ".raw"
	if err := gunzipFile(gzPath, rawPath); err != nil {
		os.Remove(rawPath)
		httpx.WriteJSONError(w, http.StatusBadGateway, "gzip 解压失败（包损坏？）: "+err.Error())
		return
	}
	os.Remove(gzPath)
	if err := os.Chmod(rawPath, 0755); err != nil {
		os.Remove(rawPath)
		httpx.WriteJSONError(w, http.StatusInternalServerError, "设置权限失败: "+err.Error())
		return
	}
	defer os.Remove(rawPath)

	// 简单自检：可执行且能输出版本（避免把坏包换上去）
	checkCmd := exec.Command(rawPath, "-v")
	out, err := checkCmd.Output()
	if err != nil || !strings.Contains(strings.ToLower(string(out)), "smart") {
		httpx.WriteJSONError(w, http.StatusBadGateway,
			fmt.Sprintf("新内核自检失败: %v out=%s", err, strings.TrimSpace(string(out))))
		return
	}

	// 备份旧内核并原子替换（源=rawPath）
	backupDir := filepath.Join(config.FluxorBinDir, "fluxor-backup")
	_ = os.MkdirAll(backupDir, 0755)
	backupName := filepath.Join(backupDir, "mihomo.alpha-smart."+time.Now().Format("20060102150405"))
	if data, err := os.ReadFile(config.CoreBin); err == nil {
		_ = os.WriteFile(backupName, data, 0755)
	}

	// 换内核二进制必须换进程（reload 只重载配置不换正在跑的二进制）
	if err := replaceFile(config.CoreBin, rawPath); err != nil {
		httpx.WriteJSONError(w, http.StatusInternalServerError, "替换内核文件失败: "+err.Error())
		return
	}

	// 重启内核进程
	if core.IsCoreRunning() {
		if err := core.StopCore(); err != nil {
			httpx.WriteJSONError(w, http.StatusInternalServerError, "停止旧内核失败: "+err.Error())
			return
		}
	}
	if err := core.StartCore(); err != nil {
		httpx.WriteJSONError(w, http.StatusInternalServerError, "新内核启动失败（已备份旧内核于 "+backupName+"，可手动回滚）: "+err.Error())
		return
	}

	w.Header().Set("Content-Type", "application/json")
	json.NewEncoder(w).Encode(map[string]interface{}{
		"status":   "ok",
		"updated":  true,
		"local":    localVer,
		"remote":   "alpha-smart-" + remoteHash,
		"variant":  "smart",
		"backup":   backupName,
		"message":  fmt.Sprintf("smart 内核已升级: %s → alpha-smart-%s", localVer, remoteHash),
	})
}

// HandleUpgradeGate 内核升级闸门（路由 POST /upgrade）。
// smart 内核一律拦下：内核自带 /upgrade 会拉 MetaCubeX 裸 alpha 覆盖
// smart 二进制（红线：禁止 smart 被静默换回裸 alpha），引导走
// /core/smart-update；非 smart 内核保持原有透传行为不变。
func HandleUpgradeGate(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodPost {
		httpx.WriteJSONError(w, http.StatusMethodNotAllowed, "Method Not Allowed")
		return
	}
	// fail-closed：smart 内核取不到版本时（core.sock 瞬断、内核重启中）
	// 一律拦下。放行会让内核裸 /upgrade 拉到 MetaCubeX 版本线，报
	// "already using latest version" 500 并可能替换二进制，面板随之挂死。
	localVer, isSmart, verErr := isLocalCoreSmart()
	if verErr != nil || isSmart {
		httpx.WriteJSONError(w, http.StatusConflict,
			"当前运行 smart 内核（vernesong）或非通用升级线，面板通用升级通道已禁用以防降级/版本线错配；请使用 Smart 内核更新（POST /core/smart-update）")
		return
	}

	// 透传前先确认内核在位：内核停止 / core.sock 不可用时，CoreRequest 只会
	// 返回一句笼统的「请求内核失败: dial unix ... connect: no such file or
	// directory」，运维无从分辨是升级失败还是内核根本没跑。这里给出可操作的分诊。
	if !core.IsCoreRunning() {
		httpx.WriteJSONError(w, http.StatusServiceUnavailable,
			"内核未运行，无法升级：请先启动内核后重试")
		return
	}

	targetPath := "/upgrade"
	if r.URL.RawQuery != "" {
		targetPath += "?" + r.URL.RawQuery
	}
	resp, err := core.CoreRequest(http.MethodPost, targetPath, nil)
	if err != nil {
		// PID 在位但 socket 拨不通：多半是内核正在重启窗口（core.sock 尚未重建）。
		httpx.WriteJSONError(w, http.StatusServiceUnavailable,
			"内核可能在重启中，升级请求未能送达: "+err.Error())
		return
	}
	defer resp.Body.Close()

	// 已是最新版时内核 /upgrade 会回 500 "already using latest version"——
	// 这是正常语义而非故障。识别后改写为 200 no-op，避免前端面板把「无更新」
	// 渲染成升级失败并挂死升级按钮。
	body, readErr := io.ReadAll(resp.Body)
	if readErr == nil && resp.StatusCode >= 400 &&
		strings.Contains(strings.ToLower(string(body)), "already using latest") {
		w.Header().Set("Content-Type", "application/json")
		w.WriteHeader(http.StatusOK)
		json.NewEncoder(w).Encode(map[string]interface{}{
			"status":  "ok",
			"updated": false,
			"message": "已是最新内核版本: " + localVer,
			"local":   localVer,
		})
		return
	}

	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(resp.StatusCode)
	if readErr != nil {
		return
	}
	w.Write(body)
}

// gunzipFile 把 .gz 文件解压到 dst。
func gunzipFile(src, dst string) error {
	in, err := os.Open(src)
	if err != nil {
		return err
	}
	defer in.Close()
	gzr, err := gzip.NewReader(in)
	if err != nil {
		return err
	}
	defer gzr.Close()
	out, err := os.Create(dst)
	if err != nil {
		return err
	}
	if _, err := io.Copy(out, gzr); err != nil {
		out.Close()
		os.Remove(dst)
		return err
	}
	return out.Close()
}

// replaceFile 把 src 原子替换到 dst（先尝试 rename 同卷，失败退回 copy）。
func replaceFile(dst, src string) error {
	if err := os.Rename(src, dst); err == nil {
		return nil
	}
	in, err := os.Open(src)
	if err != nil {
		return err
	}
	defer in.Close()
	out, err := os.OpenFile(dst+".new", os.O_CREATE|os.O_TRUNC|os.O_WRONLY, 0755)
	if err != nil {
		return err
	}
	if _, err := io.Copy(out, in); err != nil {
		out.Close()
		os.Remove(dst + ".new")
		return err
	}
	out.Close()
	return os.Rename(dst+".new", dst)
}
