import { useEffect, useState } from "react";
import { NavLink, useLocation } from "react-router-dom";
import { openUrl } from "@tauri-apps/plugin-opener";
import {
  LayoutDashboard,
  BookOpen,
  Radio,
  Key,
  ScrollText,
  Settings,
  Settings2,
  Server,
  ChevronRight,
  ExternalLink,
  Link,
  Database,
  LogOut,
} from "lucide-react";
import { serverApi } from "../../lib/api";
import { isWebRuntime } from "../../lib/web";
import type { ServerStatus } from "../../types";
import packageJson from "../../../package.json";

const navItems = [
  { to: "/", icon: LayoutDashboard, label: "仪表盘" },
  { to: "/usage", icon: BookOpen, label: "使用", subLabel: "API、Codex ... 配置" },
  { to: "/channels", icon: Radio, label: "渠道" },
  { to: "/api-keys", icon: Key, label: "密钥" },
  { to: "/services", icon: Database, label: "服务", subLabel: "RAG、Wiki、Skills ..." },
  { to: "/logs", icon: ScrollText, label: "日志" },
  { to: "/settings", icon: Settings, label: "设置" },
];

const githubUrl = "https://github.com/fuzhengwei/WaLiAPI";
const appVersion = packageJson.version;

/** Web 管理面板：清除会话并回到登录页（桌面端不渲染入口）。 */
function webLogout() {
  const token = localStorage.getItem("waliapi_admin_token");
  void fetch("/admin/api/auth/logout", {
    method: "POST",
    headers: {
      "X-Requested-With": "XMLHttpRequest",
      ...(token ? { Authorization: `Bearer ${token}` } : {}),
    },
  }).catch(() => {});
  localStorage.removeItem("waliapi_admin_token");
  location.assign("/login");
}

export function Sidebar({
  hasUpdate,
  onCheckUpdate,
}: {
  hasUpdate: boolean;
  onCheckUpdate: () => void;
}) {
  const [serverStatus, setServerStatus] = useState<ServerStatus | null>(null);
  const location = useLocation();

  useEffect(() => {
    serverApi.getStatus().then(setServerStatus).catch(() => {});
    const interval = setInterval(() => {
      serverApi.getStatus().then(setServerStatus).catch(() => {});
    }, 5000);
    return () => clearInterval(interval);
  }, []);

  return (
    <aside className="w-72 h-screen flex-col border-r border-slate-200 bg-[#eef3f8] px-3 py-3 hidden md:flex">
      <div className="surface rounded-[22px] p-5">
        <div className="flex items-center gap-3">
          <div className="flex h-10 w-10 shrink-0 items-center justify-center rounded-xl bg-white shadow-[0_8px_16px_rgba(47,111,237,0.18)] overflow-hidden">
            <img src="/logo.png" alt="WaLiAPI" className="h-full w-full object-cover" />
          </div>
          <div className="min-w-0 flex-1">
            <div className="flex items-center justify-between gap-2">
              <div className="text-[20px] font-bold tracking-[-0.04em] text-slate-900 leading-none">WaLiAPI</div>
              <button
                onClick={onCheckUpdate}
                className={`relative rounded-full border px-2 py-0.5 text-[10px] font-semibold transition-colors ${
                  hasUpdate
                    ? "border-emerald-300 bg-emerald-50 text-emerald-600"
                    : "border-blue-100 bg-blue-50 text-blue-600 hover:bg-blue-100"
                }`}
              >
                {hasUpdate && (
                  <span className="absolute -right-0.5 -top-0.5 flex h-2.5 w-2.5">
                    <span className="absolute inline-flex h-full w-full animate-ping rounded-full bg-emerald-400 opacity-75" />
                    <span className="relative inline-flex h-2.5 w-2.5 rounded-full bg-emerald-500 ring-2 ring-white" />
                  </span>
                )}
                v{appVersion}
              </button>
            </div>
            <div className="mt-1.5 text-[11px] font-medium text-slate-500">AI 网关 · 统一模型配置和负载</div>
          </div>
        </div>
      </div>

      <nav className="mt-4 flex-1 min-h-0 space-y-1.5 overflow-y-auto">
        {navItems.map(({ to, icon: Icon, label, subLabel }) => (
          <NavLink
            key={to}
            to={to}
            end={to === "/channels"}
            className={({ isActive }) =>
              `group flex items-center gap-3 rounded-2xl px-4 py-3 text-sm transition-colors ${
                isActive || (to === "/" && location.pathname === "/")
                  ? "border border-blue-100 bg-white text-slate-900 shadow-[0_8px_18px_rgba(15,23,42,0.05)]"
                  : "text-slate-600 hover:bg-white/70 hover:text-slate-900"
              }`
            }
          >
            <span className="flex h-9 w-9 items-center justify-center rounded-xl border border-slate-200 bg-white group-hover:bg-slate-50">
              <Icon size={17} />
            </span>
            <span className="font-medium">{label}</span>
            {subLabel && (
              <span className="ml-1 whitespace-nowrap text-[10px] font-normal text-slate-400" style={{ textShadow: "0 1px 1px rgba(0,0,0,0.06), inset 0 0.5px 0 rgba(255,255,255,0.8)" }}>
                ({subLabel})
              </span>
            )}
            <ChevronRight size={15} className="ml-auto opacity-0 transition-opacity group-hover:opacity-40" />
          </NavLink>
        ))}
      </nav>

      <div className="space-y-3">
        <div className="surface-soft rounded-[20px] p-4">
          <div className="mb-3 flex items-center justify-between">
            <div>
              <div className="text-xs text-slate-500">服务状态</div>
              <div className="mt-1 text-sm font-medium text-slate-900">
                {serverStatus?.running ? "运行中" : "未启动"}
              </div>
            </div>
            <div className="flex items-center gap-2">
              <NavLink
                to="/settings#server"
                className="flex h-6 w-6 items-center justify-center rounded-lg text-slate-400 transition-colors hover:bg-white hover:text-slate-700"
                title="服务配置"
              >
                <Settings2 size={14} />
              </NavLink>
              <span className={`h-2.5 w-2.5 rounded-full ${serverStatus?.running ? "bg-emerald-500" : "bg-rose-500"}`} />
            </div>
          </div>
          <div className="flex items-start gap-3 rounded-2xl border border-slate-200 bg-white px-3 py-3 text-xs text-slate-500">
            <Server size={14} className={serverStatus?.running ? "text-emerald-500" : "text-rose-500"} />
            <div className="min-w-0 flex-1">
              <div className="mb-1">API BaseUrl 地址</div>
              <div className="truncate font-mono text-[12px] text-slate-700">
                {serverStatus?.running ? serverStatus.url : "等待服务启动"}
              </div>
            </div>
          </div>
        </div>

        <button
          onClick={() => openUrl(githubUrl)}
          className="flex w-full items-center gap-3 rounded-[18px] border border-slate-200 bg-white/70 px-4 py-3 text-left text-sm text-slate-600 transition-all hover:bg-white hover:text-slate-900 hover:shadow-[0_8px_18px_rgba(15,23,42,0.05)]"
        >
          <span className="flex h-9 w-9 items-center justify-center rounded-xl border border-slate-200 bg-white">
            <Link size={17} />
          </span>
          <span className="min-w-0 flex-1">
            <span className="block font-medium">GitHub 开源仓库</span>
            <span className="block truncate text-xs text-slate-500">github.com/fuzhengwei/WaLiAPI</span>
          </span>
          <ExternalLink size={14} className="text-slate-400" />
        </button>

        {isWebRuntime() && (
          <button
            onClick={webLogout}
            className="flex w-full items-center gap-3 rounded-[18px] border border-slate-200 bg-white/70 px-4 py-3 text-left text-sm text-slate-600 transition-all hover:bg-white hover:text-rose-600 hover:shadow-[0_8px_18px_rgba(15,23,42,0.05)]"
          >
            <span className="flex h-9 w-9 items-center justify-center rounded-xl border border-slate-200 bg-white">
              <LogOut size={17} />
            </span>
            <span className="min-w-0 flex-1 font-medium">退出登录</span>
          </button>
        )}
      </div>
    </aside>
  );
}
