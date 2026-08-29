/**
 * `@tauri-apps/api/event` 的 Web 替身。
 * 单例 EventSource 连接 /admin/api/events（Cookie 鉴权），按事件名分发。
 * 签名与 Tauri `listen` 一致：返回 Promise<unlisten>。
 */

export interface Event<T> {
  payload: T;
}

export type EventCallback<T> = (event: Event<T>) => void;
export type UnlistenFn = () => void;

let source: EventSource | null = null;
const listeners = new Map<string, Set<EventCallback<unknown>>>();

function ensureSource(): EventSource {
  if (source) return source;
  const es = new EventSource("/admin/api/events");
  es.onerror = () => {
    // EventSource 默认自动重连；会话过期时服务端返回 401，浏览器会持续重试，
    // 登录跳转由 invoke shim 处理，这里无需额外动作。
  };
  source = es;
  return es;
}

function addListener(event: string, cb: EventCallback<unknown>) {
  const es = ensureSource();
  let set = listeners.get(event);
  if (!set) {
    set = new Set();
    listeners.set(event, set);
    es.addEventListener(event, (e: MessageEvent) => {
      let payload: unknown = null;
      try {
        payload = JSON.parse(e.data);
      } catch {
        payload = e.data;
      }
      set?.forEach((fn) => fn({ payload }));
    });
  }
  set.add(cb);
}

function removeListener(event: string, cb: EventCallback<unknown>) {
  const set = listeners.get(event);
  if (!set) return;
  set.delete(cb);
  if (set.size === 0) listeners.delete(event);
  if (listeners.size === 0 && source) {
    source.close();
    source = null;
  }
}

export function listen<T>(event: string, handler: EventCallback<T>): Promise<UnlistenFn> {
  const cb = handler as EventCallback<unknown>;
  addListener(event, cb);
  return Promise.resolve(() => removeListener(event, cb));
}
