package configgen

import (
	"log"
	"os"
	"path/filepath"

	"fluxor/internal/config"
)

// asnAvailable 检查内核工作目录下的 ASN.mmdb 是否存在且非空。
//
// 背景（1.4.0-3 掉线根因）：smart 组的 `prefer-asn: true` 需要 Mihomo 读
// ASN.mmdb 做自治系统号优选。该文件缺失时 smart 内核会 fatal 崩溃——这是
// 1.4.0-3 的掉线根因，1.4.0-4 临时靠去掉 prefer-asn 绕过，但那是「牺牲功能
// 换可用性」，用户拿不到 ASN 优选能力且无从察觉原因。
//
// 这里改为优雅降级：生成配置时探测 ASN.mmdb，缺失则把 prefer-asn 写成
// false（smart 组仍能按延迟测速优选，只是不做 ASN 归并），并打一条醒目日志，
// 告诉运维去补齐 mmdb，而不是让内核直接崩掉。
func asnAvailable() bool {
	path := filepath.Join(config.CoreWorkDir, "ASN.mmdb")
	fi, err := os.Stat(path)
	if err != nil || fi.Size() == 0 {
		log.Printf("[configgen] ASN.mmdb 不存在或为空（%s），prefer-asn 降级为 false；补齐该文件可恢复 ASN 优选", path)
		return false
	}
	return true
}
