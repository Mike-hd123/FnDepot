import { useState, useMemo, useEffect, useRef } from "react";
import { channelApi } from "../lib/api";
import { writeClipboard } from "../lib/runtime";
import type {
  Channel, CreateChannelInput, UpdateChannelInput,
  ChannelProtocol, ChannelProvider, ChannelEndpoint, ChannelAuthScheme,
  ChannelPreset, ChannelProtocolPresetGroup,
  DraftChannelTestResult, DraftChannelTestInput,
  UpstreamModelsResult,
} from "../types";
import {
  PROTOCOL_LABELS, ENDPOINT_LABELS, ENDPOINT_PATHS,
} from "../lib/constants";
import { X, Plus, Check, RefreshCw, KeyRound, Undo, Loader2, Trash2, Power, Copy } from "lucide-react";
import { MappingSection } from "./MappingSection";
import { DraftTestModal } from "./channel-form/DraftTestModal";
import { ModelSyncModal } from "./channel-form/ModelSyncModal";
import { ProviderDropdown } from "./channel-form/ProviderDropdown";

// ─── 协议级结构（UI 结构常量，非厂商模板副本）────────────────────────────────
// 这些描述的是「协议本身的语义」（设计 3.2）：OpenAI 有两个可选端点，
// Anthropic 固定 Messages，Ollama 固定 /api/chat。厂商 URL/模型模板唯一来源
// 是后端 registry（get_channel_presets）。
const PROTOCOLS: ChannelProtocol[] = ["openai", "anthropic", "ollama"];

const PROTOCOL_ENDPOINT_OPTIONS: Record<ChannelProtocol, ChannelEndpoint[]> = {
  openai: ["chat_completions", "responses"],
  anthropic: ["messages"],
  ollama: ["api_chat"],
};

const PROTOCOL_BASE_URL_HINTS: Record<ChannelProtocol, string> = {
  openai: "不包含端点路径；通常以 /v1 或兼容服务根路径结束",
  anthropic: "以 /v1 结尾（如 https://api.anthropic.com/v1），端点自动补 /messages",
  ollama: "本机或远程 Ollama 的主机与端口（例如 http://localhost:11434）",
};

const PROTOCOL_DEFAULT_AUTH: Record<ChannelProtocol, ChannelAuthScheme> = {
  openai: "bearer",
  anthropic: "x_api_key",
  ollama: "optional_bearer",
};

const isProtocol = (v: unknown): v is ChannelProtocol =>
  v === "openai" || v === "anthropic" || v === "ollama";

/** 全部已知端点（含能力端点 count_tokens/embeddings），用于编辑回填保真（F2）。 */
const ALL_ENDPOINTS: ChannelEndpoint[] = [
  "chat_completions", "responses", "messages", "count_tokens", "embeddings", "api_chat",
];

const isEndpoint = (v: unknown): v is ChannelEndpoint =>
  typeof v === "string" && (ALL_ENDPOINTS as string[]).includes(v);

/** 协议 custom option 的默认勾选端点（与后端 custom_preset 一致）。 */
function defaultEndpointsFor(protocol: ChannelProtocol): ChannelEndpoint[] {
  switch (protocol) {
    case "openai": return ["chat_completions"];
    case "anthropic": return ["messages"];
    case "ollama": return ["api_chat"];
  }
}

/** 应用预设时写入 form 的端点集合（F1）：
 *  Anthropic 的能力端点是固定的（固定 Messages + 模板声明的 count_tokens），
 *  必须持久化全量 native_endpoints 供路由命中；OpenAI/Ollama 端点可勾选，
 *  以 default_checked（决定 UI 勾选态）为准。 */
function endpointsForPreset(preset: ChannelPreset): ChannelEndpoint[] {
  if (preset.protocol === "anthropic") return [...preset.native_endpoints];
  return [...preset.default_checked_endpoints];
}

/** 自定义预设（legacy_base_url 为空）时，从 native 根推导旧代码兼容根（F6）。
 *  旧适配器在 base_url 后追加 /chat/completions（openai）或 /messages（claude），
 *  因此 anthropic/ollama 需要 /v1 根；openai 保留用户输入的根（通常已含 /v1）。 */
function deriveLegacyBaseUrl(protocol: ChannelProtocol, native: string): string {
  const root = native.trim().replace(/\/+$/, "");
  if (!root) return "";
  if (protocol === "openai") return root;
  return root.endsWith("/v1") ? root : `${root}/v1`;
}

/** Key 省略展示：超长时显示【前4...后4】，短则全显。 */
function maskKey(key: string): string {
  if (!key) return "";
  if (key.length <= 10) return key;
  return `【${key.slice(0, 4)}...${key.slice(-4)}】`;
}

/** 复制到剪贴板 */
async function copyToClipboard(text: string) {
  try {
    await writeClipboard(text);
  } catch {
    try {
      await writeClipboard(text);
    } catch {
      // ignore
    }
  }
}

/** Base URL（去尾斜杠）+ 端点路径（去首斜杠）→ 实际请求 URL；Base 为空返回空串。 */
function joinUrl(base: string, path: string): string {
  const root = base.trim().replace(/\/+$/, "");
  if (!root) return "";
  return `${root}/${path.replace(/^\/+/, "")}`;
}

interface FormState {
  name: string;
  protocol: ChannelProtocol;
  provider: ChannelProvider;
  native_base_url: string;
  api_key: string;
  models: string[];
  native_endpoints: ChannelEndpoint[];
  model_mapping: Record<string, string | string[]>;
  priority: number;
  weight: number;
  timeout_secs: number;
  preset_revision: string | null;
  legacy_executor_override?: string;
  // Multi-key: extra API keys for load balancing
  extra_keys: ExtraKeyItem[];
}

/** UI state for a single extra API key entry. */
interface ExtraKeyItem {
  id: string;          // DB id for existing keys, temp id for new keys
  api_key: string;     // masked value from DB or raw input for new keys
  weight: number;
  enabled: boolean;
  isEditing: boolean;  // inline edit mode
  isRevealed: boolean; // show/hide full key value
  isExisting: boolean; // true if loaded from DB
  rawValue: string;    // unmasked value when revealed
}

function initForm(editing: Channel | null, duplicate = false): FormState {
  if (editing) {
    const protocol = isProtocol(editing.protocol) ? editing.protocol : "openai";
    const endpoints = (editing.native_endpoints ?? []).filter(isEndpoint);
    return {
      name: duplicate ? `${editing.name} (副本)` : editing.name,
      protocol,
      provider: (editing.provider as ChannelProvider) || "custom",
      native_base_url: editing.native_base_url || editing.base_url,
      // 复制模式：清空 api_key（列表中是脱敏的），用户需重新输入
      api_key: duplicate ? "" : (editing.api_key || ""),
      models: editing.models ?? [],
      native_endpoints: endpoints.length > 0 ? endpoints : defaultEndpointsFor(protocol),
      model_mapping: editing.model_mapping ?? {},
      priority: editing.priority ?? 0,
      weight: editing.weight ?? 1,
      timeout_secs: editing.timeout_secs ?? 300,
      preset_revision: editing.preset_revision ?? null,
      legacy_executor_override: editing.legacy_executor_override ?? undefined,
      extra_keys: duplicate
        ? [] // 复制模式不复制额外 key（脱敏值无法还原）
        : (editing.extra_keys ?? []).map(k => ({
        id: k.id,
        api_key: k.api_key,
        weight: k.weight,
        enabled: k.status === 1,
        isEditing: false,
        isRevealed: false,
        isExisting: true,
        rawValue: "",
      })),
    };
  }
  return {
    name: "",
    protocol: "openai",
    provider: "custom",
    native_base_url: "",
    api_key: "",
    models: [],
    native_endpoints: defaultEndpointsFor("openai"),
    model_mapping: {},
    priority: 0,
    weight: 1,
    timeout_secs: 300,
    preset_revision: null,
    extra_keys: [],
  };
}


export function ChannelForm({ editing, duplicate = false, onClose, onSaved }: {
  editing: Channel | null;
  duplicate?: boolean;
  onClose: () => void;
  onSaved: () => void;
}) {
  const [form, setForm] = useState<FormState>(() => initForm(editing, duplicate));
  // 记录编辑态初始掩码值，用于保存时区分「用户未改」vs「真实输入」
  const [mainKeyOriginalMasked] = useState(duplicate ? "" : (editing?.api_key || ""));
  const [modelInput, setModelInput] = useState("");

  // ── presets（T01）────────────────────────────────────────────────────────
  const [presetGroups, setPresetGroups] = useState<ChannelProtocolPresetGroup[]>([]);
  const [presetsLoading, setPresetsLoading] = useState(true);
  const [presetsError, setPresetsError] = useState<string | null>(null);
  useEffect(() => {
    let alive = true;
    channelApi.getPresets()
      .then(groups => { if (alive) { setPresetGroups(groups); setPresetsLoading(false); } })
      .catch(e => { if (alive) { setPresetsError(String(e)); setPresetsLoading(false); } });
    return () => { alive = false; };
  }, []);

  // 三协议 Tab 常驻（无功能开关依赖）。
  const availableProtocols = PROTOCOLS;

  // ── 连接参数 / 测试 receipt 状态 ─────────────────────────────────────────
  // 编辑态下已保存的渠道名视为「用户已命名」，切换预设不自动改名。
  // 复制模式下名称已被修改过（加了后缀），也视为已命名。
  const [nameTouched, setNameTouched] = useState(!!editing || duplicate);
  const autoNameRef = useRef<string | null>(null);
  const [receipt, setReceipt] = useState<DraftChannelTestResult | null>(null);
  const [testPhase, setTestPhase] = useState<"idle" | "running" | "failed">("idle");
  const [testResult, setTestResult] = useState<DraftChannelTestResult | null>(null);
  const [saving, setSaving] = useState(false);
  const [clearKeyRequested, setClearKeyRequested] = useState(false);
  const [mainKeyEditing, setMainKeyEditing] = useState(false);
  const [localError, setLocalError] = useState<string | null>(null);
  const [saveError, setSaveError] = useState<string | null>(null);
  // ── T14 同步上游模型 ─────────────────────────────────────────────────────
  const [syncState, setSyncState] = useState<"idle" | "loading">("idle");
  const [syncResult, setSyncResult] = useState<UpstreamModelsResult | null>(null);
  const [syncError, setSyncError] = useState<string | null>(null);
  const [toastMsg, setToastMsg] = useState<string | null>(null);
  /** 本次应用后新增的模型（绿色高亮动画用）。 */
  const [addedModels, setAddedModels] = useState<string[]>([]);
  const toastTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const highlightTimer = useRef<ReturnType<typeof setTimeout> | null>(null);

  const currentPreset = useMemo(() => {
    const group = presetGroups.find(g => g.protocol === form.protocol);
    return group?.presets.find(p => p.provider === form.provider) ?? null;
  }, [presetGroups, form.protocol, form.provider]);

  const authScheme: ChannelAuthScheme = currentPreset?.auth_scheme ?? PROTOCOL_DEFAULT_AUTH[form.protocol];
  const keyRequired = authScheme !== "optional_bearer";

  // ── receipt 失效规则（T07）：protocol/provider/URL/Key/模型/端点/timeout 变更即失效；
  //    name/priority/weight/映射 变更不失效。 ───────────────────────────────
  function invalidateReceipt() {
    setReceipt(null);
    setTestPhase("idle");
    setTestResult(null);
    setSaveError(null);
  }

  function findPreset(protocol: ChannelProtocol, provider: ChannelProvider): ChannelPreset | null {
    const group = presetGroups.find(g => g.protocol === protocol);
    return group?.presets.find(p => p.provider === provider) ?? null;
  }

  function applyPreset(preset: ChannelPreset, apply: boolean) {
    setForm(prev => {
      let name = prev.name;
      if (apply && preset.provider !== "custom" && !nameTouched) {
        // 仅当名称为空或仍等于上次自动名称时，更新为厂商展示名
        if (!name || name === autoNameRef.current) {
          name = preset.display_name;
          autoNameRef.current = preset.display_name;
        }
      }
      return {
        ...prev,
        name,
        protocol: preset.protocol,
        provider: preset.provider,
        preset_revision: preset.preset_revision,
        ...(apply ? {
          native_base_url: preset.native_base_url,
          native_endpoints: endpointsForPreset(preset),
          models: preset.model_suggestions.map(m => m.id),
        } : {}),
      };
    });
    invalidateReceipt();
  }

  function applyProtocolDefaults(protocol: ChannelProtocol) {
    setForm(prev => ({
      ...prev,
      protocol,
      provider: "custom",
      preset_revision: null,
      native_base_url: "",
      native_endpoints: defaultEndpointsFor(protocol),
      models: [],
    }));
    invalidateReceipt();
  }

  function requestProtocolSwitch(protocol: ChannelProtocol) {
    if (protocol === form.protocol || saving) return;
    // 无确认：回该协议 custom 模板（连接参数重置，Key/名称/映射/P/W/超时保留）。
    const custom = findPreset(protocol, "custom");
    if (custom) applyPreset(custom, true);
    else applyProtocolDefaults(protocol);
  }

  function selectProvider(provider: ChannelProvider) {
    if (provider === form.provider || saving) return;
    const target = findPreset(form.protocol, provider);
    if (target) applyPreset(target, true);
  }

  // ── 连接字段变更 ─────────────────────────────────────────────────────────
  function onUrlChange(v: string) {
    setForm(prev => ({ ...prev, native_base_url: v }));
    invalidateReceipt();
  }
  function onKeyChange(v: string) {
    setForm(prev => ({ ...prev, api_key: v }));
    // 仅在非清除态下，有值时重置清除标记
    // 清除态下输入内容不应自动退出清除模式，需用户点撤销
    if (v.trim() !== "" && !clearKeyRequested) setClearKeyRequested(false);
    invalidateReceipt();
  }
  function onTimeoutChange(v: number) {
    setForm(prev => ({ ...prev, timeout_secs: v }));
    invalidateReceipt();
  }
  function onModelListChange(nextModels: string[]) {
    setForm(prev => ({ ...prev, models: nextModels }));
    invalidateReceipt();
  }
  function toggleEndpoint(ep: ChannelEndpoint, checked: boolean) {
    const has = form.native_endpoints.includes(ep);
    const next = checked
      ? (has ? form.native_endpoints : [...form.native_endpoints, ep])
      : form.native_endpoints.filter(e => e !== ep);
    setForm(prev => ({ ...prev, native_endpoints: next }));
    invalidateReceipt();
  }
  function requestClearKey() {
    setForm(prev => ({ ...prev, api_key: "" }));
    setClearKeyRequested(true);
    invalidateReceipt();
  }
  function undoClearKey() {
    setForm(prev => ({ ...prev, api_key: mainKeyOriginalMasked }));
    setClearKeyRequested(false);
  }

  // ── 多 Key 管理（负载均衡）─────────────────────────────────────────────

  // 主 Key 编辑模式：点钥匙进入，可看到/修改真实值；点确认退回缩略展示
  async function enterMainKeyEdit() {
    setMainKeyEditing(true);
    // 进入编辑后拉取真实 key 替换掩码值（复制模式不需要，因为没有原渠道）
    if (editing && !duplicate) {
      try {
        const fullValue = await channelApi.getApiKey(editing.id);
        onKeyChange(fullValue);
      } catch { /* ignore */ }
    }
  }
  function exitMainKeyEdit() {
    // 退出编辑：如果用户没改过（仍是真实值），恢复掩码值用于展示
    if (mainKeyOriginalMasked && form.api_key !== mainKeyOriginalMasked) {
      // 用户改过了，保留真实值（保存时会提交）
    }
    setMainKeyEditing(false);
    invalidateReceipt();
  }

  function addExtraKey() {
    const tempId = `new_${Date.now()}_${Math.random().toString(36).slice(2, 8)}`;
    setForm(prev => ({
      ...prev,
      extra_keys: [...prev.extra_keys, {
        id: tempId,
        api_key: "",
        weight: 1,
        enabled: true,
        isEditing: true,
        isRevealed: true,
        isExisting: false,
        rawValue: "",
      }],
    }));
  }

  function removeExtraKey(keyId: string) {
    setForm(prev => ({
      ...prev,
      extra_keys: prev.extra_keys.filter(k => k.id !== keyId),
    }));
    invalidateReceipt();
  }

  function updateExtraKeyField(keyId: string, field: "api_key" | "weight" | "enabled", value: string | number | boolean) {
    setForm(prev => ({
      ...prev,
      extra_keys: prev.extra_keys.map(k =>
        k.id === keyId ? { ...k, [field]: value } : k,
      ),
    }));
    invalidateReceipt();
  }

  async function toggleExtraKeyEdit(keyId: string) {
    const item = form.extra_keys.find(k => k.id === keyId);
    if (!item) return;
    if (!item.isEditing && item.isExisting) {
      // 进入编辑时拉取真实 key 替换掩码值
      try {
        const fullValue = await channelApi.getExtraKeyValue(keyId);
        setForm(prev => ({
          ...prev,
          extra_keys: prev.extra_keys.map(k =>
            k.id === keyId ? { ...k, isEditing: true, api_key: fullValue } : k,
          ),
        }));
        return;
      } catch { /* ignore */ }
    }
    setForm(prev => ({
      ...prev,
      extra_keys: prev.extra_keys.map(k =>
        k.id === keyId ? { ...k, isEditing: !k.isEditing } : k,
      ),
    }));
  }

  async function toggleExtraKeyStatus(keyId: string) {
    const item = form.extra_keys.find(k => k.id === keyId);
    if (!item || !item.isExisting) return;
    const newStatus = item.enabled ? 0 : 1;
    try {
      await channelApi.toggleExtraKey(keyId, newStatus);
      updateExtraKeyField(keyId, "enabled", !item.enabled);
    } catch {
      // ignore
    }
  }

  // ── 模型列表 ────────────────────────────────────────────────────────────
  function addModel() {
    const m = modelInput.trim();
    if (!m) return;
    if (!form.models.includes(m)) onModelListChange([...form.models, m]);
    setModelInput("");
  }
  function removeModel(m: string) {
    onModelListChange(form.models.filter(x => x !== m));
  }

  // ── legacy type/base_url 兼容字段 ────────────────────────────────────────
  function legacyType(): string {
    // 旧 Gemini 原生配置保留 type=gemini（后端 new_to_legacy 同规则）。
    if (form.legacy_executor_override === "gemini_native") return "gemini";
    return currentPreset?.legacy_type ?? (form.protocol === "anthropic" ? "claude" : "openai");
  }
  function legacyBaseUrl(): string {
    // 旧 Gemini 原生配置：保持原始 native 根（后端 new_to_legacy 同规则）。
    if (form.legacy_executor_override === "gemini_native") return form.native_base_url || "";
    if (currentPreset?.legacy_base_url) return currentPreset.legacy_base_url;
    // 自定义预设 legacy_base_url 为空：按后端 T02 推导约定生成旧代码兼容根（F6）。
    return deriveLegacyBaseUrl(form.protocol, form.native_base_url);
  }

  function buildDraftInput(): DraftChannelTestInput {
    // 编辑场景：如果 api_key 仍是掩码值（用户未修改），不传给后端测试，
    // 让 resolve_draft_api_key 走「编辑留空回填已存 Key」路径拿到真实 Key。
    // 与 buildSaveInput 的保存逻辑保持一致，避免用掩码值当真实 Key 探测。
    const draftApiKey =
      editing && form.api_key === mainKeyOriginalMasked ? "" : form.api_key;
    return {
      id: editing?.id,
      name: form.name,
      type: legacyType(),
      base_url: legacyBaseUrl(),
      api_key: draftApiKey,
      // 让草稿测试在后端与保存路径解析出相同的有效 Key：
      // 编辑留空未清除 → 沿用已存 Key；显式清除 → 空 Key。
      clear_api_key: clearKeyRequested || undefined,
      models: form.models,
      priority: form.priority,
      weight: form.weight,
      model_mapping: form.model_mapping,
      timeout_secs: form.timeout_secs,
      protocol: form.protocol,
      provider: form.provider,
      native_base_url: form.native_base_url,
      native_endpoints: form.native_endpoints,
      preset_revision: form.preset_revision || undefined,
      legacy_executor_override: form.legacy_executor_override,
    };
  }

  // ── T14 同步上游模型流程 ────────────────────────────────────────────────
  function showToast(msg: string) {
    setToastMsg(msg);
    if (toastTimer.current) clearTimeout(toastTimer.current);
    toastTimer.current = setTimeout(() => setToastMsg(null), 2600);
  }

  async function handleSync() {
    if (saving || syncState === "loading") return;
    setSyncState("loading");
    setSyncError(null);
    try {
      // 后端绝不写库；api_key 复用草稿语义（编辑留空回填已存 Key）。
      const result = await channelApi.syncUpstreamModels(buildDraftInput());
      setSyncResult(result);
    } catch (e) {
      setSyncError(String(e));
    } finally {
      setSyncState("idle");
    }
  }

  /** 合并去重（保序：已有顺序 + 新增追加），新增 chip 高亮 + toast。 */
  function applySync(selected: string[]) {
    const existing = new Set(form.models);
    const toAdd = selected.filter(m => !existing.has(m));
    if (toAdd.length === 0) { setSyncResult(null); return; }
    onModelListChange([...form.models, ...toAdd]);
    setAddedModels(toAdd);
    setSyncResult(null);
    showToast(`已添加 ${toAdd.length} 个模型到「${form.name || "渠道"}」`);
    if (highlightTimer.current) clearTimeout(highlightTimer.current);
    highlightTimer.current = setTimeout(() => setAddedModels([]), 1800);
  }

  // 卸载时清理 toast/高亮计时器。
  useEffect(() => () => {
    if (toastTimer.current) clearTimeout(toastTimer.current);
    if (highlightTimer.current) clearTimeout(highlightTimer.current);
  }, []);

  type ReceiptFields = { test_run_id: string; draft_fingerprint: string; force_save: boolean };

  function receiptFields(result: DraftChannelTestResult | null, forceSave: boolean): ReceiptFields | null {
    if (!result) return null;
    return {
      test_run_id: result.test_run_id,
      draft_fingerprint: result.draft_fingerprint,
      force_save: forceSave,
    };
  }

  function buildCreateInput(rf: ReceiptFields | null): CreateChannelInput {
    return {
      name: form.name,
      type: legacyType(),
      base_url: legacyBaseUrl(),
      api_key: form.api_key,
      models: form.models,
      priority: form.priority,
      weight: form.weight,
      model_mapping: form.model_mapping,
      timeout_secs: form.timeout_secs,
      protocol: form.protocol,
      provider: form.provider,
      native_base_url: form.native_base_url,
      native_endpoints: form.native_endpoints,
      preset_revision: form.preset_revision || undefined,
      legacy_executor_override: form.legacy_executor_override,
      // Multi-key: send extra keys that have a non-empty api_key value
      extra_keys: form.extra_keys
        .filter(k => k.api_key.trim() !== "")
        .map(k => ({
          api_key: k.api_key,
          weight: k.weight,
          status: k.enabled ? 1 : 0,
        })),
      ...(rf ?? {}),
    };
  }

  function buildUpdateInput(rf: ReceiptFields | null): UpdateChannelInput {
    return {
      id: editing!.id,
      name: form.name,
      models: form.models,
      priority: form.priority,
      weight: form.weight,
      model_mapping: form.model_mapping,
      timeout_secs: form.timeout_secs,
      // F3：始终写回解析后的身份（type/base_url/protocol/provider/native_*）。
      // 对 legacy（identity_revision 0）渠道，保存即迁移；对已迁移渠道为幂等写。
      // 配合 F2（isEndpoint 保留 count_tokens/embeddings），编辑保存不再剥离能力端点。
      type: legacyType(),
      base_url: legacyBaseUrl(),
      protocol: form.protocol,
      provider: form.provider,
      native_base_url: form.native_base_url,
      native_endpoints: form.native_endpoints,
      preset_revision: form.preset_revision || undefined,
      legacy_executor_override: form.legacy_executor_override,
      // 编辑留空 = 不修改；掩码值未改 = 不修改；显式清除走 clear_api_key 标记。
      ...((form.api_key.trim() !== "" && form.api_key !== mainKeyOriginalMasked) ? { api_key: form.api_key } : {}),
      ...(clearKeyRequested ? { clear_api_key: true } : {}),
      // Multi-key: full-replace semantics — send all extra keys
      extra_keys: form.extra_keys
        .filter(k => k.api_key.trim() !== "")
        .map(k => ({
          api_key: k.api_key,
          weight: k.weight,
          status: k.enabled ? 1 : 0,
        })),
      ...(rf ?? {}),
    };
  }

  // ── 保存流程：本地校验 → 草稿测试 → 全过自动保存 / 失败弹窗强制保存 ───────
  function validate(): string | null {
    if (!form.name.trim()) return "名称不能为空";
    if (!/^https?:\/\//i.test(form.native_base_url.trim())) {
      return "Base URL 必须是 http(s) 地址";
    }
    if (form.protocol === "openai" && form.native_endpoints.length === 0) {
      return "OpenAI 协议至少勾选一个端点（Chat Completions 或 Responses）";
    }
    if (form.protocol === "anthropic" && !form.native_endpoints.includes("messages")) {
      return "Anthropic 协议必须包含 /messages 端点";
    }
    if (form.protocol === "ollama" && !form.native_endpoints.includes("api_chat")) {
      return "Ollama 协议必须包含 /api/chat 端点";
    }
    if ((!editing || duplicate) && keyRequired && !form.api_key.trim()) {
      return "API Key 不能为空";
    }
    return null;
  }

  async function doSave(result: DraftChannelTestResult | null, forceSave: boolean) {
    if (saving) return;
    setSaving(true);
    setSaveError(null);
    try {
      const rf = receiptFields(result, forceSave);
      if (editing && !duplicate) await channelApi.update(buildUpdateInput(rf));
      else await channelApi.create(buildCreateInput(rf));
      onSaved();
    } catch (e) {
      const msg = `保存失败：${String(e)}`;
      setSaveError(msg);
      setLocalError(msg);
      setSaving(false);
      // 自动保存（全过）失败：回到 idle 关闭测试弹窗，展示表单错误。
      // 强制保存失败：保持 failed 弹窗，展示 saveError 供重试。
      setTestPhase(prev => (prev === "running" ? "idle" : prev));
    }
  }

  async function handleSubmit(e: React.FormEvent) {
    e.preventDefault();
    if (saving || testPhase === "running") return;
    const err = validate();
    if (err) { setLocalError(err); return; }
    setLocalError(null);
    setSaveError(null);
    setTestPhase("running");
    setTestResult(null);
    try {
      const result = await channelApi.testDraft(buildDraftInput());
      setReceipt(result);
      const allPassed = result.results.length > 0 && result.results.every(r => r.status === "passed");
      if (allPassed) {
        await doSave(result, false);
      } else {
        setTestResult(result);
        setTestPhase("failed");
      }
    } catch (e) {
      setTestPhase("failed");
      setTestResult(null);
      setLocalError(`连通性测试失败：${String(e)}`);
    }
  }

  async function handleForceSave() {
    await doSave(receipt ?? testResult, true);
  }

  // ── 渲染用派生数据 ───────────────────────────────────────────────────────
  function onTabKeyDown(e: React.KeyboardEvent, idx: number) {
    if (e.key === "ArrowRight") { e.preventDefault(); requestProtocolSwitch(availableProtocols[(idx + 1) % availableProtocols.length]); }
    else if (e.key === "ArrowLeft") { e.preventDefault(); requestProtocolSwitch(availableProtocols[(idx - 1 + availableProtocols.length) % availableProtocols.length]); }
    else if (e.key === "Home") { e.preventDefault(); requestProtocolSwitch(availableProtocols[0]); }
    else if (e.key === "End") { e.preventDefault(); requestProtocolSwitch(availableProtocols[availableProtocols.length - 1]); }
  }

  const editingLegacy = editing !== null && editing.identity_revision === 0;

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 p-4 backdrop-blur-sm" onClick={onClose}>
      <div className="surface w-full max-w-2xl max-h-[92vh] overflow-auto rounded-[28px]" onClick={e => e.stopPropagation()}>
        <div className="flex items-center justify-between border-b border-border px-5 py-4 sticky top-0 bg-inherit z-20">
          <h2 className="text-lg font-semibold">{duplicate ? "复制渠道" : editing ? "编辑渠道" : "新建渠道"}</h2>
          <button onClick={onClose} disabled={saving} className="action-secondary px-3 py-2"><X size={18} /></button>
        </div>

        <form
          onSubmit={handleSubmit}
          className="space-y-5 p-5"
          onKeyDown={e => { if (e.key === "Enter" && (e.nativeEvent.isComposing || e.keyCode === 229)) e.preventDefault(); }}
        >
          {/* 协议 Tab */}
          <div>
            <label className="mb-2 block text-sm font-medium">协议</label>
            <div role="tablist" aria-label="协议" className="grid grid-cols-3 gap-2 rounded-2xl bg-muted p-1.5">
              {availableProtocols.map((p, idx) => {
                const active = form.protocol === p;
                return (
                  <button
                    key={p}
                    type="button"
                    role="tab"
                    id={`protocol-tab-${p}`}
                    aria-selected={active}
                    aria-controls={`protocol-panel-${p}`}
                    tabIndex={active ? 0 : -1}
                    onClick={() => requestProtocolSwitch(p)}
                    onKeyDown={e => onTabKeyDown(e, idx)}
                    className={`rounded-xl px-4 py-2.5 text-sm font-semibold transition-all ${
                      active
                        ? "bg-white text-primary shadow-sm"
                        : "text-muted-foreground hover:text-foreground"
                    }`}
                  >
                    {PROTOCOL_LABELS[p]}
                  </button>
                );
              })}
            </div>
          </div>

          {editingLegacy && (
            <div className="rounded-xl border border-amber-200 bg-amber-50 px-3 py-2 text-xs text-amber-700">
              来自旧配置：该渠道身份由旧 type/base_url 推导；保存后才写入新的 protocol/provider 字段。
            </div>
          )}

          {/* 名称 */}
          <div>
            <label className="mb-2 block text-sm font-medium">名称</label>
            <input
              value={form.name}
              onChange={e => { setNameTouched(true); autoNameRef.current = null; setForm(prev => ({ ...prev, name: e.target.value })); }}
              className="w-full rounded-2xl border border-border bg-background/70 px-4 py-3 text-sm"
              placeholder="渠道名称"
              required
            />
          </div>

          {/* 渠道提供商选择器 */}
          <div>
            <label className="mb-2 block text-sm font-medium">渠道提供商</label>
            {presetsLoading ? (
              <div className="flex items-center gap-2 rounded-2xl border border-dashed border-border bg-background/40 px-4 py-5 text-sm text-muted-foreground">
                <Loader2 size={15} className="animate-spin" /> 正在加载提供商模板…
              </div>
            ) : presetsError ? (
              <div className="rounded-2xl border border-red-200 bg-red-50 px-4 py-3 text-sm text-red-700">
                提供商模板加载失败（{presetsError}）。已禁用厂商预设，可继续使用自定义配置手动填写；恢复后刷新重试。
              </div>
            ) : (
              <ProviderDropdown
                presets={presetGroups.find(g => g.protocol === form.protocol)?.presets ?? []}
                current={form.provider}
                onSelect={selectProvider}
              />
            )}
          </div>

          {/* 协议配置区 */}
          <div id={`protocol-panel-${form.protocol}`} role="tabpanel" aria-labelledby={`protocol-tab-${form.protocol}`}>
            {/* Base URL */}
            <div>
              <label className="mb-2 block text-sm font-medium">Base URL</label>
              <input
                value={form.native_base_url}
                onChange={e => onUrlChange(e.target.value)}
                className="w-full rounded-2xl border border-border bg-background/70 px-4 py-3 text-sm font-mono"
                placeholder={form.protocol === "ollama" ? "http://localhost:11434" : "https://api.example.com"}
                required
              />
              <p className="mt-1.5 text-xs text-muted-foreground">{PROTOCOL_BASE_URL_HINTS[form.protocol]}</p>
            </div>

            {/* 端点 */}
            <div className="mt-4">
              <label className="mb-2 block text-sm font-medium">端点</label>
              {form.protocol === "openai" ? (
                <div className="flex flex-wrap gap-2.5">
                  {PROTOCOL_ENDPOINT_OPTIONS.openai.map(ep => (
                    <label key={ep} className={`flex items-center gap-2 rounded-[14px] border px-3.5 py-2.5 text-[13px] transition-all ${form.native_endpoints.includes(ep) ? "border-primary/40 bg-primary/8 font-medium text-primary" : "border-border bg-background/40 hover:border-primary/30"}`}>
                      <input
                        type="checkbox"
                        checked={form.native_endpoints.includes(ep)}
                        onChange={e => toggleEndpoint(ep, e.target.checked)}
                        className="h-4 w-4 accent-[#2f6fed]"
                      />
                      <span className="shrink-0 font-medium">{ENDPOINT_LABELS[ep]}</span>
                      <span className="font-mono text-xs text-muted-foreground">{ENDPOINT_PATHS[ep]}</span>
                    </label>
                  ))}
                </div>
              ) : (
                <div className="space-y-2.5">
                  {(form.protocol === "anthropic" ? ["messages"] : ["api_chat"]).map(ep => (
                    <label key={ep} className="flex cursor-default items-center gap-2 rounded-[14px] border border-border bg-background/40 px-3.5 py-2.5 text-[13px]">
                      <input type="checkbox" checked disabled className="h-4 w-4 accent-[#2f6fed]" />
                      <span className="shrink-0 font-semibold">{ENDPOINT_LABELS[ep]}</span>
                      <span className="font-mono text-xs text-muted-foreground">{ENDPOINT_PATHS[ep]}</span>
                    </label>
                  ))}
                </div>
              )}
            </div>

            {/* 实际请求 URL 预览：Base URL + 端点路径，随输入实时派生。纯文本靠左；
                count_tokens 不在表单展示（T06 legacy 推断的能力保留，仅 UI 隐藏）。 */}
            <div className="mt-4">
              <label className="mb-2 block text-sm font-medium">实际请求 URL</label>
              {form.native_base_url.trim() === "" ? (
                <p className="text-xs text-muted-foreground">填写 Base URL 后显示各端点的实际请求地址</p>
              ) : (
                <ul className="space-y-1.5 text-xs text-muted-foreground">
                  {form.native_endpoints.filter(ep => ep !== "count_tokens").map(ep => (
                    <li key={ep} className="text-left">
                      <span className="font-medium text-foreground">{ENDPOINT_LABELS[ep]}</span>{" "}
                      <code className="break-all font-mono">{joinUrl(form.native_base_url, ENDPOINT_PATHS[ep])}</code>
                    </li>
                  ))}
                </ul>
              )}
            </div>

            {/* API Keys（负载均衡）：主 Key（#1）+ 额外 Keys 统一管理 */}
            <div className="mt-4">
              <div className="mb-2 flex items-center justify-between">
                <label className="block text-sm font-medium">API Keys（负载均衡）</label>
                <button
                  type="button"
                  onClick={addExtraKey}
                  className="flex items-center gap-1 rounded-lg border border-primary/30 bg-primary/8 px-2.5 py-1.5 text-xs font-medium text-primary transition-all hover:bg-primary/12"
                >
                  <Plus size={13} /> 添加 Key
                </button>
              </div>
              <div className="space-y-2.5">
                {/* 主 Key 行：#1，权重联动渠道权重；无删除/启停（后端无主 Key 独立启停概念） */}
                <div className="rounded-xl border border-border bg-background/50 px-3.5 py-3">
                  <div className="flex items-center gap-2">
                    <span className="shrink-0 text-xs font-mono text-muted-foreground w-6">#1</span>
                    <span className="shrink-0 rounded-md bg-primary/12 px-1.5 py-0.5 text-[11px] font-semibold text-primary">主</span>

                    {/* 主 Key 显示（编辑态缩略展示 + 复制；点钥匙进入输入模式看真实值） */}
                    {editing && !clearKeyRequested && !mainKeyEditing && (
                      <code className="min-w-0 flex-1 truncate rounded-lg border border-border bg-background/40 px-3 py-2 text-sm font-mono text-muted-foreground">
                        {form.api_key.trim() !== "" ? maskKey(form.api_key) : "未设置"}
                      </code>
                    )}
                    {(!editing || clearKeyRequested || mainKeyEditing) && (
                      <input
                        type="text"
                        value={form.api_key}
                        onChange={e => onKeyChange(e.target.value)}
                        className="min-w-0 flex-1 rounded-lg border border-border bg-background/70 px-3 py-2 text-sm font-mono"
                        placeholder={(editing && !duplicate) ? (clearKeyRequested ? "将清除已保存的 Key" : "留空则不修改") : keyRequired ? "sk-..." : "可留空（本地/自管 Ollama）"}
                        autoCapitalize="none"
                        autoComplete="off"
                        spellCheck={false}
                      />
                    )}

                    {/* 复制 */}
                    {editing && !duplicate && !clearKeyRequested && !mainKeyEditing && (
                      <button
                        type="button"
                        onClick={async () => {
                          try {
                            const v = await channelApi.getApiKey(editing.id);
                            await copyToClipboard(v);
                            showToast("已复制主 Key");
                          } catch { /* ignore */ }
                        }}
                        className="action-secondary shrink-0 p-1.5"
                        title="复制"
                      >
                        <Copy size={14} />
                      </button>
                    )}

                    {/* 主 Key 权重：与渠道权重联动 */}
                    <input
                      type="number"
                      min={1}
                      max={100}
                      value={form.weight}
                      onChange={e => setForm(prev => ({ ...prev, weight: parseInt(e.target.value) || 1 }))}
                      className="w-16 shrink-0 rounded-lg border border-border bg-background/70 px-2 py-2 text-center text-sm"
                      title="主 Key 权重（联动渠道权重）"
                    />

                    {/* 钥匙编辑 / 确认编辑 */}
                    {editing && !clearKeyRequested && !mainKeyEditing && (
                      <button
                        type="button"
                        onClick={enterMainKeyEdit}
                        className="action-secondary shrink-0 p-1.5"
                        title="编辑查看"
                      >
                        <KeyRound size={14} />
                      </button>
                    )}
                    {editing && mainKeyEditing && !clearKeyRequested && (
                      <button type="button" onClick={exitMainKeyEdit} title="确认" className="action-secondary shrink-0 p-1.5">
                        <Check size={14} />
                      </button>
                    )}

                    {/* 清除 / 确认新输入 / 撤销 */}
                    {editing && !mainKeyEditing && !clearKeyRequested && (
                      <button type="button" onClick={requestClearKey} title="清除已保存的 API Key" className="action-secondary shrink-0 p-1.5">
                        <Trash2 size={14} />
                      </button>
                    )}
                    {editing && clearKeyRequested && form.api_key.trim() !== "" && (
                      <button type="button" onClick={() => setClearKeyRequested(false)} title="确认新输入的 Key" className="action-secondary shrink-0 p-1.5">
                        <Check size={14} />
                      </button>
                    )}
                    {editing && clearKeyRequested && (
                      <button type="button" onClick={undoClearKey} className="action-secondary shrink-0 p-1.5" title="撤销清除，保留原 Key">
                        <Undo size={14} />
                      </button>
                    )}
                  </div>
                  {form.protocol === "ollama" && (
                    <p className="mt-1.5 text-xs text-muted-foreground">Ollama 本地默认无 API Key，可留空；远程反向代理可填写。</p>
                  )}
                  {!keyRequired && form.protocol !== "ollama" && (
                    <p className="mt-1.5 text-xs text-muted-foreground">该提供商为可选鉴权（如 Ollama 接口），API Key 可留空。</p>
                  )}
                </div>

                {/* 额外 Key 行 */}

                {form.extra_keys.map((k, idx) => (
                    <div
                      key={k.id}
                      className={`rounded-xl border px-3.5 py-3 transition-all ${k.enabled ? "border-border bg-background/50" : "border-border bg-muted/30 opacity-60"}`}
                    >
                      {/* Key 行：序号 + 徽标 + 输入/显示 + 权重 + 操作 */}
                      <div className="flex items-center gap-2">
                        <span className="shrink-0 text-xs font-mono text-muted-foreground w-6">#{idx + 2}</span>
                        <span className="shrink-0 rounded-md bg-muted px-1.5 py-0.5 text-[11px] font-semibold text-muted-foreground">从</span>

                        {/* Key 输入/显示 */}
                        {k.isEditing ? (
                          <input
                            type="text"
                            value={k.api_key}
                            onChange={e => updateExtraKeyField(k.id, "api_key", e.target.value)}
                            className="min-w-0 flex-1 rounded-lg border border-border bg-background/70 px-3 py-2 text-sm font-mono"
                            placeholder="sk-..."
                            autoCapitalize="none"
                            autoComplete="off"
                            spellCheck={false}
                          />
                        ) : (
                          <code className="min-w-0 flex-1 truncate rounded-lg border border-border bg-background/40 px-3 py-2 text-sm font-mono text-muted-foreground">
                            {maskKey(k.api_key)}
                          </code>
                        )}

                        {/* 复制 */}
                        {k.isExisting && !k.isEditing && (
                          <button
                            type="button"
                            onClick={async () => {
                              try {
                                const v = await channelApi.getExtraKeyValue(k.id);
                                await copyToClipboard(v);
                                showToast(`已复制从 Key #${idx + 2}`);
                              } catch { /* ignore */ }
                            }}
                            className="action-secondary shrink-0 p-1.5"
                            title="复制"
                          >
                            <Copy size={14} />
                          </button>
                        )}

                        {/* 权重 */}
                        <input
                          type="number"
                          min={1}
                          max={100}
                          value={k.weight}
                          onChange={e => updateExtraKeyField(k.id, "weight", parseInt(e.target.value) || 1)}
                          className="w-16 shrink-0 rounded-lg border border-border bg-background/70 px-2 py-2 text-center text-sm"
                          title="权重"
                        />

                        {/* 操作按钮 */}
                        {k.isEditing ? (
                          <button
                            type="button"
                            onClick={() => toggleExtraKeyEdit(k.id)}
                            className="action-secondary shrink-0 p-1.5"
                            title="确认"
                          >
                            <Check size={14} />
                          </button>
                        ) : (
                          <button
                            type="button"
                            onClick={() => toggleExtraKeyEdit(k.id)}
                            className="action-secondary shrink-0 p-1.5"
                            title="编辑"
                          >
                            <KeyRound size={14} />
                          </button>
                        )}

                        {/* 启用/禁用（仅已存在的 key）*/}
                        {k.isExisting && (
                          <button
                            type="button"
                            onClick={() => toggleExtraKeyStatus(k.id)}
                            className={`shrink-0 p-1.5 transition-colors ${k.enabled ? "text-green-500 hover:text-green-600" : "text-muted-foreground hover:text-foreground"}`}
                            title={k.enabled ? "禁用" : "启用"}
                          >
                            <Power size={14} />
                          </button>
                        )}

                        {/* 删除 */}
                        <button
                          type="button"
                          onClick={() => removeExtraKey(k.id)}
                          className="shrink-0 p-1.5 text-muted-foreground transition-colors hover:text-red-500"
                          title="删除"
                        >
                          <Trash2 size={14} />
                        </button>
                      </div>
                    </div>
                  ))}
                <p className="text-xs text-muted-foreground">
                  💡 主 Key（#1）+ 额外 Keys 共同参与负载均衡，按权重随机选择。失效 Key 自动降级，请求转发至其他可用 Key。
                </p>
              </div>
            </div>
          </div>

          {/* 模型列表 */}
          <div>
            <label className="mb-2 block text-sm font-medium">模型列表</label>
            <div className="mb-3 flex flex-wrap gap-2">
              <input
                value={modelInput}
                onChange={e => setModelInput(e.target.value)}
                onKeyDown={e => { if (e.key === "Enter") { e.preventDefault(); if (!e.nativeEvent.isComposing && e.keyCode !== 229) addModel(); } }}
                className="min-w-[200px] flex-1 rounded-2xl border border-border bg-background/70 px-4 py-3 text-sm"
                placeholder="输入模型名称，回车添加"
              />
              <button type="button" onClick={addModel} className="action-secondary px-4 py-3"><Plus size={16} /></button>
              <button
                type="button"
                onClick={handleSync}
                disabled={syncState === "loading" || saving}
                title="按协议拉取上游模型列表，弹窗勾选后合并进模型列表；失败时不会覆盖已有模型列表"
                className="action-secondary px-3 py-3 disabled:cursor-not-allowed disabled:opacity-40"
              >
                {syncState === "loading"
                  ? <Loader2 size={14} className="animate-spin" />
                  : <RefreshCw size={14} />}
                同步上游模型
              </button>
            </div>
            {syncError && (
              <p className="mb-2 rounded-xl border border-red-200 bg-red-50 px-3 py-2 text-xs text-red-700">
                同步失败：{syncError}
              </p>
            )}
            <div className="flex flex-wrap gap-2">
              {form.models.map(m => (
                <span
                  key={m}
                  className="inline-flex items-center gap-1 rounded-full bg-primary/12 px-3 py-1.5 text-xs text-primary"
                  style={addedModels.includes(m) ? { animation: "model-pop 1.8s ease" } : undefined}
                >
                  {m}
                  <button type="button" onClick={() => removeModel(m)} className="hover:text-red-300"><X size={12} /></button>
                </span>
              ))}
            </div>
            {form.models.length === 0 && (
              <p className="mt-1.5 text-xs text-muted-foreground">空模型列表表示「接受所有模型」（通配）。</p>
            )}
          </div>

          {/* 模型映射 */}
          <MappingSection
            value={form.model_mapping}
            availableTargets={form.models}
            onChange={(mapping) => setForm(prev => ({ ...prev, model_mapping: mapping }))}
          />

          {/* 优先级 + 权重 */}
          <div className="grid grid-cols-1 gap-4 md:grid-cols-2">
            <div>
              <label className="mb-2 block text-sm font-medium">优先级</label>
              <input
                type="number"
                value={form.priority}
                onChange={e => setForm(prev => ({ ...prev, priority: parseInt(e.target.value) || 0 }))}
                className="w-full rounded-2xl border border-border bg-background/70 px-4 py-3 text-sm"
              />
              <p className="mt-1.5 text-xs text-muted-foreground">数字越大优先级越高，相同映射名的请求会优先路由到高优先级渠道</p>
            </div>
            <div>
              <label className="mb-2 block text-sm font-medium">权重</label>
              <input
                type="number"
                value={form.weight}
                onChange={e => setForm(prev => ({ ...prev, weight: parseInt(e.target.value) || 1 }))}
                className="w-full rounded-2xl border border-border bg-background/70 px-4 py-3 text-sm"
              />
              <p className="mt-1.5 text-xs text-muted-foreground">同优先级渠道间的负载均衡比例，数值越大分配的请求越多</p>
            </div>
          </div>

          {/* 超时 */}
          <div>
            <label className="mb-2 block text-sm font-medium">请求超时时间（秒）</label>
            <input
              type="number"
              min={1}
              max={600}
              value={form.timeout_secs}
              onChange={e => onTimeoutChange(Math.max(1, parseInt(e.target.value) || 60))}
              className="w-full rounded-2xl border border-border bg-background/70 px-4 py-3 text-sm"
            />
            <p className="mt-1.5 text-xs text-muted-foreground">非流式请求的超时时间（默认 300 秒）。流式请求仅限制连接建立时间，不受此限制。超时后会自动重试下一个渠道</p>
          </div>

          {localError && (
            <div className="flex items-center justify-between rounded-2xl border border-red-200 bg-red-50 px-4 py-3 text-sm text-red-700">
              <span>{localError}</span>
              <button type="button" onClick={() => setLocalError(null)} className="ml-3 shrink-0 text-red-400 transition-colors hover:text-red-600"><X size={16} /></button>
            </div>
          )}

          {/* Actions */}
          <div className="flex items-center justify-between gap-3 pt-2">
            <p className="text-xs leading-5 text-muted-foreground">
              保存前会逐端点发送最小推理请求验证（<span className="font-medium">可能产生极少上游费用</span>）；失败时可选择「仍然保存」。
            </p>
            <div className="flex shrink-0 gap-2">
              <button type="button" onClick={onClose} disabled={saving} className="action-secondary">取消</button>
              <button type="submit" disabled={saving || testPhase === "running"} className="action-primary">
                {saving ? <Loader2 size={16} className="animate-spin" /> : <Check size={16} />}
                {saving ? "保存中…" : "保存"}
              </button>
            </div>
          </div>
        </form>
      </div>

      {/* 草稿测试弹窗 */}
      {(testPhase === "running" || (testPhase === "failed" && testResult)) && (
        <DraftTestModal
          phase={testPhase}
          result={testResult}
          saving={saving}
          saveError={saveError}
          onModify={() => { setTestPhase("idle"); setTestResult(null); setSaveError(null); }}
          onForceSave={handleForceSave}
        />
      )}

      {/* T14 同步上游模型弹窗（仅在拉取成功后打开；失败走表单内错误提示，不覆盖列表） */}
      {syncResult && (
        <ModelSyncModal
          result={syncResult}
          channelName={form.name || "渠道"}
          existingModels={form.models}
          onApply={applySync}
          onClose={() => setSyncResult(null)}
        />
      )}

      {/* 应用后 toast */}
      {toastMsg && (
        <div className="fixed bottom-6 right-6 z-50 flex items-center gap-2 rounded-2xl bg-slate-900 px-4 py-2.5 text-sm font-medium text-white shadow-xl animate-[fadeInUp_0.2s_ease]">
          <Check size={15} className="text-emerald-400" />
          {toastMsg}
        </div>
      )}
    </div>
  );
}
