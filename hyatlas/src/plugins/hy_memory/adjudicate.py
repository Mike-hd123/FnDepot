"""Conflict adjudication wrapper for HyAtlas v4 (P0-b).

Implements impl-spec §4.1: on every agent-side write, search neighbours,
ask an LLM (four-way: add / supersede / coexist / reject_dup), and apply
the verdict with pure metadata fields (supersede_ids / superseded_by /
valid_until / pin) — no new server-side semantics, per the
"field-shaped, code disposable" rule.

Shared by the Hermes plugin (__init__.py write hooks) and the CLI
(cli.py add) so both entry points go through one pipeline.

Config via env (all optional; defaults follow spec):
  HYADJ_LLM_BASE_URL   e.g. http://127.0.0.1:8081/v1   (empty -> adjudication off)
  HYADJ_LLM_API_KEY    bearer key for the above
  HYADJ_LLM_MODEL      default "auto"
  HYADJ_ENABLED        "0" kills the pipeline entirely (default "1")
  HYADJ_PREFILTER      neighbour score cut, default 0.80
  HYADJ_NEIGHBOR_LIMIT neighbours pulled per write, default 8
  HYADJ_TIMEOUT        LLM timeout seconds, default 30
  HYADJ_LAYERS         comma layers the pipeline may supersede, default l1_profile,l3_fact

Fail-open contract: any error (LLM down, bad JSON, timeout) degrades to
a plain ADD — memories must never be lost because adjudication broke.
"""
from __future__ import annotations

import json
import logging
import os
import re
import time
import urllib.error
import urllib.request
from datetime import datetime, timezone, timedelta
from pathlib import Path
from typing import Any, Dict, List, Optional, Tuple

logger = logging.getLogger(__name__)

_VERDICTS = ("add", "supersede", "coexist", "reject_dup")

_JUDGE_SYSTEM = (
    "你是记忆库冲突裁决器。给定【新记忆】与若干【已有记忆】(含 id)，判断新记忆与已有记忆的关系，"
    "只输出一个 JSON 对象，不要解释、不要代码块。规则：\n"
    "- supersede: 新记忆与某些已有记忆讲同一主题同一属性但结论矛盾/状态更新（新推翻旧），victim_ids 列出被推翻的已有记忆 id；\n"
    "- reject_dup: 新记忆与某条已有记忆语义完全重复，没有新信息；\n"
    "- coexist: 与已有记忆同域但不矛盾、不该互相覆盖；\n"
    "- add: 与已有记忆无关，或无可比对象。\n"
    "拿不准时选 add。输出格式：{\"verdict\":\"add|supersede|coexist|reject_dup\","
    "\"victim_ids\":[\"...\"],\"reason\":\"一句话\"}"
)


def _fmt_ts(v) -> str:
    """Search rows carry gmt_created as int unix seconds (or ISO str);
    normalise for the judge prompt."""
    if v in (None, ""):
        return ""
    try:
        return datetime.fromtimestamp(float(v), timezone(timedelta(hours=8))
                                      ).isoformat(timespec="seconds")
    except (TypeError, ValueError, OSError):
        return str(v)[:19]


def _cutoff(ms: int = 0) -> str:
    tz = timezone(timedelta(hours=8))  # NAS local CST; server parses RFC3339
    return datetime.now(tz).isoformat(timespec="milliseconds")


def _env(name: str, default: str = "") -> str:
    return os.environ.get(name, default).strip()


def _flatten_hits(resp: Any) -> List[Dict[str, Any]]:
    """Search responses are 3-channel nested ({memories:{profile:[],...}} or
    flat lists depending on server version). Collect every hit dict."""
    hits: List[Dict[str, Any]] = []
    seen = set()

    def walk(o: Any) -> None:
        if isinstance(o, dict):
            if "memory_id" in o or "id" in o:
                rid = str(o.get("memory_id") or o.get("id") or "")
                if rid and rid not in seen:
                    seen.add(rid)
                    hits.append(o)
                return
            for v in o.values():
                walk(v)
        elif isinstance(o, list):
            for x in o:
                walk(x)

    walk(resp)
    hits.sort(key=lambda h: -float(h.get("score") or 0))
    return hits


def _discover_llm() -> Tuple[str, str, str]:
    """Fall back to Hermes config.yaml custom_providers (local Octopus gateway).

    Returns (base_url, api_key, model). Prefers a provider whose name or
    base_url mentions the local loopback port; any entry works since the
    gateway routes model names itself. Never raises.
    """
    home = os.environ.get("HERMES_HOME", "")
    candidates = []
    if home:
        candidates.append(Path(home) / "config.yaml")
    candidates.append(Path("/vol1/@apphome/hermes-studio/hermes-home/config.yaml"))
    try:
        import yaml  # bundled with hermes; optional here
        for p in candidates:
            if not p.exists():
                continue
            try:
                cfg = yaml.safe_load(p.read_text(encoding="utf-8")) or {}
            except Exception:
                continue
            provs = cfg.get("custom_providers")
            entries = []
            if isinstance(provs, dict):
                entries = list(provs.values())
            elif isinstance(provs, list):
                entries = provs
            model = ((cfg.get("model") or {}).get("default")) or "auto"
            for e in entries:
                if not isinstance(e, dict):
                    continue
                bu = str(e.get("base_url") or "")
                if bu.startswith("http://127.0.0.1") or bu.startswith("http://localhost"):
                    return bu, str(e.get("api_key") or ""), model
    except Exception:
        pass
    return "", "", ""


class Adjudicator:
    """Thin pipeline around the v4 API; all state lives in record metadata."""

    def __init__(self, client, cfg: Optional[Dict[str, Any]] = None) -> None:
        """client: an object with search/add/patch methods matching HyatlasClient."""
        self.client = client
        c = cfg or {}
        self.base_url = c.get("llm_base_url", _env("HYADJ_LLM_BASE_URL"))
        self.api_key = c.get("llm_api_key", _env("HYADJ_LLM_API_KEY"))
        self.model = c.get("llm_model") or _env("HYADJ_LLM_MODEL") or ""
        if not self.base_url:
            disc_url, disc_key, disc_model = _discover_llm()
            self.base_url = disc_url
            self.api_key = self.api_key or disc_key
            self.model = self.model or disc_model
        self.model = self.model or "auto"
        self.enabled = c.get("enabled", _env("HYADJ_ENABLED", "1")) != "0"
        self.prefilter = float(c.get("prefilter") or _env("HYADJ_PREFILTER") or 0.80)
        self.limit = int(c.get("neighbor_limit") or _env("HYADJ_NEIGHBOR_LIMIT") or 8)
        self.timeout = float(c.get("timeout") or _env("HYADJ_TIMEOUT") or 30)
        layers = c.get("layers") or _env("HYADJ_LAYERS") or "l1_profile,l3_fact"
        self.layers = {x.strip() for x in layers.split(",") if x.strip()}

    # -- availability -----------------------------------------------------
    def active(self) -> bool:
        return bool(self.enabled and self.base_url)

    # -- neighbours -------------------------------------------------------
    def neighbors(self, text: str, user_id: str, agent_id: str) -> List[Dict[str, Any]]:
        """§4.1 step 1: /search with include_expired so victims are visible
        to re-adjudication too."""
        try:
            resp = self.client.search(
                query=text, user_id=user_id, agent_id=agent_id,
                limit=self.limit, include_expired=True,
            )
        except Exception as exc:  # unreachable server -> plain add
            logger.debug("adjudicate: neighbor search failed: %s", exc)
            return []
        hits = _flatten_hits(resp)
        out = []
        for h in hits:
            if not isinstance(h, dict):
                continue
            layer = (h.get("layer") or h.get("Layer") or "").lower()
            if layer and layer not in self.layers:
                continue
            out.append(h)
        return out

    # -- LLM verdict ------------------------------------------------------
    def judge(self, text: str, neighbors: List[Dict[str, Any]]) -> Tuple[str, List[str], str]:
        """Returns (verdict, victim_ids, reason). Never raises."""
        cand = [n for n in neighbors if float(n.get("score") or 0) >= self.prefilter]
        if not cand:
            return "add", [], "no candidate above prefilter"
        lines = []
        for n in cand[:self.limit]:
            nid = n.get("memory_id") or n.get("id") or ""
            content = re.sub(r"\s+", " ", (n.get("content") or ""))[:400]
            t = _fmt_ts(n.get("gmt_created"))
            expired = "已失效" if (n.get("metadata") or {}).get("superseded_by") else "有效"
            lines.append(f"- id={nid} | {t} | {expired} | {content}")
        user_payload = "【新记忆】\n" + text[:800] + "\n\n【已有记忆】\n" + "\n".join(lines)
        body = {
            "model": self.model,
            "messages": [
                {"role": "system", "content": _JUDGE_SYSTEM},
                {"role": "user", "content": user_payload},
            ],
            "temperature": 0,
        }
        req = urllib.request.Request(
            self.base_url.rstrip("/") + "/chat/completions",
            data=json.dumps(body).encode("utf-8"),
            headers={
                "Content-Type": "application/json",
                "Authorization": "Bearer " + self.api_key,
            },
            method="POST",
        )
        valid_ids = {str(n.get("memory_id") or n.get("id") or "") for n in cand}
        try:
            with urllib.request.urlopen(req, timeout=self.timeout) as r:
                resp = json.loads(r.read().decode("utf-8"))
            content = (resp.get("choices") or [{}])[0].get("message", {}).get("content", "")
            m = re.search(r"\{.*\}", content, re.S)
            if not m:
                return "add", [], "llm output unparsable"
            v = json.loads(m.group(0))
            verdict = v.get("verdict", "add")
            if verdict not in _VERDICTS:
                return "add", [], "unknown verdict from llm"
            victims = [x for x in (v.get("victim_ids") or []) if str(x) in valid_ids]
            reason = str(v.get("reason") or "")[:200]
            if verdict == "supersede" and not victims:
                return "add", [], "supersede without valid victims -> add"
            return verdict, victims, reason
        except Exception as exc:
            logger.debug("adjudicate: llm judge failed: %s", exc)
            return "add", [], f"judge degraded: {exc}"

    # -- apply ------------------------------------------------------------
    def apply_supersede(self, new_id: str, victims: List[Dict[str, Any]], reason: str,
                        user_id: str, agent_id: str) -> Dict[str, Any]:
        """Patch victims + back-fill new record's supersede_ids. Victims are
        never deleted; pin=manual victims are skipped (pin 优先人工 拍板)."""
        now = _cutoff()
        done, skipped = [], []
        for n in victims:
            nid = str(n.get("memory_id") or n.get("id") or "")
            meta = n.get("metadata") or {}
            if not nid:
                continue
            if str(meta.get("pin") or "") == "manual":
                skipped.append(nid)
                continue
            try:
                self.client.patch(
                    nid,
                    set_meta={
                        "superseded_by": new_id,
                        "supersede_reason": reason,
                        "valid_until": now,
                    },
                )
                done.append(nid)
            except Exception as exc:
                logger.warning("adjudicate: patch victim %s failed: %s", nid, exc)
        if done and new_id:
            try:
                self.client.patch(
                    new_id,
                    set_meta={"supersede_ids": json.dumps(done)},
                )
            except Exception as exc:
                logger.warning("adjudicate: backfill supersede_ids failed: %s", exc)
        return {"superseded": done, "skipped_pin_manual": skipped}

    # -- explicit pair supersede (backfill of in-store contradictions) ----
    def supersede_pair(self, winner_id: str, victims: List[Dict[str, Any]],
                       reason: str, user_id: str, agent_id: str,
                       dry_run: bool = False) -> Dict[str, Any]:
        """Backfill primitive: both records already exist; the newer/winning
        fact supersedes the losers WITHOUT creating a duplicate copy.
        Patches only (valid_until/superseded_by/supersede_ids) — zero new
        writes, zero deletions (AC: 回填零删除)."""
        if dry_run:
            return {"dry_run": True, "winner": winner_id,
                    "victims": [str(v.get("memory_id")) for v in victims]}
        by_id = {str(n.get("memory_id") or ""): n for n in victims}
        objs = list(by_id.values())
        res = self.apply_supersede(winner_id, objs, reason, user_id, agent_id)
        # winner carries the forward chain too (official memory_ids style)
        if res["superseded"] and winner_id:
            try:
                self.client.patch(winner_id,
                                  set_meta={"supersede_ids": json.dumps(res["superseded"])})
            except Exception as exc:
                logger.warning("adjudicate: winner supersede_ids patch failed: %s", exc)
        res["winner"] = winner_id
        return res

    # -- the one-call wrapper ---------------------------------------------
    def wrap_add(self, text: str, user_id: str, agent_id: str,
                 metadata: Optional[Dict[str, str]] = None) -> Dict[str, Any]:
        """Adjudicated add. Returns {memory_id, verdict, victim_ids, degraded}.

        Plain client.add() stays untouched — this is the wrapper the plugin
        hooks and CLI call instead.
        """
        meta = dict(metadata or {})
        if not self.active():
            resp = self._plain_add(text, user_id, agent_id, meta)
            return {"memory_id": resp.get("memory_id", ""), "verdict": "add",
                    "victim_ids": [], "adjudication": "off"}
        nbrs = self.neighbors(text, user_id, agent_id)
        verdict, victims, reason = self.judge(text, nbrs)

        if verdict == "reject_dup":
            dup_id = victims[0] if victims else ""
            return {"memory_id": dup_id, "verdict": "reject_dup",
                    "victim_ids": victims, "reason": reason,
                    "note": "duplicate of existing memory, not stored"}

        meta.setdefault("source_kind", "agent_write")
        if verdict == "supersede":
            meta["supersede_ids_pending"] = json.dumps(victims)
            meta["supersede_reason"] = reason
        resp = self._plain_add(text, user_id, agent_id, meta)
        new_id = str(resp.get("memory_id", ""))
        out = {"memory_id": new_id, "verdict": verdict, "victim_ids": victims,
               "reason": reason}
        if verdict == "supersede":
            by_id = {str(n.get("memory_id") or n.get("id") or ""): n for n in nbrs}
            vobjs = [by_id[i] for i in victims if i in by_id]
            out["apply"] = self.apply_supersede(new_id, vobjs, reason, user_id, agent_id)
        return out

    def _plain_add(self, text, user_id, agent_id, meta) -> Dict[str, Any]:
        meta = dict(meta or {})
        session_id = meta.pop("session_id", "") or ""
        return self.client.add(text=text, user_id=user_id, agent_id=agent_id,
                               session_id=session_id, metadata=meta or None)


# ---------------------------------------------------------------------------
# Backfill runbook (§4.3): adjudicate existing records through the SAME
# pipeline. Only L1/L3 in scope; newest fact wins (pipeline is
# newest-reviews-older by design — feed facts oldest-first).
# ---------------------------------------------------------------------------

_PROBE_GROUP_QUERIES = {
    "minibill": ["minibill 退役 部署 账本", "minibill"],
    "charger":  ["充电器 功率 判据 笔记本 电动车", "充电器"],
    "neohorse": ["NeoHorse 模型 路由 兜底", "NeoHorse"],
}


def collect_backfill(client, user_id: str, agent_id: str) -> List[Dict[str, Any]]:
    """Enumerate L1+L3 records (paginated list) sorted by memory_id prefix
    (birth time — gmt_created is unreliable per spec §4)."""
    rows: List[Dict[str, Any]] = []
    for layer in ("l1_profile", "l3_fact"):
        offset, total = 0, None
        while True:
            d = client.list_memories(user_id=user_id, agent_id=agent_id,
                                     layer=layer, limit=200, offset=offset)
            mems = d.get("memories") or []
            if total is None:
                total = int(d.get("total") or len(mems))
            rows.extend(mems)
            offset += 200
            if offset >= total or not mems:
                break
    # dedupe by id (views may repeat records across layers)
    seen, uniq = set(), []
    for r in rows:
        rid = str(r.get("memory_id") or r.get("id") or "")
        if rid and rid not in seen:
            seen.add(rid)
            uniq.append(r)
    uniq.sort(key=lambda r: str(r.get("memory_id") or ""))
    return uniq


def probes(client, user_id: str, agent_id: str) -> Dict[str, Any]:
    """AC#1 check: recall probes for the 3 contradiction groups."""
    out = {}
    for name, queries in _PROBE_GROUP_QUERIES.items():
        q = queries[0]
        resp = client.search(query=q, user_id=user_id, agent_id=agent_id,
                             limit=3, include_expired=False)
        hits = _flatten_hits(resp)[:3]
        out[name] = {
            "query": q,
            "top3": [
                {"id": str(h.get("memory_id") or h.get("id") or ""),
                 "score": h.get("score"),
                 "content": re.sub(r"\s+", " ", h.get("content") or "")[:160]}
                for h in hits
            ],
        }
    return out


def _match_group(rec: Dict[str, Any], group: str) -> bool:
    kw = {
        "minibill": ("minibill",),
        "charger": ("充电器",),
        "neohorse": ("neohorse",),
    }[group]
    content = re.sub(r"\s+", "", (rec.get("content") or "").lower())
    return any(k in content for k in kw)


def backfill(adjud: Adjudicator, client, user_id: str, agent_id: str,
             group: Optional[str] = None, dry_run: bool = True,
             limit: int = 100) -> Dict[str, Any]:
    """One-shot §4.3 runner. Feed in-scope records oldest-first through
    wrap_add; the adjudicator decides supersede per record."""
    recs = collect_backfill(client, user_id, agent_id)
    if group:
        recs = [r for r in recs if _match_group(r, group)]
    recs = [r for r in recs if not (r.get("metadata") or {}).get("superseded_by")]
    recs = recs[:limit]
    report = {"scanned": len(recs), "verdicts": {}, "actions": [], "dry_run": dry_run}
    for r in recs:
        text = re.sub(r"\s+", " ", (r.get("content") or "")).strip()
        if len(text) < 10:
            continue
        if dry_run:
            nbrs = adjud.neighbors(text, user_id, agent_id)
            verdict, victims, reason = adjud.judge(text, nbrs)
            report["actions"].append({"id": r.get("memory_id"), "verdict": verdict,
                                      "victims": victims, "reason": reason})
        else:
            res = adjud.wrap_add(text, user_id, agent_id,
                                 metadata={"source_kind": "backfill",
                                           "backfill_of": str(r.get("memory_id") or "")})
            report["actions"].append(res)
        verdict = (report["actions"][-1] or {}).get("verdict", "?")
        report["verdicts"][verdict] = report["verdicts"].get(verdict, 0) + 1
    return report
