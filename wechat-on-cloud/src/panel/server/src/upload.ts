// 上传请求体的接收：流式处理，不把整个文件读进面板内存。
// 此前所有上传都先整个读成 Buffer（Fastify parseAs: 'buffer'），再在内存里拼 tar / 解 gzip：
// 传一个 400MB 的文件面板内存涨约 1.2GB，整卷恢复上限 3GB 时峰值可到十几 GB，NAS 上面板直接被 OOM 杀掉，
// 所有人的桌面跟着断。现在：
//   单个文件（桌面文件中转、数据卷上传）→ 边收边打成 tar 转给 docker（见 tar.ts 的 tarFileStream）
//   压缩包（上传并解压、整卷恢复）→ 先落到面板数据目录的临时文件，整体校验通过后再写进实例
//   壁纸 / 字体 / 粘贴图片这些小文件 → 仍读进内存，但有明确上限
import { createReadStream, createWriteStream, mkdirSync, rmSync, statfsSync, statSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { randomBytes } from 'node:crypto';
import { Readable } from 'node:stream';
import { pipeline } from 'node:stream/promises';
import type { FastifyRequest } from 'fastify';

export const MiB = 1024 * 1024;
export const GiB = 1024 * MiB;

// 带 HTTP 状态码的上传错误（413 太大 / 507 空间不足 / 400 中断或格式不对 ……）
export class UploadError extends Error {
  constructor(
    public statusCode: number,
    message: string,
  ) {
    super(message);
  }
}

export function fmtSize(n: number): string {
  if (n >= GiB) return `${(n / GiB).toFixed(n >= 10 * GiB ? 0 : 1)} GB`;
  if (n >= MiB) return `${Math.round(n / MiB)} MB`;
  return `${Math.max(1, Math.round(n / 1024))} KB`;
}

const tooBig = (limit: number) => new UploadError(413, `文件太大（上限 ${fmtSize(limit)}）`);

// 请求头声明的长度；没有（分块传输）返回 null
export function declaredLength(req: FastifyRequest): number | null {
  const v = req.headers['content-length'];
  if (v === undefined) return null;
  const n = Number(v);
  if (!Number.isSafeInteger(n) || n < 0) throw new UploadError(400, 'Content-Length 不合法');
  return n;
}

// octet-stream 请求体的原始流（index.ts 注册的解析器原样交出，不读取）
export function rawBody(req: FastifyRequest): NodeJS.ReadableStream {
  const b = req.body as any;
  if (!b || typeof b.on !== 'function' || typeof b.pause !== 'function') {
    throw new UploadError(400, '请以 application/octet-stream 上传文件内容');
  }
  return b as NodeJS.ReadableStream;
}

// 把请求体包成一个「放弃读取时不毁掉请求」的流：计数、超限 / 与声明长度不符 / 中途断开都报错。
// 直接 for await 或 pipeline 请求本身的话，中途出错会连带 destroy 请求，socket 随之被关，错误响应根本发不出去
// （前端只看到「网络错误」）；这里出错只暂停请求，由路由回一个带 connection: close 的错误响应后再断开。
export function guardedBody(src: NodeJS.ReadableStream, expected: number | null, limit: number): Readable {
  let got = 0;
  let ended = false;
  const out = new Readable({
    highWaterMark: 256 * 1024,
    read() {
      src.resume();
    },
    destroy(err, cb) {
      detach();
      src.pause();
      cb(err);
    },
  });
  const onData = (chunk: Buffer) => {
    got += chunk.length;
    if (got > limit) return void out.destroy(tooBig(limit));
    if (expected !== null && got > expected) return void out.destroy(new UploadError(400, '上传的数据比声明的长度多'));
    if (!out.push(chunk)) src.pause();
  };
  const onEnd = () => {
    ended = true;
    detach();
    if (expected !== null && got !== expected) {
      out.destroy(new UploadError(400, `上传中断：只收到 ${fmtSize(got)} / ${fmtSize(expected)}`));
      return;
    }
    out.push(null);
  };
  const onError = () => out.destroy(new UploadError(400, '上传中断（连接已断开）'));
  const onClose = () => {
    if (!ended) out.destroy(new UploadError(400, '上传中断（连接已断开）'));
  };
  const detach = () => {
    src.off('data', onData);
    src.off('end', onEnd);
    src.off('error', onError);
    src.off('close', onClose);
  };
  src.on('data', onData);
  src.on('end', onEnd);
  src.on('error', onError);
  src.on('close', onClose);
  src.pause(); // 下游开始读才放水
  return out;
}

// 小文件：整个读进内存（有上限）
export async function readBody(req: FastifyRequest, limit: number): Promise<Buffer> {
  const expected = declaredLength(req);
  if (expected !== null && expected > limit) throw tooBig(limit);
  const chunks: Buffer[] = [];
  for await (const c of guardedBody(rawBody(req), expected, limit)) chunks.push(c as Buffer);
  return Buffer.concat(chunks);
}

// ---------- 落盘暂存（压缩包：要先整体校验再写进实例） ----------
const SPOOL_DIR = join(dirname(process.env.PANEL_DATA || '/data/panel/accounts.json'), '.upload-tmp');
const SPOOL_RESERVE = 256 * MiB; // 暂存后面板数据目录至少还留这么多，免得把宿主盘写满

// 面板启动时清掉上次异常退出（重启 / 升级 / 断电）留下的暂存文件
export function cleanSpoolDir(): void {
  try {
    rmSync(SPOOL_DIR, { recursive: true, force: true });
  } catch {
    /* 清不掉不影响启动 */
  }
}

function freeBytes(dir: string): number | null {
  try {
    const s = statfsSync(dir);
    return Number(s.bavail) * Number(s.bsize);
  } catch {
    return null;
  }
}

export interface Spooled {
  path: string;
  size: number;
  dispose: () => void;
}

export async function spoolUpload(req: FastifyRequest, limit: number): Promise<Spooled> {
  const expected = declaredLength(req);
  if (expected !== null && expected > limit) throw tooBig(limit);
  const src = rawBody(req);
  mkdirSync(SPOOL_DIR, { recursive: true, mode: 0o700 });
  const free = freeBytes(SPOOL_DIR);
  if (free !== null && (expected ?? 0) + SPOOL_RESERVE > free) {
    throw new UploadError(
      507,
      `面板数据目录剩余空间不足：${expected !== null ? `需要暂存 ${fmtSize(expected)}，` : ''}只剩 ${fmtSize(free)}（压缩包要先完整收下、校验后再写进实例）`,
    );
  }
  const path = join(SPOOL_DIR, `${Date.now()}-${randomBytes(6).toString('hex')}`);
  const dispose = () => rmSync(path, { force: true });
  try {
    await pipeline(guardedBody(src, expected, limit), createWriteStream(path, { flags: 'wx', mode: 0o600 }));
  } catch (e: any) {
    dispose();
    if (e?.code === 'ENOSPC') throw new UploadError(507, '面板数据目录所在磁盘已满，压缩包没能收完');
    throw e;
  }
  return { path, size: statSync(path).size, dispose };
}

// 单个文件：有 Content-Length 就边收边写（use 拿到的是请求体本身）；没有（分块传输，少见）就先落盘拿到大小，
// 因为 tar 头里得先写明文件大小。
export async function receiveFile<T>(
  req: FastifyRequest,
  limit: number,
  use: (size: number, body: AsyncIterable<Buffer>) => Promise<T>,
): Promise<T> {
  const expected = declaredLength(req);
  if (expected !== null) {
    if (expected > limit) throw tooBig(limit);
    const body = guardedBody(rawBody(req), expected, limit);
    try {
      return await use(expected, body);
    } finally {
      body.destroy();
    }
  }
  const spool = await spoolUpload(req, limit);
  try {
    return await use(spool.size, createReadStream(spool.path));
  } finally {
    spool.dispose();
  }
}
