package core

import (
	"fluxor/internal/config"
	"fmt"
	"log"
	"os"
	"path/filepath"
)

// 内核操作日志记录器
var CoreLogger *log.Logger

// InitCoreLogger 初始化内核日志记录器
func InitCoreLogger() {
	dir := filepath.Dir(config.InfoLogFile)
	if err := os.MkdirAll(dir, 0755); err != nil {
		fmt.Printf("无法创建日志目录 %s: %v\n", dir, err)
		return
	}
	file, err := os.OpenFile(config.InfoLogFile, os.O_CREATE|os.O_WRONLY|os.O_APPEND, 0644)
	if err != nil {
		fmt.Printf("无法打开日志文件 %s: %v\n", config.InfoLogFile, err)
		return
	}
	CoreLogger = log.New(file, "", log.Ldate|log.Ltime|log.Lmicroseconds)
}
