// tar 编解码（putArchive 只收 tar；不引入第三方依赖）。
// 打包：上传是边收边转给 docker 的，所以头部、数据、结尾分开产出，数据块原样透传，不在内存里拼整个文件。
// 解析：解压 / 整卷恢复前先把整个包走一遍（只读头部、跳过数据），确认没被截断、路径不越界，再真正写进实例。
import { Readable } from 'node:stream';
import { createReadStream } from 'node:fs';
import { open } from 'node:fs/promises';
import zlib from 'node:zlib';

const BLOCK = 512;
const ZERO_BLOCK = Buffer.alloc(BLOCK, 0);
const END_OF_ARCHIVE = Buffer.alloc(BLOCK * 2, 0);
const MAX_OCTAL_SIZE = 0o77777777777; // 头部 size 字段 11 位八进制，最大约 8 GiB；再大写进 PAX
const padLen = (n: number) => (BLOCK - (n % BLOCK)) % BLOCK;
export const pad512 = (n: number): Buffer => Buffer.alloc(padLen(n), 0);

export class TarError extends Error {}

// 属主写实例里 abc 用户的 uid / gid：面板把自己的 PUID / PGID 原样传给实例（见 docker.ts），默认 1000。
// 此前写死 1000，PUID 设成别的（群晖常见 1026）时传进去的文件属于另一个用户，应用删不掉也改不了。
const ownerField = (v: string | undefined): string => {
  const n = Number(v || '1000');
  return (Number.isInteger(n) && n >= 0 && n <= 0o7777777 ? n : 1000).toString(8).padStart(7, '0') + '\0';
};
const UID_FIELD = ownerField(process.env.PUID);
const GID_FIELD = ownerField(process.env.PGID);

function ustarHeader(name: string, size: number, type: string, mtime: number): Buffer {
  const h = Buffer.alloc(BLOCK, 0);
  // 名字字段只有 100 字节：最多写 100 字节，且不写半个 UTF-8 字符（完整名字另放 PAX 头）。
  // 此前不限长度直接写，超长中文名会一路写进后面的 linkname / uname 字段，文件名也被截成半个字。
  h.write(name, 0, 100, 'utf8');
  h.write('0000644\0', 100); // mode
  h.write(UID_FIELD, 108);
  h.write(GID_FIELD, 116);
  h.write((size > MAX_OCTAL_SIZE ? 0 : size).toString(8).padStart(11, '0') + '\0', 124);
  // mtime 必须写当前时间：写 0 的话上传的文件全是 1970 年，按时间排序时刚上传的反而沉到最底下
  h.write(mtime.toString(8).padStart(11, '0') + '\0', 136);
  h.write('        ', 148); // 校验和占位（8 个空格）
  h.write(type, 156);
  h.write('ustar\0', 257);
  h.write('00', 263);
  let sum = 0;
  for (let i = 0; i < BLOCK; i++) sum += h[i];
  h.write(sum.toString(8).padStart(6, '0') + '\0 ', 148);
  return h;
}

// PAX 记录格式 "长度 key=value\n"，长度含它自己的位数
function paxRecord(key: string, value: string): string {
  const body = ` ${key}=${value}\n`;
  const len = Buffer.byteLength(body, 'utf8');
  let digits = String(len).length;
  if (String(len + digits).length > digits) digits++;
  return `${len + digits}${body}`;
}

// 一个普通文件的头部；名字超过 100 字节或文件超过 8 GiB 时前面加一个 PAX 扩展头（docker 用的 Go archive/tar 支持）
export function tarFileHeader(name: string, size: number, mtime = Math.floor(Date.now() / 1000)): Buffer {
  const records: string[] = [];
  if (Buffer.byteLength(name, 'utf8') > 100) records.push(paxRecord('path', name));
  if (size > MAX_OCTAL_SIZE) records.push(paxRecord('size', String(size)));
  const head = ustarHeader(name, size, '0', mtime);
  if (!records.length) return head;
  const pax = Buffer.from(records.join(''), 'utf8');
  return Buffer.concat([ustarHeader('././@PaxHeader', pax.length, 'x', mtime), pax, pad512(pax.length), head]);
}

// 单个文件的 entry（头部 + 内容 + 对齐填充），不含结尾块
export function tarEntry(name: string, content: Buffer): Buffer {
  return Buffer.concat([tarFileHeader(name, content.length), content, pad512(content.length)]);
}

// 只含一个小文件的完整 tar（壁纸、字体、粘贴的文字 / 图片这类几十 MB 以内的）
export function tarSingleFile(name: string, content: Buffer): Buffer {
  return Buffer.concat([tarEntry(name, content), END_OF_ARCHIVE]);
}

export function tarArchive(entries: Buffer[]): Buffer {
  return Buffer.concat([...entries, END_OF_ARCHIVE]);
}

// 边收边打包：头部 → 数据原样透传 → 填充 → 结尾块。收到的数据和 size 对不上（上传中断）就报错，
// 下游的 putArchive 随之中止，不会把半截文件当成完整文件。
export function tarFileStream(name: string, size: number, body: AsyncIterable<Buffer>): Readable {
  async function* gen() {
    yield tarFileHeader(name, size);
    let got = 0;
    for await (const chunk of body) {
      got += chunk.length;
      if (got > size) throw new TarError('上传的数据比声明的长度多');
      yield chunk;
    }
    if (got !== size) throw new TarError(`上传中断：只收到 ${got} / ${size} 字节`);
    yield pad512(size);
    yield END_OF_ARCHIVE;
  }
  return Readable.from(gen(), { objectMode: false });
}

// ---------- 解析 ----------
export interface TarEntryInfo {
  name: string;
  type: string; // '0' 普通文件 '5' 目录 '2' 软链接 '1' 硬链接 ……
  size: number;
  linkname: string;
}

// 只有头部、没有数据区的类型（Go archive/tar 对这些类型一律按 0 字节数据处理，这里保持一致）
const HEADER_ONLY = new Set(['1', '2', '3', '4', '5', '6']);

class TruncatedError extends TarError {
  constructor() {
    super('压缩包不完整（可能上传中断或文件已损坏）');
  }
}

// 从块流里按字节数取数据；数据区只跳过、不保留
class ByteReader {
  private it: AsyncIterator<Buffer>;
  private buf: Buffer = Buffer.alloc(0);
  private done = false;
  constructor(src: AsyncIterable<Buffer>) {
    this.it = src[Symbol.asyncIterator]();
  }
  private async fill(): Promise<boolean> {
    if (this.done) return false;
    const r = await this.it.next();
    if (r.done) {
      this.done = true;
      return false;
    }
    const chunk = Buffer.isBuffer(r.value) ? r.value : Buffer.from(r.value);
    this.buf = this.buf.length ? Buffer.concat([this.buf, chunk]) : chunk;
    return true;
  }
  // 恰好 n 字节；一个字节都没有就到头了返回 null，读到一半到头抛「不完整」
  async read(n: number): Promise<Buffer | null> {
    while (this.buf.length < n) {
      if (!(await this.fill())) {
        if (!this.buf.length) return null;
        throw new TruncatedError();
      }
    }
    const out = this.buf.subarray(0, n);
    this.buf = this.buf.subarray(n);
    return out;
  }
  async skip(n: number): Promise<void> {
    while (n > 0) {
      if (!this.buf.length && !(await this.fill())) throw new TruncatedError();
      const k = Math.min(n, this.buf.length);
      this.buf = this.buf.subarray(k);
      n -= k;
    }
  }
  async close(): Promise<void> {
    await this.it.return?.().catch(() => undefined);
  }
}

function cstr(b: Buffer): string {
  const i = b.indexOf(0);
  return (i < 0 ? b : b.subarray(0, i)).toString('utf8');
}

function parseNumeric(b: Buffer): number {
  if (b[0] & 0x80) {
    // GNU base-256（大文件）
    let n = b[0] & 0x7f;
    for (let i = 1; i < b.length; i++) n = n * 256 + b[i];
    return n;
  }
  const m = /^[\s\0]*([0-7]*)/.exec(b.toString('latin1'));
  const n = m && m[1] ? parseInt(m[1], 8) : 0;
  if (!Number.isSafeInteger(n)) throw new TarError('tar 头部损坏');
  return n;
}

function checksumOk(h: Buffer): boolean {
  const m = /^[\s\0]*([0-7]+)/.exec(h.toString('latin1', 148, 156));
  if (!m) return false;
  const stored = parseInt(m[1], 8);
  let unsigned = 0;
  let signed = 0; // 个别老工具按有符号字节算
  for (let i = 0; i < BLOCK; i++) {
    const v = i >= 148 && i < 156 ? 32 : h[i];
    unsigned += v;
    signed += v > 127 ? v - 256 : v;
  }
  return stored === unsigned || stored === signed;
}

function parsePax(b: Buffer): Record<string, string> {
  const out: Record<string, string> = {};
  let off = 0;
  while (off < b.length) {
    const sp = b.indexOf(0x20, off);
    if (sp < 0) break;
    const len = parseInt(b.toString('latin1', off, sp), 10);
    if (!(len > 0) || off + len > b.length) throw new TarError('tar 扩展头损坏');
    const rec = b.toString('utf8', sp + 1, off + len - 1); // 去掉结尾换行
    const eq = rec.indexOf('=');
    if (eq > 0) out[rec.slice(0, eq)] = rec.slice(eq + 1);
    off += len;
  }
  return out;
}

// 顺序走一遍 tar：每个条目回调一次（名字已合并 PAX / GNU 长名 / ustar 前缀），数据区直接跳过。
// 到结尾标记（或恰好在块边界上结束，与 docker 的解包行为一致）正常返回；中途断了抛 TarError。
export async function walkTar(src: AsyncIterable<Buffer>, onEntry: (e: TarEntryInfo) => void): Promise<void> {
  const r = new ByteReader(src);
  try {
    let pax: Record<string, string> = {};
    let longName: string | null = null;
    let longLink: string | null = null;
    for (;;) {
      const h = await r.read(BLOCK);
      if (!h || h.equals(ZERO_BLOCK)) return;
      if (!checksumOk(h)) throw new TarError('不是有效的 tar 包（或文件已损坏）');
      const type = h[156] === 0 ? '0' : String.fromCharCode(h[156]);
      let size = parseNumeric(h.subarray(124, 136));
      if (type === 'x' || type === 'g' || type === 'L' || type === 'K') {
        if (size > 8 * 1024 * 1024) throw new TarError('tar 扩展头过大（文件可能已损坏）');
        const data = await r.read(size + padLen(size));
        if (!data) throw new TruncatedError();
        const body = data.subarray(0, size);
        if (type === 'x') Object.assign(pax, parsePax(body));
        else if (type === 'L') longName = cstr(body);
        else if (type === 'K') longLink = cstr(body);
        continue; // 'g' 全局扩展头：docker 解包时也是忽略
      }
      if (pax.size !== undefined) size = Number(pax.size);
      if (!Number.isSafeInteger(size) || size < 0) throw new TarError('tar 头部损坏');
      const prefix = h.toString('latin1', 257, 263) === 'ustar\0' ? cstr(h.subarray(345, 500)) : '';
      const base = cstr(h.subarray(0, 100));
      const name = pax.path ?? longName ?? (prefix ? `${prefix}/${base}` : base);
      const linkname = pax.linkpath ?? longLink ?? cstr(h.subarray(157, 257));
      pax = {};
      longName = null;
      longLink = null;
      const dataLen = HEADER_ONLY.has(type) ? 0 : size;
      onEntry({ name, type, size: dataLen, linkname });
      if (type === 'S' && h[482]) {
        // GNU 旧式稀疏文件：头部后面还跟着若干个稀疏表扩展块
        for (;;) {
          const ext = await r.read(BLOCK);
          if (!ext) throw new TruncatedError();
          if (!ext[504]) break;
        }
      }
      await r.skip(dataLen + padLen(dataLen));
    }
  } finally {
    await r.close();
  }
}

// ---------- 上传的压缩包文件 ----------
export type ArchiveKind = 'tar' | 'gzip' | 'zip' | 'other';

// 看文件头判断格式（不信扩展名）
export async function sniffArchive(path: string): Promise<ArchiveKind> {
  const fh = await open(path, 'r');
  try {
    const head = Buffer.alloc(BLOCK, 0);
    const { bytesRead } = await fh.read(head, 0, BLOCK, 0);
    if (bytesRead >= 2 && head[0] === 0x1f && head[1] === 0x8b) return 'gzip';
    if (bytesRead >= 4 && head[0] === 0x50 && head[1] === 0x4b && (head[2] === 3 || head[2] === 5)) return 'zip';
    if (bytesRead === BLOCK && checksumOk(head)) return 'tar';
    return 'other';
  } finally {
    await fh.close();
  }
}

// 压缩包文件 → 解压后的 tar 字节流（.tar.gz 在这里解开，交给 docker 的始终是普通 tar）
export function openTarStream(path: string, gzip: boolean): Readable {
  const file = createReadStream(path);
  if (!gzip) return file;
  const gunzip = zlib.createGunzip();
  file.on('error', (e) => gunzip.destroy(e));
  gunzip.on('close', () => file.destroy());
  return file.pipe(gunzip);
}

// 校验整个压缩包：走一遍所有条目（gzip 的话连同解压）；gzip 损坏、tar 截断都在这一步报出来
export async function scanArchive(path: string, gzip: boolean, onEntry: (e: TarEntryInfo) => void): Promise<void> {
  const stream = openTarStream(path, gzip);
  try {
    await walkTar(stream, onEntry);
  } catch (e: any) {
    if (e instanceof TarError) throw e;
    if (typeof e?.code === 'string' && e.code.startsWith('Z_')) throw new TarError('压缩包不完整或已损坏（gzip 解压失败）');
    throw e;
  } finally {
    stream.destroy();
  }
}
