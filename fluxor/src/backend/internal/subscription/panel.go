package subscription

import (
	"fluxor/internal/config"
	"fmt"
	"os"
	"path/filepath"
	"regexp"
)

const metaConfigFile = "config.js"

// modifyMetaConfig 修改 MetaCubeXD 的 config.js 文件中的后端地址
func modifyMetaConfig(backendURL string) error {
	configPath := filepath.Join(config.MetaDir, metaConfigFile)
	if _, err := os.Stat(configPath); os.IsNotExist(err) {
		return fmt.Errorf("config.js 不存在")
	}
	content, err := os.ReadFile(configPath)
	if err != nil {
		return err
	}
	re := regexp.MustCompile(`defaultBackendURL:\s*['"][^'"]*['"]`)
	if !re.MatchString(string(content)) {
		return fmt.Errorf("未找到 defaultBackendURL 配置项或格式不匹配")
	}
	newContent := re.ReplaceAllString(string(content), fmt.Sprintf("defaultBackendURL: '%s'", backendURL))
	if string(content) == newContent {
		return nil // 无需重复写入
	}
	return os.WriteFile(configPath, []byte(newContent), 0644)
}
