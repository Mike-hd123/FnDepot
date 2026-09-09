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
  error?: string;
}

/**
 * 打开飞牛目录选择器，选中的目录授权给本应用（管理员专用，单选目录）。
 * 返回 {ok:false} 时调用方应回落到手动授权引导，不要抛错打断面板。
 */
export async function pickSharedFolder(): Promise<PickSharedResult> {
  const kind = await detectHost();
  if (kind !== 'fnos-iframe' || !app) {
    return { ok: false, paths: [], error: '当前不是飞牛桌面宿主环境，请在 fnOS 应用设置里手动授权目录' };
  }
  try {
    const resp = await app.pickSharedFile({
      title: '选择微信数据存放目录',
      okText: '授权',
      creatable: true,
    });
    if (!resp) return { ok: false, paths: [], error: '未获得宿主响应' };
    if (resp.code !== 0 || !Array.isArray(resp.data) || resp.data.length === 0) {
      return { ok: false, paths: [], error: resp.msg || '未选择目录或无管理员权限' };
    }
    return { ok: true, paths: resp.data };
  } catch (e: any) {
    return { ok: false, paths: [], error: String(e?.message || e) };
  }
}

/** 对已知路径重新申请授权（用户此前在设置里手动授权过时可用）。 */
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
