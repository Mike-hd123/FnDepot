// 全局持久化日志（存在面板数据卷 /…/logs/，宿主 ./data-panel 持久保留，跨容器/面板重建不丢）。
// 两类日志：
//   _panel.log         面板级运维事件（实例创建/删除/升级/启停、镜像拉取、错误等跨实例的全局动作）
//   <实例id>.log       单实例生命周期 + 重启原因 + 重建前容器日志快照
// 单文件按大小封顶（超限截掉前半保留最近），并按「一年保留」定期清理过期行。
// 本模块不依赖 docker，避免与 docker.ts 形成循环依赖（docker.ts/index.ts 反过来引用本模块）。

import { appendFileSync, mkdirSync, statSync, readFileSync, writeFileSync, existsSync, rmSync, readdirSync } from 'node:fs';
import { dirname } from 'node:path';

// 与 store.ts 的 accounts.json 同目录。fallback 须与 store.ts 一致。
export const LOG_DIR = `${dirname(process.env.PANEL_DATA || '/data/panel/accounts.json')}/logs`;
const PER_FILE_CAP = 512 * 1024; // 单文件 ~512KB，超限截掉前半保留最近
const RETENTION_MS = 365 * 24 * 60 * 60 * 1000; // 日志保留一年，更早的自动清理
const PANEL_LOG = `${LOG_DIR}/_panel.log`;
const INSTANCE_ID_RE = /^[0-9a-f]{1,32}$/; // 实例 id 为十六进制；校验防路径注入

export type LogLevel = 'INFO' | 'WARN' | 'ERROR';

function nowIso(): string {
  return new Date().toISOString();
}

function appendTo(file: string, line: string): void {
  try {
    mkdirSync(LOG_DIR, { recursive: true });
    appendFileSync(file, line.endsWith('\n') ? line : line + '\n');
    const sz = statSync(file).size;
    if (sz > PER_FILE_CAP) writeFileSync(file, readFileSync(file).subarray(sz - Math.floor(PER_FILE_CAP / 2)));
  } catch {
    /* 写日志失败不影响主流程 */
  }
}

function instanceLogPath(id: string): string {
  return `${LOG_DIR}/${id}.log`;
}

// ---------- 单实例日志 ----------
export function appendInstanceLog(id: string, line: string): void {
  if (!INSTANCE_ID_RE.test(id)) return;
  appendTo(instanceLogPath(id), `[${nowIso()}] ${line}`);
}

export function readInstanceLog(id: string): string {
  if (!INSTANCE_ID_RE.test(id)) return '';
  try {
    const p = instanceLogPath(id);
    return existsSync(p) ? readFileSync(p, 'utf8') : '';
  } catch {
    return '';
  }
}

// 实例彻底删除（连数据卷一并清除）时，顺手删掉它的持久日志文件，避免遗留孤儿。
export function deleteInstanceLog(id: string): void {
  if (!INSTANCE_ID_RE.test(id)) return;
  try {
    rmSync(instanceLogPath(id), { force: true });
  } catch {
    /* 忽略 */
  }
}

// ---------- 面板级全局日志 ----------
// 同时写入持久文件并回显 stdout（docker logs woc-panel 仍可见）。运维动作统一走这里，便于诊断包汇总。
export function appendPanelLog(level: LogLevel, message: string): void {
  appendTo(PANEL_LOG, `[${nowIso()}] [${level}] ${message}`);
  const c = level === 'ERROR' ? console.error : level === 'WARN' ? console.warn : console.log;
  c(`[panel] ${message}`);
}

export function readPanelLog(): string {
  try {
    return existsSync(PANEL_LOG) ? readFileSync(PANEL_LOG, 'utf8') : '';
  } catch {
    return '';
  }
}

// ---------- 时间裁剪 / 保留期清理 ----------
// 保留行首 [ISO时间] >= sinceMs 的行；无法解析时间戳的行跟随上一条的保留状态（多行块整体保留）。
export function filterSince(text: string, sinceMs: number): string {
  if (!text) return '';
  const out: string[] = [];
  let keeping = false;
  for (const ln of text.split('\n')) {
    const m = /^\[(\d{4}-\d\d-\d\dT[\d:.]+Z)\]/.exec(ln);
    if (m) {
      const t = Date.parse(m[1]);
      if (Number.isFinite(t)) keeping = t >= sinceMs;
    }
    if (keeping) out.push(ln);
  }
  return out.join('\n');
}

// 清理所有日志文件中早于一年的行；整文件均过期则删除。每日定时调用。
export function pruneOldLogs(): void {
  const cutoff = Date.now() - RETENTION_MS;
  try {
    if (!existsSync(LOG_DIR)) return;
    for (const f of readdirSync(LOG_DIR)) {
      if (!f.endsWith('.log')) continue;
      const p = `${LOG_DIR}/${f}`;
      try {
        const kept = filterSince(readFileSync(p, 'utf8'), cutoff);
        if (kept.trim()) writeFileSync(p, kept.endsWith('\n') ? kept : kept + '\n');
        else rmSync(p, { force: true });
      } catch {
        /* 单文件失败不影响其它 */
      }
    }
  } catch {
    /* 忽略 */
  }
}

// 诊断包可选时间范围（→ 毫秒）。默认 24h。
export const DIAG_RANGES: Record<string, number> = {
  '24h': 24 * 60 * 60 * 1000,
  '7d': 7 * 24 * 60 * 60 * 1000,
  '30d': 30 * 24 * 60 * 60 * 1000,
  '1y': 365 * 24 * 60 * 60 * 1000,
};
export function rangeToMs(range: string | undefined): number {
  return DIAG_RANGES[range ?? '24h'] ?? DIAG_RANGES['24h'];
}
