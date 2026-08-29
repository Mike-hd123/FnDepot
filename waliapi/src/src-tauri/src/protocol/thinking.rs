//! Thinking / reasoning effort mapping helpers shared by the codec and the
//! legacy conversion paths.
//!
//! Both directions are fail-open: we never consult a model-capability table
//! (models change faster than we can maintain one) and never reject thinking
//! intent.  We map intent between the two wire formats and let the upstream
//! provider adjudicate.  Thresholds follow CLIProxyAPI's `ConvertBudgetToLevel`
//! and `MapToClaudeEffort`.

/// Map an Anthropic `budget_tokens` value to an OpenAI `reasoning_effort`
/// level, following CLIProxyAPI's `ConvertBudgetToLevel`.
///
/// Returns `None` for budgets outside the documented range (including `<-1`),
/// in which case the caller should omit `reasoning_effort` entirely rather
/// than invent a level.
pub fn budget_to_level(budget: i64) -> Option<&'static str> {
    match budget {
        -1 => Some("auto"),
        0 => Some("none"),
        1..=512 => Some("minimal"),
        513..=1024 => Some("low"),
        1025..=8192 => Some("medium"),
        8193..=24576 => Some("high"),
        24_577..=i64::MAX => Some("xhigh"),
        // -2 and below: CLIProxyAPI treats as invalid and omits the field.
        _ => None,
    }
}

/// Map an OpenAI `reasoning_effort` (or `output_config.effort`) value to a
/// Claude `output_config.effort`, following CLIProxyAPI's `MapToClaudeEffort`.
///
/// WaLiAPI deliberately has no model registry, so we cannot look up a model's
/// `supports_max` — `xhigh`/`max` conservatively collapse to `high` (matching
/// CPA's no-supportsMax default and 9router's `claude-adaptive`).  `low`/
/// `medium`/`high` pass through unchanged; `minimal` maps to `low`; `auto` and
/// unknown values collapse to `high`.
///
/// Returns a lowercase value ready for the Anthropic wire format.
pub fn map_effort_to_claude(effort: &str) -> &'static str {
    match effort.to_ascii_lowercase().as_str() {
        "minimal" => "low",
        // low / medium / high pass through unchanged (canonical OpenAI values).
        "low" => "low",
        "medium" => "medium",
        "high" => "high",
        // auto / xhigh / max / unknown all collapse to high.
        _ => "high",
    }
}
