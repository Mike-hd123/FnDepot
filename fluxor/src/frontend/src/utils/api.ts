// mock 仅在 DEV 下动态加载：生产构建中 `import.meta.env.DEV` 静态折叠为 false，
// 整个 mock 模块被摇树移除，杜绝离线模拟器进入生产包（安全）
const mock = import.meta.env.DEV ? await import('./mock') : null;

const rawBase = window.BASE_URL || import.meta.env.BASE_URL || '';
// 根路径部署时 import.meta.env.BASE_URL 为 './'，须归一化为空串，
// 否则 withBase 会拼出 '/./configs' 这类非法路径
const BASE = rawBase === './' || rawBase === '.' ? '' : rawBase;

function isMockEnabled(): boolean {
  return mock !== null && localStorage.getItem('MOCK_BACKEND') !== 'false';
}

export function withBase(path: string): string {
  if (!BASE || BASE === '/') return path;
  const base = BASE.replace(/^\/|\/$/g, '');
  return '/' + base + '/' + path.replace(/^\//, '');
}

export async function apiFetch(path: string, options: RequestInit = {}): Promise<Response> {
  if (isMockEnabled() && mock) {
    // 模拟网络延迟（50-150ms 仿真）
    await new Promise(resolve => setTimeout(resolve, 50 + Math.random() * 100));
    return mock.handleMockFetch(path, options);
  }
  try {
    const url = withBase(path);
    const resp = await fetch(url, options);
    return resp;
  } catch (err: any) {
    throw new Error('网络错误: ' + err.message);
  }
}

export interface WsHandlers {
  onOpen?: () => void;
  onError?: (ev: Event) => void;
  onClose?: (ev: CloseEvent) => void;
}

export function wsConnect(
  path: string,
  onMessage: (ev: MessageEvent) => void,
  handlers: WsHandlers = {}
): WebSocket {
  if (isMockEnabled() && mock) {
    const ws = new mock.MockWebSocket('', path) as any;
    ws.onopen = handlers.onOpen || null;
    ws.onclose = handlers.onClose || null;
    ws.onerror = handlers.onError || null;
    ws.onmessage = (e: any) => {
      try {
        onMessage(e);
      } catch (err) {
        // ignore
      }
    };
    return ws;
  }

  const wsUrl = (location.protocol === 'https:' ? 'wss://' : 'ws://') + location.host + withBase(path);
  const ws = new WebSocket(wsUrl);

  // 5秒握手超时计时器
  let isOpened = false;
  const timer = setTimeout(() => {
    if (!isOpened && ws.readyState === WebSocket.CONNECTING) {
      ws.close();
    }
  }, 5000);

  ws.onopen = () => {
    isOpened = true;
    clearTimeout(timer);
    if (handlers.onOpen) handlers.onOpen();
  };

  ws.onmessage = (e) => {
    try {
      onMessage(e);
    } catch (err) {
      // ignore
    }
  };

  ws.onerror = (e) => {
    clearTimeout(timer);
    if (handlers.onError) handlers.onError(e);
  };

  ws.onclose = (e) => {
    clearTimeout(timer);
    if (handlers.onClose) handlers.onClose(e);
  };

  return ws;
}
