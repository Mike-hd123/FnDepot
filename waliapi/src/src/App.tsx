import { lazy, Suspense, useEffect, useState } from "react";
import { BrowserRouter, Routes, Route, Navigate } from "react-router-dom";
import { Layout } from "./components/layout/Layout";
import { settingsApi } from "./lib/api";
import { isTauriRuntime } from "./lib/runtime";
import { WebAdminGate } from "./components/WebAdminGate";

const DashboardPage = lazy(() => import("./pages/DashboardPage").then(module => ({ default: module.DashboardPage })));
const ChannelsPage = lazy(() => import("./pages/ChannelsPage").then(module => ({ default: module.ChannelsPage })));
const AuthChannelsPage = lazy(() => import("./pages/AuthChannelsPage").then(module => ({ default: module.AuthChannelsPage })));
const ApiKeysPage = lazy(() => import("./pages/ApiKeysPage").then(module => ({ default: module.ApiKeysPage })));
const LogsPage = lazy(() => import("./pages/LogsPage").then(module => ({ default: module.LogsPage })));
const SettingsPage = lazy(() => import("./pages/SettingsPage").then(module => ({ default: module.SettingsPage })));
const UsagePage = lazy(() => import("./pages/UsagePage").then(module => ({ default: module.UsagePage })));
const KnowledgeBasePage = lazy(() => import("./pages/KnowledgeBasePage").then(module => ({ default: module.KnowledgeBasePage })));
const UpdateChecker = lazy(() => import("./components/UpdateChecker").then(module => ({ default: module.UpdateChecker })));

function App() {
  const [showUpdater, setShowUpdater] = useState(false);
  // 全局:是否有新版本可用(用户点"稍后"后仍保留,用于侧边栏红点提示)
  const [hasUpdate, setHasUpdate] = useState(false);

  useEffect(() => {
    settingsApi.get().then((settings) => {
      document.documentElement.setAttribute("data-theme", settings.ui_theme || "dark");
      document.documentElement.lang = settings.ui_language || "zh-CN";
    }).catch(() => {});

    // 启动 5 秒后静默检查更新,发现新版本标记 hasUpdate + 弹窗
    if (!isTauriRuntime()) return;
    const timer = setTimeout(() => {
      import("@tauri-apps/plugin-updater").then(({ check }) => check())
        .then((update) => {
          if (update) {
            setHasUpdate(true);
            setShowUpdater(true);
          }
        })
        .catch(() => {
          // 检查失败(网络问题/无 release)时静默忽略,不影响使用
        });
    }, 5000);
    return () => clearTimeout(timer);
  }, []);

  return (
    <WebAdminGate>
      <BrowserRouter>
        <Layout hasUpdate={hasUpdate} onCheckUpdate={() => isTauriRuntime() && setShowUpdater(true)}>
          <Suspense fallback={<div className="page-shell text-sm text-muted-foreground">加载中...</div>}>
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
          </Suspense>
        </Layout>
        {isTauriRuntime() && showUpdater && (
          <Suspense fallback={null}>
            <UpdateChecker
              onClose={() => setShowUpdater(false)}
              onUpdateStarted={() => setHasUpdate(false)}
            />
          </Suspense>
        )}
      </BrowserRouter>
    </WebAdminGate>
  );
}

export default App;
