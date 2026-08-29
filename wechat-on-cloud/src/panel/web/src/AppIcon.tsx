import type { AppType } from './api';

// 实例图标。支持三种来源（优先级从高到低）：
//   1) 自定义上传/裁剪的图片 → inst.icon = "data:image/...;base64,..."
//   2) 内置图标 → inst.icon = "builtin:<key>"（如 builtin:xiaohongshu）
//   3) 缺省：按 appType 给默认图标（微信 / Chromium / Telegram / 通用）
// 内置图标用简洁 SVG（彩色圆角块 + 白色字形），风格统一、无需联网抓取。后续可往 BUILTIN 里加更多平台。

type Glyph = { bg: string; el: JSX.Element };
const G = (bg: string, el: JSX.Element): Glyph => ({ bg, el });

// 白色字形（viewBox 0 0 48 48，置于彩色圆角块上）
const chat = (
  <path
    fill="#fff"
    d="M19 12c-6.6 0-12 4.2-12 9.5 0 3 1.8 5.7 4.6 7.4l-1.1 3.9 4.4-2.3c1.3.3 2.7.5 4.1.5 6.6 0 12-4.2 12-9.5S25.6 12 19 12zm-4 8.2a1.6 1.6 0 110-3.2 1.6 1.6 0 010 3.2zm8 0a1.6 1.6 0 110-3.2 1.6 1.6 0 010 3.2z"
  />
);
const globe = (
  <g fill="none" stroke="#fff" strokeWidth="2.4">
    <circle cx="24" cy="24" r="13" />
    <ellipse cx="24" cy="24" rx="5.5" ry="13" />
    <path d="M11.5 20h25M11.5 28h25" />
  </g>
);
const plane = <path fill="#fff" d="M35 14L13 23.2l6.1 2.2 2.3 7.2 3.3-3.9 5.6 4.1L35 14zm-12.4 12.6l9-7.2-6.7 8.1-.1 3.6-2.2-4.5z" />;
const dots = (
  <g fill="#fff">
    <circle cx="16" cy="24" r="2.6" />
    <circle cx="24" cy="24" r="2.6" />
    <circle cx="32" cy="24" r="2.6" />
  </g>
);

// 文字字形（品牌色块 + 白字），用于没有简单标志的平台
const txt = (s: string, fs = 22) => (
  <text x="24" y="25" fill="#fff" fontSize={fs} fontWeight="700" textAnchor="middle" dominantBaseline="central" fontFamily="-apple-system, system-ui, sans-serif">
    {s}
  </text>
);
const play = <path fill="#fff" d="M20 17l12 7-12 7z" />;

// key → 字形。default-by-appType 与「内置图标选择器」共用同一张表。
export const BUILTIN_ICONS: Record<string, Glyph> = {
  wechat: G('#07c160', chat),
  chromium: G('#4285f4', globe),
  telegram: G('#2aabee', plane),
  xiaohongshu: G('#ff2442', txt('书')),
  douyin: G('#111111', txt('抖')),
  bilibili: G('#fb7299', txt('B', 26)),
  weibo: G('#e6162d', txt('微')),
  zhihu: G('#0084ff', txt('知')),
  youtube: G('#ff0000', play),
  globe: G('#5b8def', globe),
  app: G('#8a9099', dots),
};
// 「内置图标」选择器里展示的可选项（顺序即展示顺序）
export const ICON_CHOICES: { key: string; label: string }[] = [
  { key: 'wechat', label: '微信' },
  { key: 'chromium', label: 'Chromium' },
  { key: 'telegram', label: 'Telegram' },
  { key: 'xiaohongshu', label: '小红书' },
  { key: 'douyin', label: '抖音' },
  { key: 'bilibili', label: 'B站' },
  { key: 'weibo', label: '微博' },
  { key: 'zhihu', label: '知乎' },
  { key: 'youtube', label: 'YouTube' },
  { key: 'globe', label: '通用' },
];
const DEFAULT_BY_APP: Record<AppType, string> = {
  wechat: 'wechat',
  chromium: 'chromium',
  telegram: 'telegram',
  custom: 'app',
};

export function InstanceIcon({
  icon,
  appType,
  size = 36,
  radius = 12,
}: {
  icon?: string;
  appType?: AppType;
  size?: number;
  radius?: number;
}) {
  // 1) 自定义图片
  if (icon && icon.startsWith('data:')) {
    return <img src={icon} width={size} height={size} alt="" style={{ borderRadius: radius, objectFit: 'cover', display: 'block' }} />;
  }
  // 2) 内置 / 3) 默认
  const key = icon && icon.startsWith('builtin:') ? icon.slice(8) : DEFAULT_BY_APP[appType ?? 'wechat'] ?? 'app';
  const g = BUILTIN_ICONS[key] ?? BUILTIN_ICONS.app;
  return (
    <svg width={size} height={size} viewBox="0 0 48 48" style={{ display: 'block' }} aria-hidden="true">
      <rect width="48" height="48" rx={(radius / size) * 48} fill={g.bg} />
      {g.el}
    </svg>
  );
}
