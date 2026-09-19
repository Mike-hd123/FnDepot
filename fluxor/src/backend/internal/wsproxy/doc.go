package wsproxy

// Package wsproxy 在前端与内核之间双向桥接 WebSocket 数据流。
//
// 覆盖 /traffic、/memory、/logs、/connections 四条实时流：升级浏览器连接后，
// 通过 Unix Socket 拨号连接内核，并开启两个 goroutine 互相拷贝消息，任一侧
// 出错即整体退出，避免连接泄漏。
