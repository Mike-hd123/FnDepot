// Host-header allowlist for DNS-rebinding protection.
//
// Background: the panel binds 0.0.0.0:8080 and ships default credentials
// (admin / wechat). Without Host-header validation, a malicious site the
// operator visits can use DNS rebinding to point a hostname at the panel's
// LAN/loopback IP and drive every authenticated API from the operator's own
// browser — including the docker.sock-backed admin endpoints. The
// `sameSite: 'lax'` cookie does not stop this: after rebinding, the browser
// treats the attacker hostname as same-origin with the panel and includes
// any cookie it issues. The fix is host-allowlisting at the request edge.
//
// Default allowlist (covers documented deploys without operator action):
//   - loopback: localhost / 127.0.0.1 / ::1
//   - RFC1918 private LAN: 10/8, 172.16-31/12, 192.168/16
//   - link-local IPv4: 169.254/16
// Public hostnames (the recommended reverse-proxy deployment) must be added
// via PANEL_ALLOWED_HOSTS=<comma-separated>.

export function parseHost(headerHost: string | undefined): string {
  if (!headerHost) return '';
  const trimmed = headerHost.trim();
  if (!trimmed) return '';
  if (trimmed.startsWith('[')) {
    const close = trimmed.indexOf(']');
    if (close <= 0) return '';
    return trimmed.slice(0, close + 1).toLowerCase();
  }
  const colon = trimmed.lastIndexOf(':');
  const host = colon > 0 ? trimmed.slice(0, colon) : trimmed;
  return host.toLowerCase();
}

export function isLoopbackHost(host: string): boolean {
  return (
    host === 'localhost' ||
    host === '127.0.0.1' ||
    host === '[::1]' ||
    host === '::1'
  );
}

export function isPrivateIpv4(host: string): boolean {
  const m = host.match(/^(\d{1,3})\.(\d{1,3})\.(\d{1,3})\.(\d{1,3})$/);
  if (!m) return false;
  const o = [m[1], m[2], m[3], m[4]].map((s) => Number(s));
  if (o.some((n) => Number.isNaN(n) || n < 0 || n > 255)) return false;
  // 10.0.0.0/8
  if (o[0] === 10) return true;
  // 172.16.0.0/12
  if (o[0] === 172 && o[1] >= 16 && o[1] <= 31) return true;
  // 192.168.0.0/16
  if (o[0] === 192 && o[1] === 168) return true;
  // 169.254.0.0/16 (link-local)
  if (o[0] === 169 && o[1] === 254) return true;
  // 100.64.0.0/10 (CGNAT)：Tailscale/Headscale 给节点分配的就是这一段，
  // 组网后用 100.x.y.z 访问面板属于「等价于内网」的正常用法，此前被误拒（issue #124 评论）。
  if (o[0] === 100 && o[1] >= 64 && o[1] <= 127) return true;
  return false;
}

// 内网 IPv6 字面量。此前只放行 ::1，导致纯 IPv6 内网 / Tailscale IPv6（fd7a:115c:a1e0::/48）
// 访问面板一律 400。覆盖 fc00::/7（ULA，含 Tailscale）与 fe80::/10（链路本地）。
// 安全性同 RFC1918：校验的是 Host 头字面量，DNS-rebinding 攻击者的 Host 是自己的域名，匹配不上。
export function isPrivateIpv6(host: string): boolean {
  let h = host;
  if (h.startsWith('[') && h.endsWith(']')) h = h.slice(1, -1);
  const zone = h.indexOf('%'); // fe80::1%eth0 的 zone id
  if (zone >= 0) h = h.slice(0, zone);
  h = h.toLowerCase();
  if (!h.includes(':') || !/^[0-9a-f:]+$/.test(h)) return false;
  const first = h.split(':')[0];
  if (!first) return false; // "::1" 这类由 isLoopbackHost 负责
  // IPv6 hextet 省略前导零，故 "fd" 实为 00fd（不是 ULA）——补零后再取高字节才正确
  const padded = first.padStart(4, '0');
  const hi = parseInt(padded.slice(0, 2), 16);
  if (Number.isNaN(hi)) return false;
  if (hi === 0xfc || hi === 0xfd) return true; // fc00::/7 ULA
  if (hi === 0xfe) {
    const lo = parseInt(padded.slice(2, 4), 16);
    return lo >= 0x80 && lo <= 0xbf; // fe80::/10 链路本地
  }
  return false;
}

export function parseAllowedHosts(raw: string | undefined): string[] {
  if (!raw) return [];
  const out: string[] = [];
  for (const part of raw.split(',')) {
    const lower = part.trim().toLowerCase();
    if (lower) out.push(lower);
  }
  return [...new Set(out)];
}

export function isAllowedHost(host: string, allowlist: string[]): boolean {
  if (!host) return false;
  if (isLoopbackHost(host)) return true;
  if (isPrivateIpv4(host)) return true;
  if (isPrivateIpv6(host)) return true;
  for (const entry of allowlist) {
    if (entry === host) return true;
    // 通配子域：*.example.com 匹配任意子域（a.example.com），但不匹配裸 example.com。
    if (entry.startsWith('*.')) {
      const suffix = entry.slice(1); // ".example.com"
      if (host.length > suffix.length && host.endsWith(suffix)) return true;
    }
  }
  return false;
}

// 反代/CDN（Cloudflare、nginx、Caddy 等）部署时，真实对外域名可能在 X-Forwarded-Host 里，
// 而 Host 被改写成内部地址。综合判定：Host 或 X-Forwarded-Host 任一在白名单即放行。
// 安全性：DNS-rebinding 攻击者直连面板时，浏览器 fetch 无法设置 X-Forwarded-Host（禁止首部），
// 故该首部只会由可信反代设置，不会被攻击者利用。
export function isRequestHostAllowed(
  hostHeader: string | undefined,
  forwardedHostHeader: string | string[] | undefined,
  allowlist: string[],
): boolean {
  if (isAllowedHost(parseHost(hostHeader), allowlist)) return true;
  let xfh = Array.isArray(forwardedHostHeader) ? forwardedHostHeader[0] : forwardedHostHeader;
  if (xfh) {
    xfh = xfh.split(',')[0]; // 多级代理链取第一个（最初的客户端 Host）
    if (isAllowedHost(parseHost(xfh), allowlist)) return true;
  }
  return false;
}
