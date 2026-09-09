// 飞牛「应用开放 API」前端封装（@trimjs/web-app）。
//
// 只有在 fnOS 桌面/手机宿主内（iframe 且宿主注入了微应用桥）时 pickSharedFile 才可用；
// 独立浏览器直连端口时 isStandaloneWeb=true，桥不存在 —— 此时降级为「手动进 fnOS 应用
// 设置授权」的旧路径，UI 据 detectHost() 的结果决定显示哪个入口，绝不静默失败。
import { TrimApp } from '@trimjs/web-app';

export type TrimHostKind =
  | 'fnos-iframe' // fnOS 桌面 iframe（宿主桥可用）→ 直接 pickSharedFile
  | 'fnos-standalone' // 独立浏览器直连（无宿主桥）→ 只能手动授权
  | 'non-fnos'; // SDK 初始化失败/非飞牛宿主

let app: TrimApp | null = null;
let initPromise: Promise<TrimHostKind> | null = null;

async function init(): Promise<TrimHostKind> {
  try {
    app = new TrimApp();
    await app.ready();
    if (app.isWeb && !app.isStandaloneWeb) return 'fnos-iframe';
    return 'fnos-standalone';
  } catch {
    app = null;
    return 'non-fnos';
  }
}

/** 判定当前运行宿主（结果缓存，只初始化一次）。 */
export function detectHost(): Promise<TrimHostKind> {
  if (!initPromise) initPromise = init();
  return initPromise;
}

export interface PickSharedResult {
  ok: boolean;
  paths: string[];
  /** 用户在目录选择器里点了取消/关闭 —— 不是错误，别当失败提示 */
  cancelled?: boolean;
  error?: string;
}

/**
 * 打开飞牛目录选择器**纯选**目录（不授权）。
 *
 * 用 pickFile（而非 pickSharedFile）：pickSharedFile 是「选+授权」原子操作，且会**排除
 * 已授权的目录**（无法重选）；pickFile 纯选不授权，所以任何目录都能选到。是否已授权由
 * 调用方交给后端 /admin/fnos-shared-folders 判定，未授权时弹提示。
 *
 * 用户点取消时返回 {ok:false, cancelled:true}；返回 {ok:false} 且非取消时调用方应回落到
 * 手填路径引导，不要抛错打断面板。
 */
export async function pickSharedFolder(): Promise<PickSharedResult> {
  const kind = await detectHost();
  if (kind !== 'fnos-iframe' || !app) {
    return { ok: false, paths: [], error: '当前不是飞牛桌面宿主环境，请手填绝对路径' };
  }
  try {
    const resp = await app.pickFile({
      title: '选择微信数据存放目录',
      directory: true,
      creatable: true,
    });
    if (!resp) return { ok: false, paths: [], error: '未获得宿主响应' };
    // pickFile 返回 string[]；可能为空数组（用户点了确定但没选中）
    if (!Array.isArray(resp) || resp.length === 0) {
      return { ok: false, paths: [], cancelled: true, error: '已取消或未选中目录' };
    }
    return { ok: true, paths: resp };
  } catch (e: any) {
    return { ok: false, paths: [], error: String(e?.message || e) };
  }
}

/**
 * 对已知路径补授权（目录此前已授权但 ACL 被清、或用户在输入框手填了已授权目录时可用）。
 * 飞牛选择器无法重选已授权目录，本方法就是那条路径。已授权时飞牛会直接返回成功（幂等）。
 */
export async function authorizeSharedFolder(path: string): Promise<{ ok: boolean; error?: string }> {
  const kind = await detectHost();
  if (kind !== 'fnos-iframe' || !app) {
    return { ok: false, error: '当前不是飞牛桌面宿主环境' };
  }
  try {
    const resp = await app.authorizeSharedFile(path);
    if (!resp) return { ok: false, error: '未获得宿主响应' };
    if (resp.code !== 0) return { ok: false, error: resp.msg || '授权失败' };
    return { ok: true };
  } catch (e: any) {
    return { ok: false, error: String(e?.message || e) };
  }
}
