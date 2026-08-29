import { useEffect, useState, useMemo, useRef } from "react";
import { useNavigate } from "react-router-dom";
import { Prism as SyntaxHighlighter } from "react-syntax-highlighter";
import { oneDark } from "react-syntax-highlighter/dist/esm/styles/prism";
import { channelApi, apiKeyApi, serverApi, authApi } from "../lib/api";
import type { Channel, ApiKey, ServerStatus, AuthAccount } from "../types";
import { isTauriRuntime, writeClipboard } from "../lib/runtime";
import { AppConfigPanel, getAppIcon } from "./AppConfigPage";
import {
  BookOpen, Copy, Check, Play, Loader2, Link2, KeyRound, Bot,
  ChevronDown, Terminal, Code2, Coffee, Zap, ArrowRight,
  MessageSquare, Layers, Sparkles, AlertCircle,
} from "lucide-react";

type Platform = "curl-mac" | "curl-windows" | "javascript" | "typescript" | "java";
type TestState = "idle" | "running" | "success" | "error";
type Protocol = "chat" | "responses" | "anthropic";
type UsageTab = "api" | "claude-code" | "codex" | "gemini-cli" | "claude-desktop" | "opencode" | "openclaw" | "hermes" | "walicode";

const APP_TABS: { id: UsageTab; label: string }[] = [
  { id: "codex", label: "Codex" },
  { id: "claude-code", label: "Claude Code" },
  // { id: "walicode", label: "WaLiCode" }, // 暂时隐藏，配置写入不生效问题待解决
  // { id: "gemini-cli", label: "Gemini CLI" },
  // { id: "claude-desktop", label: "Claude Desktop" },
  { id: "opencode", label: "OpenCode" },
  { id: "openclaw", label: "OpenClaw" },
  { id: "hermes", label: "Hermes" },
];

const platformTabs: { id: Platform; label: string; shortLabel: string; icon: typeof Terminal; color: string; lang: string }[] = [
  { id: "curl-mac", label: "cURL Mac/Linux", shortLabel: "cURL", icon: Terminal, color: "emerald", lang: "bash" },
  { id: "curl-windows", label: "cURL Windows", shortLabel: "cURLWin", icon: Terminal, color: "blue", lang: "batch" },
  { id: "javascript", label: "JavaScript", shortLabel: "JS", icon: Code2, color: "amber", lang: "javascript" },
  { id: "typescript", label: "TypeScript", shortLabel: "TS", icon: Code2, color: "blue", lang: "typescript" },
  { id: "java", label: "Java", shortLabel: "Java", icon: Coffee, color: "orange", lang: "java" },
];

const protocolTabs: { id: Protocol; label: string; endpoint: string; desc: string; icon: typeof MessageSquare; accent: string }[] = [
  { id: "chat", label: "OpenAI Chat", endpoint: "/v1/chat/completions", desc: "标准 Chat Completions 协议，广泛兼容", icon: MessageSquare, accent: "emerald" },
  { id: "responses", label: "OpenAI Responses", endpoint: "/v1/responses", desc: "Responses API，input/output 格式", icon: Layers, accent: "blue" },
  { id: "anthropic", label: "Anthropic Messages", endpoint: "/v1/messages", desc: "Claude Messages 协议，支持 Claude Code", icon: Sparkles, accent: "violet" },
];

// Accent color classes mapping
const accentClasses: Record<string, { bg: string; border: string; text: string; ring: string; dot: string; softBg: string }> = {
  emerald: { bg: "bg-emerald-500", border: "border-emerald-300", text: "text-emerald-700", ring: "ring-emerald-200", dot: "bg-emerald-500", softBg: "bg-emerald-50" },
  blue: { bg: "bg-blue-500", border: "border-blue-300", text: "text-blue-700", ring: "ring-blue-200", dot: "bg-blue-500", softBg: "bg-blue-50" },
  violet: { bg: "bg-violet-500", border: "border-violet-300", text: "text-violet-700", ring: "ring-violet-200", dot: "bg-violet-500", softBg: "bg-violet-50" },
  amber: { bg: "bg-amber-500", border: "border-amber-300", text: "text-amber-700", ring: "ring-amber-200", dot: "bg-amber-500", softBg: "bg-amber-50" },
  orange: { bg: "bg-orange-500", border: "border-orange-300", text: "text-orange-700", ring: "ring-orange-200", dot: "bg-orange-500", softBg: "bg-orange-50" },
};

export function UsagePage() {
  const navigate = useNavigate();
  const [channels, setChannels] = useState<Channel[]>([]);
  const [authAccounts, setAuthAccounts] = useState<AuthAccount[]>([]);
  const [keys, setKeys] = useState<ApiKey[]>([]);
  const [ss, setSs] = useState<ServerStatus | null>(null);
  const [selKey, setSelKey] = useState("");
  const [selModel, setSelModel] = useState("");
  const [copied, setCopied] = useState<string | null>(null);
  const [testState, setTestState] = useState<TestState>("idle");
  const [testResult, setTestResult] = useState("");
  const [testLatency, setTestLatency] = useState<number | null>(null);
  const [activeTab, setActiveTab] = useState<Platform>("curl-mac");
  const [activeProtocol, setActiveProtocol] = useState<Protocol>("chat");
  const [toastMsg, setToastMsg] = useState<string | null>(null);
  const toastTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const [usageTab, setUsageTab] = useState<UsageTab>("api");

  // Persist model & key selection across sessions
  const modelStorageKey = "waliapi:usage-model";
  const keyStorageKey = "waliapi:usage-key";

  useEffect(() => {
    Promise.all([
      channelApi.getAll().catch(() => []), apiKeyApi.getAll().catch(() => []),
      serverApi.getStatus().catch(() => null),
      authApi.accountsList().catch(() => []),
    ]).then(([ch, ks, s, accts]) => {
      setChannels(ch as Channel[]); setKeys(ks as ApiKey[]); setSs(s as ServerStatus | null);
      setAuthAccounts(accts as AuthAccount[]);

      // Restore persisted key, fallback to first
      const savedKey = localStorage.getItem(keyStorageKey);
      const keyList = ks as ApiKey[];
      if (keyList.length > 0) {
        const keyValid = savedKey && keyList.some(k => k.key === savedKey);
        setSelKey(keyValid ? savedKey! : keyList[0].key);
      }

      // Build model list from active channels + active auth accounts
      const ms: string[] = [];
      (ch as Channel[]).forEach(c => {
        if (c.status !== 1) return; // skip inactive channels
        c.models.forEach(m => { if (!ms.includes(m)) ms.push(m); });
        if (c.model_mapping) {
          Object.keys(c.model_mapping).forEach(from => { if (!ms.includes(from)) ms.push(from); });
        }
      });
      (accts as AuthAccount[]).forEach(a => {
        if (a.disabled) return; // skip disabled auth accounts
        a.models.forEach(m => { if (!ms.includes(m.id)) ms.push(m.id); });
        if (a.model_mapping) {
          Object.keys(a.model_mapping).forEach(from => { if (!ms.includes(from)) ms.push(from); });
        }
      });
      if (ms.length > 0) {
        const savedModel = localStorage.getItem(modelStorageKey);
        setSelModel(savedModel && ms.includes(savedModel) ? savedModel : ms[0]);
      }
    });
    const iv = setInterval(() => serverApi.getStatus().then(setSs).catch(() => {}), 5000);
    return () => clearInterval(iv);
  }, []);

  const baseUrl = isTauriRuntime()
    ? (ss?.running ? `${ss.url}/v1` : "http://127.0.0.1:8777/v1")
    : `${window.location.origin}/v1`;

  // Find the currently selected API key object.
  const selectedApiKey = useMemo(
    () => keys.find(k => k.key === selKey),
    [keys, selKey],
  );

  // Three categories: API channel models, Auth account models, mapping aliases (unified).
  // Filtered by the selected API key's allowed/denied lists.
  const modelCategories = useMemo(() => {
    const key = selectedApiKey;
    // Check if a channel id is allowed by the key's channel rules.
    const channelAllowed = (chId: string) => {
      if (!key) return true;
      if (key.allowed_channels.length > 0 && !key.allowed_channels.includes(chId)) return false;
      if (key.denied_channels.includes(chId)) return false;
      return true;
    };
    // Check if a model name is allowed by the key's model rules.
    const modelAllowed = (model: string) => {
      if (!key) return true;
      if (key.allowed_models.length > 0 && !key.allowed_models.includes(model)) return false;
      if (key.denied_models.includes(model)) return false;
      return true;
    };

    const channelSet = new Set<string>();
    const authSet = new Set<string>();
    const mappingSet = new Set<string>();

    channels.filter(c => c.status === 1).forEach(c => {
      if (!channelAllowed(c.id)) return;
      c.models.forEach(m => { if (modelAllowed(m)) channelSet.add(m); });
      if (c.model_mapping) Object.keys(c.model_mapping).forEach(from => { if (modelAllowed(from)) mappingSet.add(from); });
    });
    // Auth accounts are exempt from channel-level restrictions (no channel id).
    // Model-level restrictions still apply.
    authAccounts.filter(a => !a.disabled).forEach(a => {
      a.models.filter(m => !m.unavailable).forEach(m => { if (modelAllowed(m.id)) authSet.add(m.id); });
      if (a.model_mapping) Object.keys(a.model_mapping).forEach(from => { if (modelAllowed(from)) mappingSet.add(from); });
    });

    return {
      channel: [...channelSet],
      auth: [...authSet],
      mapping: [...mappingSet],
    };
  }, [channels, authAccounts, selectedApiKey]);

  // Flat deduplicated list for localStorage validation.
  const models = useMemo(() => {
    const seen = new Set<string>();
    const all: string[] = [];
    [...modelCategories.channel, ...modelCategories.auth, ...modelCategories.mapping].forEach(m => {
      if (!seen.has(m)) { seen.add(m); all.push(m); }
    });
    return all;
  }, [modelCategories]);

  // Reset selModel if it's no longer in the filtered list (e.g. after switching API Key).
  useEffect(() => {
    if (models.length > 0 && !models.includes(selModel)) {
      const fallback = models[0];
      setSelModel(fallback);
      localStorage.setItem(modelStorageKey, fallback);
    }
  }, [models, selModel]);

  const showToast = (msg: string) => {
    setToastMsg(msg);
    if (toastTimer.current) clearTimeout(toastTimer.current);
    toastTimer.current = setTimeout(() => setToastMsg(null), 1800);
  };

  const copy = async (text: string, id: string) => {
    await writeClipboard(text);
    setCopied(id);
    showToast("已复制到剪贴板");
    setTimeout(() => setCopied(null), 2000);
  };

  // Protocol-specific endpoints
  const endpoints: Record<Protocol, string> = {
    chat: `${baseUrl}/chat/completions`,
    responses: `${baseUrl}/responses`,
    anthropic: `${baseUrl}/messages`,
  };
  // API/SDK examples use the `/v1` base. Claude Code is different: it adds
  // `/v1/messages` itself to ANTHROPIC_BASE_URL.
  const anthropicClientBaseUrl = baseUrl.replace(/\/v1\/?$/, "");

  const scripts: Record<Protocol, Record<Platform, string>> = {
    chat: {
      "curl-mac": `curl ${endpoints.chat} \\
  -H "Content-Type: application/json" \\
  -H "Authorization: Bearer ${selKey}" \\
  -d '{
    "model": "${selModel}",
    "messages": [{"role": "user", "content": "Hello!"}]
  }'`,
      "curl-windows": `curl ${endpoints.chat} ^
  -H "Content-Type: application/json" ^
  -H "Authorization: Bearer ${selKey}" ^
  -d "{\\"model\\": \\"${selModel}\\", \\"messages\\": [{\\"role\\": \\"user\\", \\"content\\": \\"Hello!\\"}]}"`,
      "javascript": `import OpenAI from "openai";

const client = new OpenAI({
  baseURL: "${baseUrl}",
  apiKey: "${selKey}",
});

const response = await client.chat.completions.create({
  model: "${selModel}",
  messages: [{ role: "user", content: "Hello!" }],
});
console.log(response.choices[0].message.content);`,
      "typescript": `import OpenAI from "openai";

const client = new OpenAI({
  baseURL: "${baseUrl}",
  apiKey: "${selKey}",
});

async function main() {
  const response = await client.chat.completions.create({
    model: "${selModel}",
    messages: [{ role: "user" as const, content: "Hello!" }],
  });
  console.log(response.choices[0].message.content);
}
main();`,
      "java": `import java.net.URI;
import java.net.http.*;

public class XapiTest {
  public static void main(String[] args) throws Exception {
    HttpClient client = HttpClient.newHttpClient();
    String body = "{\\"model\\": \\"${selModel}\\", \\"messages\\": [{\\"role\\": \\"user\\", \\"content\\": \\"Hello!\\"}]}";
    HttpRequest req = HttpRequest.newBuilder()
      .uri(URI.create("${endpoints.chat}"))
      .header("Content-Type", "application/json")
      .header("Authorization", "Bearer ${selKey}")
      .POST(HttpRequest.BodyPublishers.ofString(body))
      .build();
    HttpResponse<String> resp = client.send(req, HttpResponse.BodyHandlers.ofString());
    System.out.println(resp.body());
  }
}`,
    },
    responses: {
      "curl-mac": `curl ${endpoints.responses} \\
  -H "Content-Type: application/json" \\
  -H "Authorization: Bearer ${selKey}" \\
  -d '{
    "model": "${selModel}",
    "input": [{"type": "message", "role": "user", "content": [{"type": "input_text", "text": "Hello!"}]}]
  }'`,
      "curl-windows": `curl ${endpoints.responses} ^
  -H "Content-Type: application/json" ^
  -H "Authorization: Bearer ${selKey}" ^
  -d "{\\"model\\": \\"${selModel}\\", \\"input\\": [{\\"type\\": \\"message\\", \\"role\\": \\"user\\", \\"content\\": [{\\"type\\": \\"input_text\\", \\"text\\": \\"Hello!\\"}]}]}"`,
      "javascript": `// OpenAI Responses API (fetch)
const response = await fetch("${endpoints.responses}", {
  method: "POST",
  headers: {
    "Content-Type": "application/json",
    "Authorization": "Bearer ${selKey}",
  },
  body: JSON.stringify({
    model: "${selModel}",
    input: [{ type: "message", role: "user", content: [{ type: "input_text", text: "Hello!" }] }],
  }),
});
const data = await response.json();
console.log(data.output[0].content[0].text);`,
      "typescript": `// OpenAI Responses API (fetch)
async function main() {
  const response = await fetch("${endpoints.responses}", {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
      "Authorization": "Bearer ${selKey}",
    },
    body: JSON.stringify({
      model: "${selModel}",
      input: [{ type: "message" as const, role: "user" as const, content: [{ type: "input_text" as const, text: "Hello!" }] }],
    }),
  });
  const data = await response.json();
  console.log(data.output[0].content[0].text);
}
main();`,
      "java": `import java.net.URI;
import java.net.http.*;

public class ResponsesTest {
  public static void main(String[] args) throws Exception {
    HttpClient client = HttpClient.newHttpClient();
    String body = "{\\"model\\": \\"${selModel}\\", \\"input\\": [{\\"type\\": \\"message\\", \\"role\\": \\"user\\", \\"content\\": [{\\"type\\": \\"input_text\\", \\"text\\": \\"Hello!\\"}]}]}";
    HttpRequest req = HttpRequest.newBuilder()
      .uri(URI.create("${endpoints.responses}"))
      .header("Content-Type", "application/json")
      .header("Authorization", "Bearer ${selKey}")
      .POST(HttpRequest.BodyPublishers.ofString(body))
      .build();
    HttpResponse<String> resp = client.send(req, HttpResponse.BodyHandlers.ofString());
    System.out.println(resp.body());
  }
}`,
    },
    anthropic: {
      "curl-mac": `curl ${endpoints.anthropic} \\
  -H "Content-Type: application/json" \\
  -H "x-api-key: ${selKey}" \\
  -H "anthropic-version: 2023-06-01" \\
  -d '{
    "model": "${selModel}",
    "max_tokens": 1024,
    "messages": [{"role": "user", "content": "Hello!"}]
  }'`,
      "curl-windows": `curl ${endpoints.anthropic} ^
  -H "Content-Type: application/json" ^
  -H "x-api-key: ${selKey}" ^
  -H "anthropic-version: 2023-06-01" ^
  -d "{\\"model\\": \\"${selModel}\\", \\"max_tokens\\": 1024, \\"messages\\": [{\\"role\\": \\"user\\", \\"content\\": \\"Hello!\\"}]}"`,
      "javascript": `// Anthropic Messages API (fetch)
const response = await fetch("${endpoints.anthropic}", {
  method: "POST",
  headers: {
    "Content-Type": "application/json",
    "x-api-key": "${selKey}",
    "anthropic-version": "2023-06-01",
  },
  body: JSON.stringify({
    model: "${selModel}",
    max_tokens: 1024,
    messages: [{ role: "user", content: "Hello!" }],
  }),
});
const data = await response.json();
console.log(data.content[0].text);`,
      "typescript": `// Anthropic Messages API (fetch)
async function main() {
  const response = await fetch("${endpoints.anthropic}", {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
      "x-api-key": "${selKey}",
      "anthropic-version": "2023-06-01",
    },
    body: JSON.stringify({
      model: "${selModel}",
      max_tokens: 1024,
      messages: [{ role: "user" as const, content: "Hello!" }],
    }),
  });
  const data = await response.json();
  console.log(data.content[0].text);
}
main();`,
      "java": `import java.net.URI;
import java.net.http.*;

public class AnthropicTest {
  public static void main(String[] args) throws Exception {
    HttpClient client = HttpClient.newHttpClient();
    String body = "{\\"model\\": \\"${selModel}\\", \\"max_tokens\\": 1024, \\"messages\\": [{\\"role\\": \\"user\\", \\"content\\": \\"Hello!\\"}]}";
    HttpRequest req = HttpRequest.newBuilder()
      .uri(URI.create("${endpoints.anthropic}"))
      .header("Content-Type", "application/json")
      .header("x-api-key", "${selKey}")
      .header("anthropic-version", "2023-06-01")
      .POST(HttpRequest.BodyPublishers.ofString(body))
      .build();
    HttpResponse<String> resp = client.send(req, HttpResponse.BodyHandlers.ofString());
    System.out.println(resp.body());
  }
}`,
    },
  };

  const handleTest = async () => {
    if (!selKey || !selModel) return;
    setTestState("running"); setTestResult(""); setTestLatency(null);
    const startTime = performance.now();
    try {
      const isAnthropic = activeProtocol === "anthropic";
      const isResponses = activeProtocol === "responses";
      const url = endpoints[activeProtocol];
      const headers: Record<string, string> = { "Content-Type": "application/json" };
      if (isAnthropic) {
        headers["x-api-key"] = selKey;
        headers["anthropic-version"] = "2023-06-01";
      } else {
        headers["Authorization"] = `Bearer ${selKey}`;
      }
      let body: string;
      if (isAnthropic) {
        body = JSON.stringify({ model: selModel, max_tokens: 1024, stream: false, messages: [{ role: "user", content: "Say hello in one sentence" }] });
      } else if (isResponses) {
        body = JSON.stringify({ model: selModel, input: [{ type: "message", role: "user", content: [{ type: "input_text", text: "Say hello in one sentence" }] }] });
      } else {
        body = JSON.stringify({ model: selModel, messages: [{ role: "user", content: "Say hello in one sentence" }] });
      }
      const resp = await fetch(url, { method: "POST", headers, body });
      const data = await resp.json();
      const elapsed = Math.round(performance.now() - startTime);
      setTestLatency(elapsed);
      if (resp.ok) {
        setTestState("success");
        if (isAnthropic) {
          setTestResult(`OK ${resp.status}\n\n${data.content?.[0]?.text || JSON.stringify(data, null, 2)}`);
        } else if (isResponses) {
          setTestResult(`OK ${resp.status}\n\n${data.output?.[0]?.content?.[0]?.text || JSON.stringify(data, null, 2)}`);
        } else {
          setTestResult(`OK ${resp.status}\n\n${data.choices?.[0]?.message?.content || JSON.stringify(data, null, 2)}`);
        }
      } else {
        setTestState("error");
        setTestResult(`Error ${resp.status} ${resp.statusText}\n\n${JSON.stringify(data, null, 2)}`);
      }
    } catch (e: any) {
      const elapsed = Math.round(performance.now() - startTime);
      setTestLatency(elapsed);
      setTestState("error");
      setTestResult(`Request failed: ${e.message || String(e)}\n\nCauses:\n1. Server not running\n2. Invalid key\n3. Upstream channel error`);
    }
  };

  const currentProtocol = protocolTabs.find(p => p.id === activeProtocol)!;
  const currentAccent = accentClasses[currentProtocol.accent];
  const currentScripts = scripts[activeProtocol];
  const activePlatform = platformTabs.find(t => t.id === activeTab)!;

  return (
    <div className="page-shell space-y-6 max-w-6xl text-slate-900">
      {/* Toast */}
      {toastMsg && (
        <div className="fixed bottom-6 right-6 z-50 flex items-center gap-2 rounded-2xl bg-slate-900 px-4 py-2.5 text-sm font-medium text-white shadow-xl animate-[fadeInUp_0.2s_ease]">
          <Check size={15} className="text-emerald-400" />
          {toastMsg}
        </div>
      )}

      {/* 标题 + Tab 栏 — sticky 固定 */}
      <div className="sticky top-0 z-30 -mx-7 px-7 pb-2 bg-white/90 backdrop-blur-md border-b border-slate-100">
        {/* 标题 */}
        <div className="pt-4 pb-3">
          <h1 className="flex items-center gap-2.5 text-xl font-bold tracking-tight text-slate-900">
            <BookOpen className="h-6 w-6 text-blue-600" />
            LLM 使用
          </h1>
        </div>
        {/* Tab 栏 — 彩色 pill 风格 */}
        <div className="flex items-center gap-1.5 overflow-x-auto pb-2">
          <button
            onClick={() => setUsageTab("api")}
            className={`shrink-0 rounded-full px-3 py-1.5 text-xs font-medium transition-all ${
              usageTab === "api"
                ? "bg-blue-500 text-white shadow-sm"
                : "bg-slate-100 text-slate-600 hover:bg-slate-200"
            }`}
          >
            API 接口
          </button>
          <div className="h-4 w-px bg-slate-300 shrink-0" />
          {APP_TABS.map(app => {
            const Icon = getAppIcon(app.id);
            const isActive = usageTab === app.id;
            const colors: Record<string, { active: string; idle: string }> = {
              "codex": { active: "bg-emerald-500 text-white", idle: "bg-emerald-50 text-emerald-700 hover:bg-emerald-100" },
              "claude-code": { active: "bg-violet-500 text-white", idle: "bg-violet-50 text-violet-700 hover:bg-violet-100" },
              "gemini-cli": { active: "bg-blue-500 text-white", idle: "bg-blue-50 text-blue-700 hover:bg-blue-100" },
              "claude-desktop": { active: "bg-orange-500 text-white", idle: "bg-orange-50 text-orange-700 hover:bg-orange-100" },
              "opencode": { active: "bg-amber-500 text-white", idle: "bg-amber-50 text-amber-700 hover:bg-amber-100" },
              "openclaw": { active: "bg-teal-500 text-white", idle: "bg-teal-50 text-teal-700 hover:bg-teal-100" },
              "hermes": { active: "bg-rose-500 text-white", idle: "bg-rose-50 text-rose-700 hover:bg-rose-100" },
              "walicode": { active: "bg-indigo-500 text-white", idle: "bg-indigo-50 text-indigo-700 hover:bg-indigo-100" },
            };
            const c = colors[app.id] || { active: "bg-slate-700 text-white", idle: "bg-slate-100 text-slate-600" };
            return (
              <button
                key={app.id}
                onClick={() => setUsageTab(app.id)}
                className={`flex shrink-0 items-center gap-1 rounded-full px-3 py-1.5 text-xs font-medium transition-all ${
                  isActive ? c.active + " shadow-sm" : c.idle
                }`}
              >
                <Icon size={12} />
                {app.label}
              </button>
            );
          })}
        </div>
      </div>

      {usageTab !== "api" ? (
        <AppConfigPanel appName={usageTab} />
      ) : (
      <>
      {/* Protocol Cards — 3 张卡片直观展示三种协议 */}
      <div className="grid grid-cols-1 gap-3 md:grid-cols-3">
        {protocolTabs.map(p => {
          const accent = accentClasses[p.accent];
          const Icon = p.icon;
          const isActive = activeProtocol === p.id;
          return (
            <button
              key={p.id}
              onClick={() => setActiveProtocol(p.id)}
              className={`group relative overflow-hidden rounded-[20px] border p-5 text-left transition-all duration-200 ${
                isActive
                  ? `${accent.border} ${accent.softBg} shadow-[0_8px_24px_rgba(0,0,0,0.06)]`
                  : "border-slate-200 bg-white hover:border-slate-300 hover:shadow-sm"
              }`}
            >
              {/* Top accent bar */}
              <div className={`absolute inset-x-0 top-0 h-1 ${accent.bg} transition-opacity ${isActive ? "opacity-100" : "opacity-0 group-hover:opacity-30"}`} />

              <div className="flex items-start justify-between">
                <div className={`flex h-10 w-10 items-center justify-center rounded-xl ${isActive ? accent.softBg : "bg-slate-50"} transition-colors`}>
                  <Icon className={`h-5 w-5 ${isActive ? accent.text : "text-slate-500"}`} size={20} />
                </div>
                {isActive && (
                  <span className={`flex h-5 w-5 items-center justify-center rounded-full ${accent.bg}`}>
                    <Check size={12} className="text-white" />
                  </span>
                )}
              </div>
              <div className="mt-3 text-sm font-semibold text-slate-900">{p.label}</div>
              <div className="mt-1 text-xs text-slate-500">{p.desc}</div>
              <div className="mt-3 flex items-center gap-1.5">
                <code className={`rounded-md ${accent.softBg} px-2 py-0.5 text-[11px] font-mono ${accent.text}`}>
                  {p.endpoint}
                </code>
              </div>
            </button>
          );
        })}
      </div>

      {/* Main Grid: Left config + Right test */}
      <div className="grid grid-cols-1 gap-4 xl:grid-cols-[1.1fr_0.9fr]">
        {/* Left: Config */}
        <div className="surface rounded-[24px] p-5 space-y-4">
          <div className="flex items-center gap-2">
            <h2 className="text-base font-semibold text-slate-900">接入信息</h2>
            <span className={`rounded-full ${currentAccent.softBg} px-2 py-0.5 text-xs font-medium ${currentAccent.text}`}>
              {currentProtocol.label}
            </span>
          </div>

          {/* Base URL — 突出显示 */}
          <div className="rounded-2xl border border-slate-200 bg-gradient-to-br from-slate-50 to-white p-4">
            <div className="mb-2 flex items-center gap-2 text-xs font-semibold uppercase tracking-wide text-slate-400">
              <Link2 size={13} /> Base URL
            </div>
            <div className="flex items-center gap-2">
              <code className="flex-1 break-all rounded-xl border border-slate-200 bg-white px-3 py-2.5 text-sm font-mono text-slate-900">
                {activeProtocol === "anthropic" ? anthropicClientBaseUrl : baseUrl}
              </code>
              <button onClick={() => copy(activeProtocol === "anthropic" ? anthropicClientBaseUrl : baseUrl, "baseurl")} className="action-secondary px-3 py-2.5" title="复制 Base URL">
                {copied === "baseurl" ? <Check size={16} className="text-emerald-600" /> : <Copy size={16} />}
              </button>
            </div>
          </div>

          {/* Protocol Endpoint */}
          <div className="rounded-2xl border border-slate-200 bg-slate-50 p-4">
            <div className="mb-2 text-xs font-semibold uppercase tracking-wide text-slate-400">协议端点</div>
            <div className="flex items-center gap-2">
              <span className={`h-2 w-2 rounded-full ${currentAccent.dot}`} />
              <code className="flex-1 break-all text-sm font-mono text-slate-700">{endpoints[activeProtocol]}</code>
              <button onClick={() => copy(endpoints[activeProtocol], "endpoint")} className="action-secondary px-2.5 py-1.5" title="复制端点">
                {copied === "endpoint" ? <Check size={14} className="text-emerald-600" /> : <Copy size={14} />}
              </button>
            </div>
          </div>

          {/* API Key & Model — 并排 */}
          <div className="grid grid-cols-1 gap-3 md:grid-cols-2">
            <div className="rounded-2xl border border-slate-200 bg-slate-50 p-4">
              <div className="mb-2 flex items-center gap-2 text-xs font-semibold uppercase tracking-wide text-slate-400">
                <KeyRound size={13} /> API Key
              </div>
              <div className="flex gap-2">
                <div className="relative flex-1">
                  <select
                    value={selKey}
                    onChange={e => {
                      setSelKey(e.target.value);
                      localStorage.setItem(keyStorageKey, e.target.value);
                      // selModel validity will be checked by the models useMemo + effect below.
                    }}
                    className="w-full appearance-none rounded-xl border border-slate-200 bg-white px-3 py-2.5 pr-8 text-sm font-mono text-slate-900 shadow-sm cursor-pointer"
                  >
                    {keys.length === 0 && <option value="">请先创建密钥</option>}
                    {keys.map(k => <option key={k.id} value={k.key}>{k.name} ({k.key.slice(0, 12)}...)</option>)}
                  </select>
                  <ChevronDown size={14} className="pointer-events-none absolute right-2.5 top-1/2 -translate-y-1/2 text-slate-400" />
                </div>
                <button onClick={() => selKey && copy(selKey, "key")} disabled={!selKey} className="action-secondary px-3 py-2.5 disabled:opacity-50" title="复制 Key">
                  {copied === "key" ? <Check size={16} className="text-emerald-600" /> : <Copy size={16} />}
                </button>
              </div>
              {keys.length === 0 && (
                <div className="mt-2 flex items-center gap-1.5 text-xs text-rose-500">
                  <AlertCircle size={12} />
                  <span>尚未创建密钥，</span>
                  <button onClick={() => navigate("/api-keys")} className="font-medium text-rose-600 underline hover:text-rose-700">
                    去配置 →
                  </button>
                </div>
              )}
            </div>

            <div className="rounded-2xl border border-slate-200 bg-slate-50 p-4">
              <div className="mb-2 flex items-center gap-2 text-xs font-semibold uppercase tracking-wide text-slate-400">
                <Bot size={13} /> Model
              </div>
              <div className="flex gap-2">
                <div className="relative flex-1">
                  <select
                    value={selModel}
                    onChange={e => {
                      setSelModel(e.target.value);
                      localStorage.setItem(modelStorageKey, e.target.value);
                    }}
                    className="w-full appearance-none rounded-xl border border-slate-200 bg-white px-3 py-2.5 pr-8 text-sm font-mono text-slate-900 shadow-sm cursor-pointer"
                  >
                    {models.length === 0 && <option value="">请先配置渠道或登录账号</option>}
                    {modelCategories.channel.length > 0 && (
                      <optgroup label="API 渠道模型">{modelCategories.channel.map(m => <option key={m} value={m}>{m}</option>)}</optgroup>
                    )}
                    {modelCategories.auth.length > 0 && (
                      <optgroup label="Auth 账号模型">{modelCategories.auth.map(m => <option key={m} value={m}>{m}</option>)}</optgroup>
                    )}
                    {modelCategories.mapping.length > 0 && (
                      <optgroup label="映射模型">{modelCategories.mapping.map(m => <option key={m} value={m}>{m}</option>)}</optgroup>
                    )}
                  </select>
                  <ChevronDown size={14} className="pointer-events-none absolute right-2.5 top-1/2 -translate-y-1/2 text-slate-400" />
                </div>
                <button onClick={() => selModel && copy(selModel, "model")} disabled={!selModel} className="action-secondary px-3 py-2.5 disabled:opacity-50" title="复制 Model">
                  {copied === "model" ? <Check size={16} className="text-emerald-600" /> : <Copy size={16} />}
                </button>
              </div>
              {models.length === 0 && (
                <div className="mt-2 flex items-center gap-1.5 text-xs text-rose-500">
                  <AlertCircle size={12} />
                  <span>尚未配置可用渠道或登录账号，</span>
                  <button onClick={() => navigate("/channels")} className="font-medium text-rose-600 underline hover:text-rose-700">
                    去配置 →
                  </button>
                </div>
              )}
            </div>
          </div>
        </div>

        {/* Right: Connection Test */}
        <div className="surface rounded-[24px] p-5">
          <div className="mb-4 flex items-center justify-between">
            <div className="flex items-center gap-2">
              <h2 className="text-base font-semibold text-slate-900">连接测试</h2>
              {testLatency !== null && testState !== "idle" && (
                <span className={`rounded-full px-2 py-0.5 text-xs font-medium ${
                  testState === "success" ? "bg-emerald-100 text-emerald-700" : "bg-rose-100 text-rose-700"
                }`}>
                  {testLatency}ms
                </span>
              )}
            </div>
            <button
              onClick={handleTest}
              disabled={testState === "running" || !selKey || !selModel}
              className="action-primary disabled:opacity-50"
            >
              {testState === "running"
                ? <Loader2 size={16} className="animate-spin" />
                : <Play size={16} />}
              {testState === "running" ? "测试中..." : "发送测试请求"}
            </button>
          </div>

          {/* Test info bar */}
          <div className="rounded-2xl border border-slate-200 bg-slate-50 p-3.5 text-sm text-slate-600">
            <div className="flex items-center gap-2 flex-wrap">
              <span className="text-xs text-slate-400">将使用</span>
              <span className={`rounded-md ${currentAccent.softBg} px-2 py-0.5 text-xs font-medium ${currentAccent.text}`}>
                {currentProtocol.label}
              </span>
              <ArrowRight size={12} className="text-slate-300" />
              <code className="text-xs font-mono text-slate-500">{endpoints[activeProtocol].replace(baseUrl, "...")}</code>
            </div>
          </div>

          {/* Test result — idle/running/success/error states */}
          {testState === "idle" && (
            <div className="mt-4 flex flex-col items-center justify-center rounded-2xl border border-dashed border-slate-200 py-12 text-center">
              <Zap className="h-8 w-8 text-slate-300" />
              <p className="mt-2 text-sm text-slate-400">点击上方按钮发起测试请求</p>
            </div>
          )}

          {testState === "running" && (
            <div className="mt-4 flex flex-col items-center justify-center rounded-2xl border border-blue-200 bg-blue-50 py-12 text-center">
              <Loader2 className="h-8 w-8 animate-spin text-blue-500" />
              <p className="mt-2 text-sm text-blue-600">正在发送请求...</p>
            </div>
          )}

          {testState === "success" && (
            <div className="mt-4 rounded-2xl border border-emerald-200 bg-emerald-50 p-4">
              <div className="flex items-center gap-2 text-sm font-medium text-emerald-700">
                <Check size={16} className="text-emerald-600" />
                连接成功
                {testLatency !== null && <span className="text-emerald-500">· {testLatency}ms</span>}
              </div>
              <pre className="mt-3 max-h-56 overflow-auto rounded-xl bg-white/60 p-3 text-sm font-mono leading-6 whitespace-pre-wrap text-emerald-900">
                {testResult}
              </pre>
            </div>
          )}

          {testState === "error" && (
            <div className="mt-4 rounded-2xl border border-rose-200 bg-rose-50 p-4">
              <div className="flex items-center gap-2 text-sm font-medium text-rose-700">
                <span className="text-rose-500">✗</span>
                连接失败
                {testLatency !== null && <span className="text-rose-400">· {testLatency}ms</span>}
              </div>
              <pre className="mt-3 max-h-56 overflow-auto rounded-xl bg-white/60 p-3 text-sm font-mono leading-6 whitespace-pre-wrap text-rose-900">
                {testResult}
              </pre>
            </div>
          )}
        </div>
      </div>

      {/* Code Example Section */}
      <div className="surface rounded-[24px] p-5">
        <div className="mb-4 flex items-center justify-between">
          <h2 className="text-base font-semibold text-slate-900">代码示例</h2>
          <div className="flex items-center gap-2">
            <span className={`rounded-full ${currentAccent.softBg} px-2.5 py-1 text-xs font-medium ${currentAccent.text}`}>
              {currentProtocol.label}
            </span>
            <span className="text-sm text-slate-400">{currentProtocol.desc}</span>
          </div>
        </div>

        {/* Platform selector — pill style */}
        <div className="mb-4 flex gap-2 overflow-x-auto pb-1">
          {platformTabs.map(t => {
            const Icon = t.icon;
            const accent = accentClasses[t.color];
            const isActive = activeTab === t.id;
            return (
              <button
                key={t.id}
                onClick={() => setActiveTab(t.id)}
                className={`flex shrink-0 items-center gap-1.5 rounded-xl border px-3.5 py-2 text-sm font-medium transition-all ${
                  isActive
                    ? `${accent.border} ${accent.softBg} ${accent.text} shadow-sm`
                    : "border-slate-200 bg-white text-slate-600 hover:border-slate-300 hover:text-slate-900"
                }`}
              >
                <Icon size={15} />
                {t.label}
              </button>
            );
          })}
        </div>

        {/* Code block with header */}
        <div className="overflow-hidden rounded-2xl border border-slate-700 bg-[#111827]">
          {/* Code header bar */}
          <div className="flex items-center justify-between border-b border-slate-700/60 bg-[#0f172a] px-4 py-2.5">
            <div className="flex items-center gap-2">
              {/* Traffic lights */}
              <div className="flex gap-1.5">
                <span className="h-3 w-3 rounded-full bg-[#ff5f57]" />
                <span className="h-3 w-3 rounded-full bg-[#febc2e]" />
                <span className="h-3 w-3 rounded-full bg-[#28c840]" />
              </div>
              <span className="ml-2 text-xs font-medium text-slate-400">
                {activePlatform.label}
              </span>
            </div>
            <button
              onClick={() => copy(currentScripts[activeTab], activeTab)}
              className="flex items-center gap-1.5 rounded-lg border border-slate-600/50 bg-slate-700/40 px-2.5 py-1.5 text-xs font-medium text-slate-300 transition-colors hover:bg-slate-600/40 hover:text-white"
            >
              {copied === activeTab
                ? <><Check size={13} className="text-emerald-400" /> 已复制</>
                : <><Copy size={13} /> 复制</>}
            </button>
          </div>

          {/* Code content */}
          <SyntaxHighlighter
            language={activePlatform.lang}
            style={oneDark}
            customStyle={{
              margin: 0,
              borderRadius: 0,
              fontSize: "0.875rem",
              maxHeight: "28rem",
              overflow: "auto",
              background: "#111827",
              padding: "1rem 1.25rem",
            }}
          >
            {currentScripts[activeTab]}
          </SyntaxHighlighter>
        </div>
      </div>
      </>
      )}
    </div>
  );
}
