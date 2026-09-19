// Package buildinfo 持有构建期注入的版本信息。
//
// 版本号不再由前端注入，而是在编译时通过 -ldflags 写入本包的 Version 变量：
//
//	go build -ldflags "-X fluxor/internal/buildinfo.Version=1.0.0"
//
// make 已封装该参数（make V=1.0.0），CI 亦以相同方式传入。
//
// 本包为叶子包，不依赖任何其它 internal 包，因此可被 main（HTTP 接口）与
// appupdate（自我更新比较）同时引用而不产生依赖环。
package buildinfo
