package core

import (
	"bytes"
	"encoding/json"
	"fluxor/internal/config"
	"fluxor/internal/configgen"
	"fluxor/internal/tproxy"
	"fmt"
	"io"
	"net/http"
	"os"
	"os/exec"
	"path/filepath"
	"strconv"
	"strings"
	"syscall"
	"time"
)

// ReloadCore 通过 Unix socket 重载内核配置（热重启）
func ReloadCore() error {
	bodyJSON := fmt.Sprintf(`{"path":"%s"}`, config.ConfigTarget)
	resp, err := CoreRequest(http.MethodPut, "/configs?force=true", strings.NewReader(bodyJSON))
	if err != nil {
		return fmt.Errorf("内核重载请求失败: %w", err)
	}
	defer resp.Body.Close()
	if resp.StatusCode != http.StatusOK && resp.StatusCode != http.StatusNoContent {
		respBody, _ := io.ReadAll(resp.Body)
		return fmt.Errorf("内核返回错误状态 %d: %s", resp.StatusCode, string(respBody))
	}

	// 重载成功后，异步更新 nftables TProxy 规则
	go func() {
		time.Sleep(500 * time.Millisecond)
		resp2, err2 := CoreRequest("GET", "/configs", nil)
		if err2 == nil {
			defer resp2.Body.Close()
			var info map[string]interface{}
			if err2 := json.NewDecoder(resp2.Body).Decode(&info); err2 == nil {
				if tp, ok := info["tproxy-port"]; ok {
					if tpf, ok := tp.(float64); ok {
						if tpf > 0 && tproxy.GetTproxyState() {
							tproxy.DisableTProxyRules()
							tproxy.EnableTProxyRules(int(tpf))
						} else {
							tproxy.DisableTProxyRules()
						}
					}
				}
			}
		}
	}()

	return nil
}

// IsCoreRunning 检查内核是否在运行（通过 PID 文件）
func IsCoreRunning() bool {
	data, err := os.ReadFile(config.CorePidFile)
	if err != nil {
		return false
	}
	pidStr := strings.TrimSpace(string(data))
	if pidStr == "" {
		return false
	}
	pid, err := strconv.Atoi(pidStr)
	if err != nil {
		return false
	}
	return corePidAlive(pid)
}

// corePidAlive 判断 pid 是否存活，且确实是内核进程。
//
// 仅判断「PID 存在」是不够的：内核异常退出后 PID 文件会残留，而 PID 可能已被
// 复用给无关进程；此时 StopCore 会把 SIGTERM/SIGKILL 发给那个无关进程。
// 因此额外比对 /proc/<pid>/exe 的文件名。
func corePidAlive(pid int) bool {
	process, err := os.FindProcess(pid)
	if err != nil {
		return false
	}
	if process.Signal(syscall.Signal(0)) != nil {
		return false
	}
	return pidLooksLikeCore(pid)
}

// pidLooksLikeCore 比对 /proc/<pid>/exe 的文件名是否与内核二进制一致。
//
// 判定细节：
//   - /proc/<pid>/exe 已被内核解析为真实可执行文件，因此当 CoreBin 是符号链接
//     或经软链配置时，必须把两侧都解析到真实路径再比，否则会误判为「不是内核」，
//     进而在内核明明在运行时报告「已停止」、并重复拉起第二个实例。
//   - 自更新会替换二进制，使运行中进程的 exe 指向被删除的旧 inode，
//     Readlink 会带 " (deleted)" 后缀，需要剥掉后再比。
//   - 只比文件名而非完整路径：路径可能因启动方式不同而不同（相对路径、
//     经 PATH 查找等），文件名比对已足以区分「是不是我们的内核」。
//
// 读取失败时（无 /proc、无权限、非 Linux）返回 true，退回「仅看 PID 存活」，
// 以免在受限环境下把正在运行的内核误判为未运行。
func pidLooksLikeCore(pid int) bool {
	exe, err := os.Readlink(fmt.Sprintf("/proc/%d/exe", pid))
	if err != nil {
		return true
	}
	exe = strings.TrimSuffix(exe, " (deleted)")

	want := config.CoreBin
	// 两侧统一解析软链，保证 symlink 配置下仍能匹配
	if resolved, err := filepath.EvalSymlinks(want); err == nil {
		want = resolved
	}
	return filepath.Base(exe) == filepath.Base(want)
}

// StartCore 启动内核进程
func StartCore() error {
	if IsCoreRunning() {
		return fmt.Errorf("内核已在运行")
	}

	// 确保配置文件存在，若不存在则使用 subscribeConfig 生成；若存在则强制补齐网关属性
	if _, err := os.Stat(config.ConfigTarget); os.IsNotExist(err) {
		if err := configgen.GenerateConfig(config.Current); err != nil {
			if CoreLogger != nil {
				CoreLogger.Printf("[START][ERROR] 生成配置文件失败: %v\n", err)
			}
			return fmt.Errorf("生成配置文件失败: %w", err)
		}
	}

	cmd := exec.Command(config.CoreBin, "-d", config.CoreWorkDir)
	var stderr bytes.Buffer
	cmd.Stderr = &stderr

	if err := cmd.Start(); err != nil {
		if CoreLogger != nil {
			CoreLogger.Printf("[START][ERROR] 启动内核失败: %v, stderr: %s\n", err, stderr.String())
		}
		return fmt.Errorf("启动内核失败: %v, stderr: %s", err, stderr.String())
	}

	// 等待 1 秒，检查进程是否存活
	time.Sleep(1 * time.Second)
	err := cmd.Process.Signal(syscall.Signal(0))
	if err != nil {
		stderrContent := stderr.String()
		waitErr := cmd.Wait()
		if waitErr != nil {
			stderrContent += " (Wait err: " + waitErr.Error() + ")"
		}
		if stderrContent == "" {
			stderrContent = "进程已退出，无 stderr 输出"
		}
		if CoreLogger != nil {
			CoreLogger.Printf("[START][ERROR] 内核启动后立即退出: %s\n", stderrContent)
		}
		return fmt.Errorf("内核启动后立即退出: %s", stderrContent)
	}

	pid := cmd.Process.Pid
	os.MkdirAll(filepath.Dir(config.CorePidFile), 0755)
	if err := os.WriteFile(config.CorePidFile, []byte(strconv.Itoa(pid)), 0644); err != nil {
		cmd.Process.Kill()
		cmd.Wait()
		if CoreLogger != nil {
			CoreLogger.Printf("[START][ERROR] 写入 PID 文件失败: %v\n", err)
		}
		return fmt.Errorf("写入 PID 文件失败: %v", err)
	}

	// 进程存活 ≠ API 就绪：内核从 exec 到 external-controller-unix 的
	// core.sock 可拨通之间有数百毫秒窗口（首次启动还要加载 geo/mmdb，更久）。
	// 此前 StartCore 在该窗口即返回成功，前端紧接着拉 /version、/configs 会
	// 拨到不存在的 socket，表现为「请求内核失败」。这里同步有界等待 socket
	// 就绪（上限 8s）；超时不算失败——socket 稍后仍可连通，SSE/轮询会自愈，
	// 但多数情况下调用方返回时内核已真正可用。
	if config.CoreSocket != "" {
		waitCoreSocketReady(8 * time.Second)
	}

	// 后台等待进程退出。
	//
	// 内核可能自行退出（崩溃、被外部 kill -9、OOM），此时 PID 文件需要清理，
	// 并且必须向外广播「已停止」——否则前端会一直以为内核仍在运行。
	go func() {
		cmd.Wait()
		os.Remove(config.CorePidFile)
		PublishCoreState(false)
	}()

	// 广播「已启动」。放在 goroutine 启动之后：此刻 PID 文件已写入，
	// IsCoreRunning() 已为 true，前端收到事件后再查状态能保持一致。
	PublishCoreState(true)

	return nil
}

// StopCore 停止内核进程
func StopCore() error {
	tproxy.DisableTProxyRules() // 进程停掉前，立即释放系统 nft 规则
	_ = os.Remove(config.CoreSocket)

	if !IsCoreRunning() {
		return fmt.Errorf("内核未运行，停止操作被忽略")
	}
	data, _ := os.ReadFile(config.CorePidFile)
	pid, _ := strconv.Atoi(strings.TrimSpace(string(data)))
	process, err := os.FindProcess(pid)
	if err != nil {
		if CoreLogger != nil {
			CoreLogger.Printf("[STOP][ERROR] 查找进程失败: %v\n", err)
		}
		return fmt.Errorf("查找进程失败: %v", err)
	}
	if err := process.Signal(syscall.SIGTERM); err != nil {
		if CoreLogger != nil {
			CoreLogger.Printf("[STOP][ERROR] 停止进程失败: %v\n", err)
		}
		return fmt.Errorf("停止进程失败: %v", err)
	}

	// 轮询检查进程是否退出（最大 5 秒超时，每 100ms 一次）
	killed := false
	for i := 0; i < 50; i++ {
		if process.Signal(syscall.Signal(0)) != nil {
			killed = true
			break
		}
		// PID 已被复用给无关进程时，绝不能再发 SIGKILL：那会误杀该进程。
		// 此时视为原内核已退出，转而清理残留文件。
		if !pidLooksLikeCore(pid) {
			killed = true
			break
		}
		time.Sleep(100 * time.Millisecond)
	}

	if !killed {
		// 超时则发送 SIGKILL 强杀（此时已确认仍是我们启动的内核）
		process.Signal(syscall.SIGKILL)
		time.Sleep(200 * time.Millisecond)
	}

	os.Remove(config.CorePidFile)
	_ = os.Remove(config.CoreSocket)
	// 通知所有 SSE 订阅者：内核已停止（StartCore 中等待进程的 goroutine 也会
	// 广播一次，但 hub 仅在状态真正变化时才推送，因此不会产生重复事件）。
	PublishCoreState(false)
	return nil
}
