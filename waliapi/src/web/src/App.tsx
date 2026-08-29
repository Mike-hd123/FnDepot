import { useEffect, useState, type ReactNode } from "react";
import { BrowserRouter, Routes, Route, Navigate, useLocation } from "react-router-dom";
import { Layout } from "@app/components/layout/Layout";
import { DashboardPage } from "@app/pages/DashboardPage";
import { ChannelsPage } from "@app/pages/ChannelsPage";
import { AuthChannelsPage } from "@app/pages/AuthChannelsPage";
import { ApiKeysPage } from "@app/pages/ApiKeysPage";
import { LogsPage } from "@app/pages/LogsPage";
import { SettingsPage } from "@app/pages/SettingsPage";
import { UsagePage } from "@app/pages/UsagePage";
import { KnowledgeBasePage } from "@app/pages/KnowledgeBasePage";
import { settingsApi } from "@app/lib/api";
import { checkAuth, getToken } from "./lib/auth";
import { LoginPage } from "./pages/LoginPage";
import { ChangePasswordPage } from "./pages/ChangePasswordPage";

function RequireAuth({ children }: { children: ReactNode }) {
  const location = useLocation();
  const [state, setState] = useState<"loading" | "ok" | "login" | "change-password">("loading");

  useEffect(() => {
    let active = true;
    if (!getToken()) {
      setState("login");
      return;
    }
    checkAuth()
      .then((info) => {
        if (!active) return;
        if (!info) setState("login");
        else setState(info.must_change_password ? "change-password" : "ok");
      })
      .catch(() => {
        if (active) setState("ok"); // 网络抖动时不强制登出
      });
    return () => {
      active = false;
    };
  }, []);

  if (state === "loading") {
    return (
      <div className="flex h-screen items-center justify-center text-sm text-slate-500">
        正在验证会话…
      </div>
    );
  }
  if (state === "login") {
    return <Navigate to="/login" replace state={{ from: location.pathname }} />;
  }
  if (state === "change-password") {
    return <Navigate to="/change-password" replace />;
  }
  return <>{children}</>;
}

function App() {
  useEffect(() => {
    settingsApi
      .get()
      .then((settings) => {
        document.documentElement.setAttribute("data-theme", settings.ui_theme || "dark");
        document.documentElement.lang = settings.ui_language || "zh-CN";
      })
      .catch(() => {});
  }, []);

  return (
    <BrowserRouter>
      <Routes>
        <Route path="/login" element={<LoginPage />} />
        <Route path="/change-password" element={<ChangePasswordPage />} />
        <Route
          path="/*"
          element={
            <RequireAuth>
              <Layout hasUpdate={false} onCheckUpdate={() => {}}>
                <Routes>
                  <Route path="/" element={<DashboardPage />} />
                  <Route path="/usage" element={<UsagePage />} />
                  <Route path="/channels" element={<ChannelsPage />} />
                  <Route path="/channels/auth" element={<AuthChannelsPage />} />
                  <Route path="/api-keys" element={<ApiKeysPage />} />
                  <Route path="/logs" element={<LogsPage />} />
                  <Route path="/settings" element={<SettingsPage />} />
                  <Route path="/services" element={<KnowledgeBasePage />} />
                  <Route path="/services/knowledge-base" element={<KnowledgeBasePage />} />
                  <Route path="/services/mcp" element={<KnowledgeBasePage />} />
                  <Route path="/services/wiki" element={<KnowledgeBasePage />} />
                  <Route path="/services/skills" element={<KnowledgeBasePage />} />
                  <Route path="*" element={<Navigate to="/" replace />} />
                </Routes>
              </Layout>
            </RequireAuth>
          }
        />
      </Routes>
    </BrowserRouter>
  );
}

export default App;
