//! LLM 用量双写：UsageDb（kind=llm）与会话账单累加。

use ::session::{BillingDelta, SessionStore};
use ::usage::{estimate_usage_cost, CostStatus, NewUsageEvent, UsageDb, UsageTokens};
use providers::streaming::Usage;

/// 从一次 LLM 调用构造用量事件与会话账单增量。
pub(crate) fn build_llm_usage_event(
    agent_id: &str,
    session_id: Option<&str>,
    turn_id: Option<&str>,
    model: &str,
    usage: &Usage,
    provider: &str,
    base_url: &str,
    api_key: &str,
) -> (NewUsageEvent, BillingDelta) {
    let tokens = UsageTokens {
        input_tokens: usage.input_tokens,
        output_tokens: usage.output_tokens,
        cache_read_tokens: usage.cache_read_tokens,
        cache_write_tokens: usage.cache_write_tokens,
        request_count: if usage.request_count == 0 {
            1
        } else {
            usage.request_count
        },
    };

    let provider_opt = (!provider.is_empty()).then_some(provider);
    let base_url_opt = (!base_url.is_empty()).then_some(base_url);
    let api_key_opt = (!api_key.is_empty()).then_some(api_key);

    let cost_result = estimate_usage_cost(
        model,
        &tokens,
        provider_opt,
        base_url_opt,
        api_key_opt,
    );

    let status_str = match cost_result.status {
        CostStatus::Estimated => "estimated",
        CostStatus::Included => "included",
        CostStatus::Unknown => "unknown",
    };

    let cost_usd = match cost_result.status {
        CostStatus::Unknown => 0.0,
        _ => cost_result.amount_usd.unwrap_or(0.0),
    };

    let billing_provider = provider_opt.map(str::to_string);
    let billing_base_url = base_url_opt.map(str::to_string);
    let billing_mode = Some(cost_result.source.clone());
    let api_call_count = i64::from(if usage.request_count == 0 {
        1
    } else {
        usage.request_count
    });

    let event = NewUsageEvent {
        ts: chrono::Utc::now().to_rfc3339(),
        kind: "llm".into(),
        name: model.to_string(),
        agent_id: agent_id.to_string(),
        session_id: session_id.map(str::to_string),
        turn_id: turn_id.map(str::to_string),
        input_tokens: i64::from(usage.input_tokens),
        output_tokens: i64::from(usage.output_tokens),
        cache_read_tokens: i64::from(usage.cache_read_tokens),
        cache_write_tokens: i64::from(usage.cache_write_tokens),
        reasoning_tokens: i64::from(usage.reasoning_tokens),
        total_tokens: i64::from(usage.total_tokens()),
        cost_usd,
        cost_status: Some(status_str.to_string()),
        cost_source: Some(cost_result.source.clone()),
        pricing_version: cost_result.pricing_version.clone(),
        billing_provider: billing_provider.clone(),
        billing_base_url: billing_base_url.clone(),
        billing_mode: billing_mode.clone(),
        meta_json: None,
    };

    let delta = BillingDelta {
        input_tokens: i64::from(usage.input_tokens),
        output_tokens: i64::from(usage.output_tokens),
        cache_read_tokens: i64::from(usage.cache_read_tokens),
        cache_write_tokens: i64::from(usage.cache_write_tokens),
        reasoning_tokens: i64::from(usage.reasoning_tokens),
        estimated_cost_usd: cost_usd,
        api_call_count,
        billing_provider,
        billing_base_url,
        billing_mode,
        cost_status: Some(status_str.to_string()),
        cost_source: Some(cost_result.source),
        pricing_version: cost_result.pricing_version,
        model: Some(model.to_string()),
    };

    (event, delta)
}

/// 写入 UsageDb 并累加 SessionStore 账单；失败仅 warn。
///
/// `sessions` 应与写入消息的同一 [`SessionStore`]（通常来自 `AgentLoop`），
/// 避免再按 `default_memory_dir` 另开库导致自定义 `memory_dir` 下账单分叉。
pub(crate) fn apply_llm_usage_dual_write(
    agent_id: &str,
    session_id: Option<&str>,
    turn_id: Option<&str>,
    model: &str,
    usage: &Usage,
    provider: &str,
    base_url: &str,
    api_key: &str,
    meta_json: Option<String>,
    sessions: Option<&SessionStore>,
) {
    if usage.is_empty() {
        return;
    }
    let (mut event, delta) = build_llm_usage_event(
        agent_id,
        session_id,
        turn_id,
        model,
        usage,
        provider,
        base_url,
        api_key,
    );
    if let Some(meta) = meta_json {
        event.meta_json = Some(meta);
    }
    UsageDb::try_record(event);
    let Some(sid) = session_id else {
        return;
    };
    let Some(store) = sessions else {
        tracing::warn!(
            session_id = sid,
            "skip session billing: no SessionStore provided"
        );
        return;
    };
    if let Err(e) = store.update_session_billing(sid, delta) {
        tracing::warn!(session_id = sid, error = %e, "update_session_billing failed");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_llm_usage_event_gpt4o_mini_openai_estimated() {
        let usage = Usage {
            input_tokens: 1_000_000,
            output_tokens: 1_000_000,
            request_count: 1,
            ..Default::default()
        };
        let (event, delta) = build_llm_usage_event(
            "agent-1",
            Some("sess-1"),
            None,
            "gpt-4o-mini",
            &usage,
            "openai",
            "",
            "",
        );
        assert_eq!(event.kind, "llm");
        assert_eq!(event.name, "gpt-4o-mini");
        assert_eq!(event.agent_id, "agent-1");
        assert_eq!(event.session_id.as_deref(), Some("sess-1"));
        assert_eq!(event.turn_id, None);
        assert_eq!(event.input_tokens, 1_000_000);
        assert_eq!(event.output_tokens, 1_000_000);
        assert_eq!(event.cost_status.as_deref(), Some("estimated"));
        assert!(event.cost_usd > 0.0);
        assert_eq!(event.billing_provider.as_deref(), Some("openai"));
        assert_eq!(delta.cost_status.as_deref(), Some("estimated"));
        assert!(delta.estimated_cost_usd > 0.0);
        assert_eq!(delta.model.as_deref(), Some("gpt-4o-mini"));
    }

    #[test]
    fn build_llm_usage_event_propagates_turn_id() {
        let usage = Usage {
            input_tokens: 10,
            output_tokens: 5,
            request_count: 1,
            ..Default::default()
        };
        let (event, _) = build_llm_usage_event(
            "agent-1",
            Some("sess-1"),
            Some("t1"),
            "gpt-4o-mini",
            &usage,
            "openai",
            "",
            "",
        );
        assert_eq!(event.turn_id.as_deref(), Some("t1"));
    }
}
