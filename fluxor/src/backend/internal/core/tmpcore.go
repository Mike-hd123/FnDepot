package core

import (
	"bytes"
	"encoding/json"
	"errors"
	"fluxor/internal/config"
	"fluxor/internal/configcheck"
	"fmt"
	"io"
	"log"
	"net"
	"net/http"
	"net/url"
	"os"
	"os/exec"
	"path/filepath"
	"strconv"
	"strings"
	"sync"
	"syscall"
	"time"
)

// errInvalidSubscription 标记「订阅内容本身无法被内核解析」，用于让上层区分
// 「链接/内容问题」与「临时内核启动失败」这类环境问题，以便给出更准确的提示。
var errInvalidSubscription = errors.New("订阅内容无法被内核解析")

// tempCoreTimeout 临时内核从启动到产出节点文件的总超时（含元数据抓取）。
//
// 与直连下载保持一致：15 秒，且不做重试——超时即如实报错，
// 不再反复重试（此前为 60 秒 × 3 次重试，最长可拖到 3 分钟）。
const tempCoreTimeout = 15 * time.Second

// DownloadWithTempCore 使用临时内核下载单个订阅的节点文件，并返回元数据（updatedAt 和 subscriptionInfo）。
//
// 不重试：无论是端口分配、启动失败还是下载超时，都只尝试一次并如实返回错误，
// 避免用户等待成倍放大。
func DownloadWithTempCore(sub config.Subscription, index int, targetFile string) (updatedAt string, subInfo map[string]interface{}, err error) {
	tmpDir := filepath.Dir(config.CorePidFile)

	// 临时文件名必须唯一：此前按 index 命名（tmp{index}.yaml），并发触发的
	// 下载（例如 ensureSubscriptionFiles 与定时更新同时命中同一订阅）会互相
	// 覆盖彼此的临时配置与 PID 文件，导致读到错误的配置或清理掉对方的进程。
	uid, err := os.CreateTemp(tmpDir, fmt.Sprintf("tmp%d-*.yaml", index))
	if err != nil {
		return "", nil, fmt.Errorf("创建临时配置文件失败: %w", err)
	}
	tmpConfig := uid.Name()
	uid.Close()
	// 交给 mihomo 以 -f 打开，先移除占位文件，避免出现「空配置」窗口
	_ = os.Remove(tmpConfig)
	tmpPidFile := strings.TrimSuffix(tmpConfig, ".yaml") + ".pid"

	// 无论成功失败都回收本次的临时文件
	defer func() {
		os.Remove(tmpConfig)
		os.Remove(tmpPidFile)
	}()

	listener, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		return "", nil, fmt.Errorf("分配端口失败: %w", err)
	}
	port := listener.Addr().(*net.TCPAddr).Port
	listener.Close()

	content, err := buildTempCoreConfig(port, sub, targetFile)
	if err != nil {
		return "", nil, err
	}

	if err := os.WriteFile(tmpConfig, []byte(content), 0644); err != nil {
		return "", nil, fmt.Errorf("写入临时配置失败: %w", err)
	}

	cmd := exec.Command(config.CoreBin, "-f", tmpConfig, "-d", config.CoreWorkDir)
	// mihomo 的日志（含 provider 加载失败原因）写入 stdout 而非 stderr，
	// 因此必须捕获 stdout，否则失败原因会全部丢失。
	output := newSyncBuffer()
	cmd.Stdout = output
	cmd.Stderr = output

	if err := cmd.Start(); err != nil {
		return "", nil, fmt.Errorf("启动临时内核失败: %w, 输出: %s", err, output.String())
	}

	// 保存 PID
	_ = os.WriteFile(tmpPidFile, []byte(strconv.Itoa(cmd.Process.Pid)), 0644)

	// 监测是否因端口冲突在启动瞬间退出
	time.Sleep(100 * time.Millisecond)
	if cmd.Process != nil && cmd.Process.Signal(syscall.Signal(0)) != nil {
		return "", nil, fmt.Errorf("临时内核启动后立即退出，可能端口冲突，输出: %s", output.String())
	}

	return runDownloadProcess(cmd, targetFile, port, sub.Name, tmpConfig, tmpPidFile, output)
}

// CleanupStaleTempCores 清理上次运行遗留的临时内核进程与临时文件。
//
// 正常路径下 runDownloadProcess 的 defer 会终止临时内核并删除文件，但进程被
// kill -9 或崩溃时不会执行：残留的临时内核会继续持有端口并周期拉取订阅，
// 临时配置/PID 文件也会堆积。因此启动时清扫一次。
//
// 判定依据是 PID 文件中的进程 exe 仍指向内核二进制，避免误杀 PID 已被复用的
// 无关进程。
func CleanupStaleTempCores() {
	tmpDir := filepath.Dir(config.CorePidFile)

	pidFiles, err := filepath.Glob(filepath.Join(tmpDir, "tmp*.pid"))
	if err != nil {
		return
	}
	cleaned := 0
	for _, pidFile := range pidFiles {
		data, err := os.ReadFile(pidFile)
		if err == nil {
			if pid, convErr := strconv.Atoi(strings.TrimSpace(string(data))); convErr == nil && pid > 1 {
				// 只杀「确实是内核」的进程，PID 被复用给其它程序时跳过
				if pidLooksLikeCore(pid) {
					if proc, findErr := os.FindProcess(pid); findErr == nil {
						_ = proc.Kill()
						cleaned++
					}
				}
			}
		}
		_ = os.Remove(pidFile)
	}

	// 回收遗留的临时配置文件
	configFiles, err := filepath.Glob(filepath.Join(tmpDir, "tmp*.yaml"))
	if err == nil {
		for _, f := range configFiles {
			_ = os.Remove(f)
		}
	}

	if cleaned > 0 {
		log.Printf("[TMPCORE] 启动清理：终止了 %d 个残留的临时内核进程", cleaned)
	}
}

// buildTempCoreConfig 生成临时内核的最小配置（仅含一个 http provider）。
//
// 用 YAML 文档而非字符串拼接：订阅名作为 provider 的键，可能含 `:` 等 YAML
// 特殊字符（如「机场: 香港」），插值会产出内核无法解析的配置。经文档写入
// 可正确转义。
func buildTempCoreConfig(port int, sub config.Subscription, targetFile string) (string, error) {
	doc := configcheck.NewDoc()
	if err := doc.Set("mixed-port", 0); err != nil {
		return "", err
	}
	if err := doc.Set("log-level", "error"); err != nil {
		return "", err
	}
	if err := doc.Set("external-controller", fmt.Sprintf("127.0.0.1:%d", port)); err != nil {
		return "", err
	}
	if err := doc.Set("proxy-providers", tempCoreProviders(sub, targetFile)); err != nil {
		return "", err
	}

	out, err := doc.Bytes()
	if err != nil {
		return "", fmt.Errorf("序列化临时内核配置失败: %w", err)
	}
	return string(out), nil
}

// tempCoreProviders 构建形如 {<订阅名>: {type, url, path}} 的 provider 映射。
func tempCoreProviders(sub config.Subscription, targetFile string) map[string]any {
	return map[string]any{
		sub.Name: map[string]any{
			"type": "http",
			"url":  sub.URL,
			"path": targetFile,
		},
	}
}

func runDownloadProcess(cmd *exec.Cmd, targetFile string, port int, subName string, tmpConfig string, tmpPidFile string, output *syncBuffer) (updatedAt string, subInfo map[string]interface{}, err error) {
	defer func() {
		os.Remove(tmpConfig)
		os.Remove(tmpPidFile)
		if cmd.Process != nil {
			_ = cmd.Process.Signal(syscall.SIGTERM)
			done := make(chan error, 1)
			go func() {
				done <- cmd.Wait()
			}()
			select {
			case <-done:
			case <-time.After(1 * time.Second):
				_ = cmd.Process.Kill()
				// 再等一次但【必须有界】：cmd.Wait 会等待其 stdout/stderr 的拷贝
				// goroutine 结束，而管道写端可能被子进程继承。若内核派生了子进程
				// 且未退出，Wait 可能长期不返回——早前这里的 `<-done` 是无界的，
				// 会让调用方突破 15 秒上限、甚至永久阻塞。这里最多再等 2 秒后放弃
				// （进程已被 Kill，残留的回收 goroutine 会在其真正退出时自行结束）。
				select {
				case <-done:
				case <-time.After(2 * time.Second):
					log.Printf("[DOWNLOAD] 临时内核未在预期时间内完成回收，放弃等待")
				}
			}
		}
	}()

	// 整个「等待文件生成 + 抓取元数据」共用同一个 15 秒截止时间，
	// 保证端到端确实是 15 秒上限，而不是在等待超时后再叠加元数据抓取的耗时。
	deadline := time.Now().Add(tempCoreTimeout)
	remaining := func() time.Duration { return time.Until(deadline) }

	// 轮询等待目标文件生成，同时检查进程存活。
	//
	// provider 加载失败时内核不会退出、也不会写文件，只在日志中留下一行 error
	// （见 providerLoadError），若仅等待文件生成就会一直卡到超时，因此一旦发现
	// 内核已明确报错便立即终止，不再空等。
	timeout := time.After(tempCoreTimeout)
	ticker := time.NewTicker(500 * time.Millisecond)
	defer ticker.Stop()

	fileGenerated := false
	for !fileGenerated {
		select {
		case <-timeout:
			if reason := providerLoadError(output.String(), subName); reason != "" {
				return "", nil, fmt.Errorf("%w: 订阅内容无法被内核解析: %s", errInvalidSubscription, reason)
			}
			return "", nil, fmt.Errorf("下载超时（%s），文件未生成", tempCoreTimeout)
		case <-ticker.C:
			// 检查进程是否存活
			if cmd.Process == nil || cmd.Process.Signal(syscall.Signal(0)) != nil {
				return "", nil, fmt.Errorf("临时内核进程意外退出")
			}
			info, statErr := os.Stat(targetFile)
			if statErr == nil && info.Size() > 0 {
				// 内核按 provider 契约写回的是「原始响应」，未必是主配置契约下的
				// Clash YAML（例如 Base64 编码的 URI 列表会被原样落盘）。后续
				// patchSubscriptionFile 会在这份内容上拼接 YAML 键，因此必须先校验，
				// 否则会产出内核无法加载的 config.yaml。
				if content, readErr := os.ReadFile(targetFile); readErr == nil {
					if validErr := configcheck.ValidateClashConfig(content); validErr != nil {
						log.Printf("[DOWNLOAD] 内核产出的订阅文件无效，终止: %s", validErr)
						return "", nil, fmt.Errorf("%w: 内核可读取该订阅但产出内容不是 Clash 配置: %s",
							errInvalidSubscription, validErr)
					}
				}
				log.Printf("[DOWNLOAD] 文件 %s 已生成，大小 %d 字节", targetFile, info.Size())
				fileGenerated = true
				break
			}
			// 内核已明确判定 provider 加载失败，无需继续等待
			if reason := providerLoadError(output.String(), subName); reason != "" {
				log.Printf("[DOWNLOAD] 内核加载 provider %s 失败，提前终止: %s", subName, reason)
				return "", nil, fmt.Errorf("%w: 该订阅链接不是 Clash 配置，内核解析失败: %s", errInvalidSubscription, reason)
			}
		}
	}

	// 元数据取自本机临时内核的 HTTP 接口：此时文件已落盘、内核必然在运行，
	// 该请求应当瞬时完成。这里只发一次请求，不做重试；超时取 15 秒总预算的
	// 剩余部分，保证端到端不突破该上限。
	left := remaining()
	if left <= 0 {
		return "", nil, fmt.Errorf("获取订阅元数据超时（已用满 %s）", tempCoreTimeout)
	}
	urlPath := fmt.Sprintf("http://127.0.0.1:%d/providers/proxies/%s", port, url.QueryEscape(subName))
	client := &http.Client{Timeout: min(3*time.Second, left)}

	resp, err := client.Get(urlPath)
	if err != nil {
		return "", nil, fmt.Errorf("获取订阅元数据失败: %w", err)
	}
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusOK {
		bodyBytes, _ := io.ReadAll(resp.Body)
		return "", nil, fmt.Errorf("获取元数据返回非200状态: %d, body: %s", resp.StatusCode, string(bodyBytes))
	}

	body, err := io.ReadAll(resp.Body)
	if err != nil {
		return "", nil, fmt.Errorf("读取响应失败: %w", err)
	}

	var data map[string]interface{}
	if err := json.Unmarshal(body, &data); err != nil {
		return "", nil, fmt.Errorf("解析JSON失败: %w", err)
	}

	updatedAtVal, _ := data["updatedAt"].(string)
	subInfoVal, _ := data["subscriptionInfo"].(map[string]interface{})

	log.Printf("[DOWNLOAD] 成功获取元数据: updatedAt=%s, subInfo=%v", updatedAtVal, subInfoVal)
	return updatedAtVal, subInfoVal, nil
}

// providerLoadError 从临时内核输出中提取指定 provider 的加载失败原因。
//
// 内核在 provider 初始化失败时会输出形如：
//
//	level=error msg="initial proxy provider <name> error: <原因>"
//
// 返回空串表示内核尚未报告该 provider 的加载错误。
func providerLoadError(output string, subName string) string {
	marker := "initial proxy provider " + subName + " error:"
	for _, line := range strings.Split(output, "\n") {
		idx := strings.Index(line, marker)
		if idx < 0 {
			continue
		}
		reason := strings.TrimSpace(line[idx+len(marker):])
		// 去掉 msg 字段的收尾引号（含 \" 转义）
		reason = strings.TrimSuffix(reason, `"`)
		// 内核把多行错误压成 `标题:\n 实际原因` 的转义形式，
		// 还原后只保留最有信息量的那段，避免展示无意义的标题。
		reason = strings.ReplaceAll(reason, `\n`, "\n")
		reason = strings.ReplaceAll(reason, `\"`, `"`)
		return condenseReason(reason)
	}
	return ""
}

// condenseReason 把内核错误压成单行：yaml.v3 的错误首行只是
// 「yaml: unmarshal errors:」这样的标题，真正的原因在其后。
func condenseReason(s string) string {
	lines := strings.Split(s, "\n")
	for i := len(lines) - 1; i >= 0; i-- {
		if line := strings.TrimSpace(lines[i]); line != "" {
			return line
		}
	}
	return strings.TrimSpace(s)
}

// syncBuffer 是并发安全的输出缓冲：由内核子进程写入，父进程并发读取。
type syncBuffer struct {
	mu  sync.Mutex
	buf bytes.Buffer
}

func newSyncBuffer() *syncBuffer { return &syncBuffer{} }

func (b *syncBuffer) Write(p []byte) (int, error) {
	b.mu.Lock()
	defer b.mu.Unlock()
	return b.buf.Write(p)
}

func (b *syncBuffer) String() string {
	b.mu.Lock()
	defer b.mu.Unlock()
	return b.buf.String()
}
