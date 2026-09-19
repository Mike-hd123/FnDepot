package subscription

// Package subscription 实现订阅中心：配置 CRUD、节点文件下载、定时更新与健康检查。
//
// 子目录 download 是独立的下载层（Clash YAML 直连下载 + 临时内核回退 + 元数据解析），
// 被本包复用；它不反向依赖本包，因此不会形成循环。
//
// 文件划分：
//   - api.go               /subscribe/config 读写
//   - api_generate.go      /subscribe/generate：生成/复制 config.yaml 并热重载
//   - api_update.go        /subscribe/update/{name}
//   - api_updateinfo.go    /subscribe/update-info/{name}
//   - active.go            校验 active_subscription 是否为当前订阅列表的真实成员
//   - patch.go             往订阅节点文件注入 Fluxor 必需的端口/密钥/DNS 字段
//   - ensure.go            切换模式下确保所有订阅文件就绪
//   - update.go            切换模式下的单个订阅更新流程
//   - timers.go            定时更新与健康检查定时器的生命周期
//   - metadata.go          订阅流量/有效期等元数据的抓取与规范化
//   - healthcheck.go       switch 模式下对激活订阅的周期性测速
//   - panel.go             改写 MetaCubeXD config.js 的后端地址
//   - fileutil.go          文件复制工具
//
// config.yaml 的实际生成由 configgen 包负责（本包在 api_generate.go 中调用），
// 本包不再自带生成器。
