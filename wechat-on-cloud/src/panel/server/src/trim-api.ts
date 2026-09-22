// 飞牛「应用开放 API」最小客户端封装（经系统 Unix socket 直调 trim 网关）。
//
// 鉴权三件套（缺一即 403/401）：
//   1. 进程运行用户在 TrimApiUsers 组（cmd/install_callback 幂等补组）
//   2. Authorization: Bearer ${TRIM_API_TOKEN}（trim_app_center 启动应用时注入 env，
//      **每次现读 process.env，禁止落盘/缓存到文件/透传前端**）
//   3. 包 config/resource 声明 "api-scope"（本应用：trim.file.sharedAccess）
import http from 'node:http';

const SOCKET_PATH = process.env.TRIM_OPEN_API_SOCKET || '/var/run/trim_open_gateway_apiscope.socket';
// appName 用安装后的应用标识（appcenter 以 appname 签发 token/scope），env 可覆盖以便测试
const APP_NAME = process.env.TRIM_APPNAME || 'wechat-on-cloud';

export interface TrimApiResponse<T = unknown> {
  reqId?: string;
  code: number; // 0 成功；200001 参数错 / 200003 Forbidden(scope或组) / 200004 Unauthorized(token) / 200005 Not Found
  msg?: string;
  data?: T;
}

/** 面板进程当前是否具备调用条件（env 里有注入的 token）。*/
export function trimApiAvailable(): boolean {
  return !!(process.env.TRIM_API_TOKEN || '').trim();
}

let reqSeq = 0;

/**
 * 调一次飞牛开放 API。req 形如 'trim.file.getSharedAccessibleFolders'。
 * 超时/网络错以异常抛出；业务错误按响应 code 原样返回，由调用方判断。
 */
export function trimApi<T = unknown>(req: string, data: Record<string, unknown> = {}, timeoutMs = 10000): Promise<TrimApiResponse<T>> {
  return new Promise((resolve, reject) => {
    const token = (process.env.TRIM_API_TOKEN || '').trim();
    if (!token) {
      reject(new Error('TRIM_API_TOKEN 未注入（需 fnOS 以应用中心方式启动面板，且包已声明 api-scope）'));
      return;
    }
    const body = JSON.stringify({ reqId: String(++reqSeq), req, appName: APP_NAME, data });
    const r = http.request(
      {
        socketPath: SOCKET_PATH,
        path: '/api/v1/trimapp',
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
          'Content-Length': Buffer.byteLength(body),
          Authorization: `Bearer ${token}`,
        },
        timeout: timeoutMs,
      },
      (res) => {
        const chunks: Buffer[] = [];
        res.on('data', (c) => chunks.push(c as Buffer));
        res.on('end', () => {
          try {
            const parsed = JSON.parse(Buffer.concat(chunks).toString('utf8')) as TrimApiResponse<T>;
            resolve(parsed);
          } catch (e) {
            reject(new Error(`飞牛开放 API 响应解析失败 (HTTP ${res.statusCode}): ${e}`));
          }
        });
      },
    );
    r.on('timeout', () => {
      r.destroy(new Error('飞牛开放 API 请求超时'));
    });
    r.on('error', reject);
    r.end(body);
  });
}

/** 共享授权目录列表（scope: trim.file.sharedAccess）。返回绝对路径数组。 */
export async function getSharedAccessibleFolders(): Promise<string[]> {
  const resp = await trimApi<{ paths?: string[] }>('trim.file.getSharedAccessibleFolders');
  if (resp.code !== 0) {
    throw new Error(`getSharedAccessibleFolders 失败: code=${resp.code} ${resp.msg || ''}`.trim());
  }
  return Array.isArray(resp.data?.paths) ? resp.data!.paths! : [];
}

/** 删除一条共享授权（scope: trim.file.sharedAccess）。 */
export async function delSharedAccessibleFolder(path: string): Promise<void> {
  const resp = await trimApi('trim.file.delSharedAccessibleFolder', { path });
  if (resp.code !== 0) {
    throw new Error(`delSharedAccessibleFolder 失败: code=${resp.code} ${resp.msg || ''}`.trim());
  }
}
