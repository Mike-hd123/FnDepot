package appupdate

import (
	"encoding/json"
	"fluxor/internal/buildinfo"
	"fluxor/internal/config"
	"fluxor/internal/httpx"
	"fluxor/internal/netinfo"
	"fmt"
	"io"
	"net/http"
	"net/url"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"time"
)

// 加速源列表（按优先级排序）
var ghProxyAccelerators = []string{
	"https://gh-proxy.org/",
	"https://hk.gh-proxy.org/",
	"https://cdn.gh-proxy.org/",
	"https://edgeone.gh-proxy.org/",
	"https://gh-proxy.com/",
}

// downloadOnce 执行单次下载，支持代理，并写入目标文件
func downloadOnce(dst *os.File, rawURL string, proxyAddr string) error {
	client := &http.Client{
		Timeout: 60 * time.Second,
	}
	if proxyAddr != "" {
		proxyURL, err := url.Parse(proxyAddr)
		if err != nil {
			return err
		}
		client.Transport = &http.Transport{Proxy: http.ProxyURL(proxyURL)}
	}

	resp, err := client.Get(rawURL)
	if err != nil {
		return err
	}
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusOK {
		return fmt.Errorf("HTTP %d", resp.StatusCode)
	}

	// 重置文件指针并清空内容
	if _, err := dst.Seek(0, 0); err != nil {
		return err
	}
	if err := dst.Truncate(0); err != nil {
		return err
	}

	_, err = io.Copy(dst, resp.Body)
	return err
}

// downloadWithFallback 尝试多种方式下载文件，每个尝试支持重试
func downloadWithFallback(dst *os.File, originalURL string, proxyAddr string, accelerators []string) error {
	type attempt struct {
		url   string
		proxy string // 空表示无代理
	}

	var attempts []attempt
	// 1. 优先通过代理直接下载（如果代理可用）
	if proxyAddr != "" {
		attempts = append(attempts, attempt{url: originalURL, proxy: proxyAddr})
	}
	// 2. 直连原始URL
	attempts = append(attempts, attempt{url: originalURL, proxy: ""})
	// 3. 尝试各个加速源（不带代理）
	for _, acc := range accelerators {
		attempts = append(attempts, attempt{url: acc + originalURL, proxy: ""})
	}

	for _, att := range attempts {
		for retry := 0; retry < 3; retry++ {
			if retry > 0 {
				time.Sleep(1 * time.Second)
			}
			err := downloadOnce(dst, att.url, att.proxy)
			if err == nil {
				return nil
			}
			// 失败后重置文件以便下次重试
			if _, err := dst.Seek(0, 0); err != nil {
				return err
			}
			if err := dst.Truncate(0); err != nil {
				return err
			}
		}
	}
	return fmt.Errorf("所有下载尝试均失败")
}

// HandleSelfUpdate 更新自身
func HandleSelfUpdate(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodPost {
		http.Error(w, "Method Not Allowed", http.StatusMethodNotAllowed)
		return
	}

	rel, err := getLatestReleaseInfo()
	if err != nil {
		httpx.WriteJSONError(w, http.StatusInternalServerError, "获取版本信息失败: "+err.Error())
		return
	}

	// 当前版本取自编译期注入的 buildinfo.Version（不再由前端经 ?current= 传入）。
	current := stripVersionSuffix(buildinfo.Name())
	if !buildinfo.IsKnown() {
		httpx.WriteJSONError(w, http.StatusBadRequest, "当前版本未知（构建时未注入版本号），无法自更新")
		return
	}
	if compareVersions(rel.TagName, current) <= 0 {
		httpx.WriteJSONError(w, http.StatusBadRequest, "当前已是最新版本，无需更新")
		return
	}

	// 确定目标路径
	targetPath := filepath.Join(config.FluxorBinDir, "fluxor")
	backupDir := filepath.Join(config.FluxorBinDir, "fluxor-backup")
	if err := os.MkdirAll(backupDir, 0755); err != nil {
		httpx.WriteJSONError(w, http.StatusInternalServerError, "创建备份目录失败: "+err.Error())
		return
	}

	// 架构匹配
	archMap := map[string]string{
		"amd64":  "fluxor-amd64",
		"arm64":  "fluxor-arm64",
		"arm":    "fluxor-arm64",
		"x86":    "fluxor-amd64",
		"x86_64": "fluxor-amd64",
	}
	expectedName := archMap[runtime.GOARCH]
	if expectedName == "" {
		httpx.WriteJSONError(w, http.StatusBadRequest, "不支持的架构: "+runtime.GOARCH)
		return
	}

	var downloadURL string
	for _, asset := range rel.Assets {
		if asset.Name == expectedName {
			downloadURL = asset.BrowserDownloadURL
			break
		}
	}
	if downloadURL == "" {
		httpx.WriteJSONError(w, http.StatusNotFound, "未找到对应架构的发布文件: "+expectedName)
		return
	}

	// 获取代理端口
	proxyPort := netinfo.GetProxyPortFromConfig()
	proxyAddr := ""
	if proxyPort > 0 {
		proxyAddr = fmt.Sprintf("http://127.0.0.1:%d", proxyPort)
	}

	// 创建临时文件
	tmpFile, err := os.CreateTemp("", "fluxor-update-*")
	if err != nil {
		httpx.WriteJSONError(w, http.StatusInternalServerError, "创建临时文件失败: "+err.Error())
		return
	}
	tmpPath := tmpFile.Name()
	defer os.Remove(tmpPath)

	// 下载（包含代理、加速源、直连）
	if err := downloadWithFallback(tmpFile, downloadURL, proxyAddr, ghProxyAccelerators); err != nil {
		httpx.WriteJSONError(w, http.StatusInternalServerError, "下载失败: "+err.Error())
		return
	}
	tmpFile.Close()

	if err := os.Chmod(tmpPath, 0755); err != nil {
		httpx.WriteJSONError(w, http.StatusInternalServerError, "设置临时文件权限失败: "+err.Error())
		return
	}

	// 备份旧文件
	if _, err := os.Stat(targetPath); err == nil {
		backupName := filepath.Join(backupDir, "fluxor")
		if err := os.Rename(targetPath, backupName); err != nil {
			httpx.WriteJSONError(w, http.StatusInternalServerError, "备份旧文件失败: "+err.Error())
			return
		}
	}

	// 复制新文件
	srcFile, err := os.Open(tmpPath)
	if err != nil {
		httpx.WriteJSONError(w, http.StatusInternalServerError, "打开临时文件失败: "+err.Error())
		return
	}
	defer srcFile.Close()

	dstFile, err := os.Create(targetPath)
	if err != nil {
		httpx.WriteJSONError(w, http.StatusInternalServerError, "创建目标文件失败: "+err.Error())
		return
	}
	defer dstFile.Close()

	if _, err := io.Copy(dstFile, srcFile); err != nil {
		httpx.WriteJSONError(w, http.StatusInternalServerError, "复制文件失败: "+err.Error())
		return
	}
	if err := os.Chmod(targetPath, 0755); err != nil {
		httpx.WriteJSONError(w, http.StatusInternalServerError, "设置目标文件权限失败: "+err.Error())
		return
	}

	// 响应成功
	w.Header().Set("Content-Type", "application/json")
	json.NewEncoder(w).Encode(map[string]string{"status": "ok", "message": "更新成功，即将重启"})

	// 启动新进程并退出
	go func() {
		time.Sleep(200 * time.Millisecond)
		cmd := exec.Command(targetPath, os.Args[1:]...)
		cmd.Stdin = os.Stdin
		cmd.Stdout = os.Stdout
		cmd.Stderr = os.Stderr
		cmd.Env = os.Environ()
		if err := cmd.Start(); err != nil {
			fmt.Printf("重启失败: %v\n", err)
			return
		}
		os.Exit(0)
	}()
}
