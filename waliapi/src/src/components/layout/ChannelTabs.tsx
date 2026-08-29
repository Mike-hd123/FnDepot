import { useEffect, useState } from "react";
import { NavLink } from "react-router-dom";
import { CircleDot, KeyRound } from "lucide-react";
import { authApi, channelApi } from "../../lib/api";

/**
 * 渠道管理页顶部 API / Auth 双 Tab（对齐 prototype 的 underline tab-bar）：
 * 全宽底边框 + 图标 + 完整标签 + 数量徽标，路由 /channels 与 /channels/auth。
 * 数量自行轻量拉取，保证两个 tab 徽标在任何页面都显示。
 */
export function ChannelTabs() {
  const [channelCount, setChannelCount] = useState<number | null>(null);
  const [authCount, setAuthCount] = useState<number | null>(null);

  useEffect(() => {
    channelApi.getAll().then(cs => setChannelCount(cs.length)).catch(() => {});
    authApi.accountsList().then(as => setAuthCount(as.length)).catch(() => {});
  }, []);

  const base =
    "inline-flex items-center gap-1.5 whitespace-nowrap border-b-2 px-3 py-1.5 text-sm font-medium transition-colors";

  return (
    <nav className="flex w-full items-center gap-1 overflow-x-auto border-b border-border" aria-label="渠道视图切换">
      <NavLink
        to="/channels"
        end
        className={({ isActive }) =>
          `${base} -mb-px ${isActive ? "border-primary text-primary" : "border-transparent text-muted-foreground hover:border-border hover:text-foreground"}`
        }
      >
        <CircleDot size={15} />
        API 渠道
        {channelCount !== null && (
          <span className="rounded-full bg-blue-50 px-1.5 py-px text-[10px] font-bold leading-4 text-blue-600 tabular-nums">
            {channelCount}
          </span>
        )}
      </NavLink>
      <NavLink
          to="/channels/auth"
          className={({ isActive }) =>
            `${base} -mb-px ${isActive ? "border-primary text-primary" : "border-transparent text-muted-foreground hover:border-border hover:text-foreground"}`
          }
        >
          <KeyRound size={15} />
          Auth 账号
          {authCount !== null && (
            <span className="rounded-full bg-blue-50 px-1.5 py-px text-[10px] font-bold leading-4 text-blue-600 tabular-nums">
              {authCount}
            </span>
          )}
      </NavLink>
    </nav>
  );
}
