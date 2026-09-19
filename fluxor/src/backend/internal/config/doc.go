package config

// Package config 集中定义 Fluxor 的全局运行配置与持久化状态。
//
// 该包是整个后端的“叶子”依赖：只做路径/模式解析、订阅配置的读写与全局状态
// 的持有，绝不反向依赖 core、tproxy、subscription 等上层包，从而避免循环依赖。
//
// 主要职责：
//   - paths.go     运行路径（Socket、PID、内核二进制、配置目标等）的默认值
//   - modes.go     按运行模式（fnos / openwrt）套用默认路径
//   - name.go      订阅名校验与节点文件名清洗（防路径穿越）
//   - model.go     SubscribeConfig / Subscription 数据结构定义
//   - state.go     进程内共享的当前配置与读写锁
//   - load.go      配置的加载、默认值补齐与持久化；
//                  以及 FileMu / UpdateConfigFile —— fluxor.json 的共用文件锁
//                  与「读—改—写」入口（tproxy 旁路字段与订阅配置共用该文件）
//
// 注：main.go 中的环境变量覆盖直接调用 os.Getenv，本包不再提供读取工具。
// 运行期另有 FileMu 保护配置文件本身，与保护内存快照 Current 的 Mu 是两把独立的锁。
