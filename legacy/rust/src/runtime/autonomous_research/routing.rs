pub(crate) fn schedule_autonomous_research_provider_backoff_tick(
    resolved: &ResolvedProject,
    job: &mut AutonomousResearchJobState,
    tick_index: usize,
    tick_summary: Option<AutonomousResearchTickSummary>,
    fault: AutonomousResearchProviderFaultState,
    source: &str,
    stop_reason: &str,
    output_json: bool,
) -> Result<AutonomousResearchJobTickResult, CommandFailureOutcome> {
    let wait_until = fault
        .next_retry_at
        .as_deref()
        .and_then(|value| value.parse::<u128>().ok())
        .map(|retry_at| retry_at.saturating_sub(timestamp_millis()))
        .unwrap_or_else(|| fault.backoff_ms as u128);
    job.last_error = Some(fault.message.clone());
    job.active_provider_fault = Some(fault.clone());
    job.provider_faults.push(fault);
    let provider_obligation_id = job
        .active_provider_fault
        .clone()
        .map(|fault| record_autonomous_research_provider_fault_obligation(resolved, job, &fault));
    job.status = "running".to_string();
    job.phase = "provider_backoff".to_string();
    job.stop_reason = Some(stop_reason.to_string());
    job.updated_at = timestamp_string();
    write_autonomous_research_job_state(resolved, job).map_err(|write_err| {
        internal_failure("research jobs tick", write_err).with_output_json(output_json)
    })?;
    append_autonomous_research_job_event(
        resolved,
        job,
        "provider_backoff_scheduled",
        json!({
            "schema_version": "autonomous_research_job_event.v1",
            "source": source,
            "wait_ms": wait_until,
            "fault": job.active_provider_fault,
            "provider_fault_obligation_id": provider_obligation_id
        }),
    )
    .map_err(|err| internal_failure("research jobs tick", err).with_output_json(output_json))?;

    Ok(AutonomousResearchJobTickResult {
        schema_version: "autonomous_research_job_tick_result.v1".to_string(),
        status: "ticked".to_string(),
        project_id: resolved.project_id.clone(),
        job_id: job.job_id.clone(),
        tick_index,
        job_status: job.status.clone(),
        phase: job.phase.clone(),
        should_continue: !autonomous_research_job_terminal(&job.status),
        review_passed: false,
        tick_summary,
        review: None,
        job: job.clone(),
    })
}

pub(crate) fn autonomous_research_next_sleep_ms(
    resolved: &ResolvedProject,
    job: &AutonomousResearchJobState,
) -> u64 {
    if let Some(fault) = job.active_provider_fault.as_ref() {
        if fault.operator_gate_required
            && autonomous_research_operator_gate_recovery_route(resolved, job, fault).is_some()
        {
            return job.interval_ms;
        }
        if autonomous_research_provider_fault_can_use_recovery_route(fault)
            && autonomous_research_available_provider_recovery_route(resolved, job).is_some()
        {
            return job.interval_ms;
        }
        if let Some(next_retry_at) = fault.next_retry_at.as_deref() {
            let now = timestamp_millis();
            if let Ok(next_retry_at_ms) = next_retry_at.parse::<u128>() {
                if next_retry_at_ms > now {
                    let active_retry_delay_ms = (next_retry_at_ms - now) as u64;
                    if let Some(recovery_retry_at_ms) =
                        autonomous_research_earliest_recovery_route_retry_at_ms(resolved, job)
                    {
                        if recovery_retry_at_ms > now {
                            return job
                                .interval_ms
                                .max((recovery_retry_at_ms - now) as u64)
                                .min(active_retry_delay_ms);
                        }
                    }
                    return job.interval_ms.max(active_retry_delay_ms);
                }
            }
        }
        if fault.retryable {
            return job.interval_ms.max(fault.backoff_ms);
        }
        return job.interval_ms.max(AUTONOMOUS_RESEARCH_PROVIDER_BACKOFF_MS);
    }
    if job
        .last_error
        .as_deref()
        .map(autonomous_research_error_needs_provider_backoff)
        .unwrap_or(false)
    {
        return job.interval_ms.max(AUTONOMOUS_RESEARCH_PROVIDER_BACKOFF_MS);
    }
    job.interval_ms
}

pub(crate) fn autonomous_research_provider_fault_can_use_recovery_route(
    fault: &AutonomousResearchProviderFaultState,
) -> bool {
    fault.retryable
}

pub(crate) fn autonomous_research_earliest_recovery_route_retry_at_ms(
    resolved: &ResolvedProject,
    job: &AutonomousResearchJobState,
) -> Option<u128> {
    let effective = effective_config(resolved).ok()?.effective;
    let active_provider_id = job
        .active_provider_fault
        .as_ref()
        .and_then(|fault| fault.provider_id.as_deref());
    let mut candidate_provider_ids = Vec::new();
    if let Some(provider_id) = effective.default_provider.as_deref() {
        merge_unique_strings(&mut candidate_provider_ids, vec![provider_id.to_string()]);
    }
    merge_unique_strings(
        &mut candidate_provider_ids,
        effective.provider_failover.clone(),
    );
    let now = timestamp_millis();
    candidate_provider_ids
        .into_iter()
        .filter(|provider_id| Some(provider_id.as_str()) != active_provider_id)
        .filter_map(|provider_id| {
            autonomous_research_provider_recovery_retry_at_ms(
                &provider_id,
                job,
                &effective.provider_profiles,
            )
        })
        .filter(|retry_at| *retry_at > now)
        .min()
}

pub(crate) fn autonomous_research_provider_recovery_retry_at_ms(
    provider_id: &str,
    job: &AutonomousResearchJobState,
    profiles: &BTreeMap<String, crate::config::ProviderProfileConfig>,
) -> Option<u128> {
    let mut retry_at_ms: Option<u128> = None;
    for fault in &job.provider_faults {
        if fault.provider_id.as_deref() != Some(provider_id) {
            continue;
        }
        if !autonomous_research_historical_provider_fault_excludes_provider_for_recovery(
            fault,
            Some(profiles),
        ) {
            continue;
        }
        if fault.operator_gate_required
            || !fault.retryable
            || matches!(
                fault.category.as_str(),
                "auth_missing" | "auth_invalid" | "live_provider_required" | "invalid_request"
            )
        {
            return None;
        }
        let next_retry_at = autonomous_research_provider_fault_effective_next_retry_at_ms(fault)?;
        retry_at_ms = Some(retry_at_ms.unwrap_or(next_retry_at).max(next_retry_at));
    }
    retry_at_ms
}

pub(crate) fn autonomous_research_provider_fault_excludes_provider_for_recovery(
    fault: &AutonomousResearchProviderFaultState,
) -> bool {
    if fault.operator_gate_required || !fault.retryable {
        return true;
    }
    if matches!(
        fault.category.as_str(),
        "auth_missing" | "live_provider_required" | "invalid_request"
    ) {
        return true;
    }
    if fault.category == "quota_exhausted" {
        return fault.next_retry_at.is_none()
            || !autonomous_research_provider_fault_is_ready(fault);
    }
    !autonomous_research_provider_fault_is_ready(fault)
}

pub(crate) fn autonomous_research_historical_provider_fault_excludes_provider_for_recovery(
    fault: &AutonomousResearchProviderFaultState,
    profiles: Option<&BTreeMap<String, crate::config::ProviderProfileConfig>>,
) -> bool {
    if fault.operator_gate_required || !fault.retryable {
        if profiles
            .map(|profiles| {
                autonomous_research_provider_operator_gate_can_release_with_profiles(
                    fault, profiles,
                )
            })
            .unwrap_or(false)
        {
            return false;
        }
        return matches!(
            fault.category.as_str(),
            "auth_missing" | "auth_invalid" | "live_provider_required" | "invalid_request"
        );
    }
    if matches!(
        fault.category.as_str(),
        "auth_missing" | "live_provider_required" | "invalid_request"
    ) {
        return true;
    }
    if fault.category == "quota_exhausted" {
        return fault.next_retry_at.is_none()
            || !autonomous_research_provider_fault_is_ready(fault);
    }
    !autonomous_research_provider_fault_is_ready(fault)
}

pub(crate) fn autonomous_research_provider_fault_is_misclassified_worker_loop_exhaustion(
    fault: &AutonomousResearchProviderFaultState,
) -> bool {
    let lowered = fault.message.to_ascii_lowercase();
    fault.source == "agent_team_worker"
        && lowered.contains("agent loop")
        && (lowered.contains("max iterations") || lowered.contains("exhausted budget"))
}

pub(crate) fn autonomous_research_error_needs_provider_backoff(message: &str) -> bool {
    let lowered = message.to_ascii_lowercase();
    lowered.contains("http 429")
        || lowered.contains("rate limit")
        || lowered.contains("crossed tpm")
        || lowered.contains("crossed rpm")
}

pub(crate) fn provider_failure_disposition_name(disposition: ProviderFailureDisposition) -> &'static str {
    match disposition {
        ProviderFailureDisposition::RetryWithBackoff => "retry_with_backoff",
        ProviderFailureDisposition::OperatorGate => "operator_gate",
        ProviderFailureDisposition::Cancel => "cancel",
    }
}

pub(crate) fn autonomous_research_provider_rate_limit_retry_at_cap_ms(
    created_at_ms: u128,
) -> u128 {
    created_at_ms.saturating_add(u128::from(
        crate::providers::PROVIDER_FAILURE_BACKOFF_CAP_MS,
    ))
}

pub(crate) const AUTONOMOUS_RESEARCH_PROVIDER_RATE_LIMIT_RESET_SAFETY_MS: u128 = 5_000;
pub(crate) const AUTONOMOUS_RESEARCH_PROVIDER_RATE_LIMIT_AMBIGUOUS_RESET_PROBE_MS: u64 = 65_000;

pub(crate) fn autonomous_research_provider_rate_limit_ambiguous_reset_probe_ms(
    consecutive_failures: usize,
) -> u64 {
    let failures = consecutive_failures.max(1);
    let shift = failures.saturating_sub(1).min(5) as u32;
    AUTONOMOUS_RESEARCH_PROVIDER_RATE_LIMIT_AMBIGUOUS_RESET_PROBE_MS
        .saturating_mul(1u64 << shift)
        .min(crate::providers::PROVIDER_FAILURE_BACKOFF_CAP_MS)
}

pub(crate) fn autonomous_research_provider_rate_limit_reset_retry_at_ms(message: &str) -> Option<u128> {
    let lowered = message.to_ascii_lowercase();
    let marker = "limit resets at:";
    let start = lowered.find(marker)? + marker.len();
    let rest = message.get(start..)?.trim_start();
    let rest_lowered = rest.to_ascii_lowercase();
    let end = rest_lowered.find("utc")?;
    let value = rest.get(..end)?.trim();
    let (date, time) = value.split_once(' ')?;
    let mut date_parts = date.split('-');
    let year = date_parts.next()?.parse::<i64>().ok()?;
    let month = date_parts.next()?.parse::<u32>().ok()?;
    let day = date_parts.next()?.parse::<u32>().ok()?;
    let mut time_parts = time.split(':');
    let hour = time_parts.next()?.parse::<u32>().ok()?;
    let minute = time_parts.next()?.parse::<u32>().ok()?;
    let second = time_parts.next()?.parse::<u32>().ok()?;
    if !(1..=12).contains(&month)
        || !(1..=31).contains(&day)
        || hour > 23
        || minute > 59
        || second > 59
    {
        return None;
    }
    let days = days_from_civil_utc(year, month, day)?;
    let seconds = days
        .checked_mul(86_400)?
        .checked_add(i64::from(hour) * 3_600)?
        .checked_add(i64::from(minute) * 60)?
        .checked_add(i64::from(second))?;
    if seconds < 0 {
        return None;
    }
    Some(
        (seconds as u128)
            .saturating_mul(1_000)
            .saturating_add(AUTONOMOUS_RESEARCH_PROVIDER_RATE_LIMIT_RESET_SAFETY_MS),
    )
}

pub(crate) fn autonomous_research_provider_trusted_rate_limit_reset_retry_at_ms(
    message: &str,
    created_at_ms: u128,
) -> Option<u128> {
    let reset_retry_at = autonomous_research_provider_rate_limit_reset_retry_at_ms(message)?;
    if reset_retry_at <= created_at_ms {
        return None;
    }
    let cap_retry_at = autonomous_research_provider_rate_limit_retry_at_cap_ms(created_at_ms);
    if reset_retry_at <= cap_retry_at {
        return Some(reset_retry_at);
    }
    None
}

pub(crate) fn days_from_civil_utc(year: i64, month: u32, day: u32) -> Option<i64> {
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    let mut y = year;
    let m = i64::from(month);
    let d = i64::from(day);
    y -= if m <= 2 { 1 } else { 0 };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = m + if m > 2 { -3 } else { 9 };
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(era * 146_097 + doe - 719_468)
}

pub(crate) fn autonomous_research_provider_fault_schedule_for_message(
    policy: &crate::providers::ProviderFailurePolicy,
    consecutive_failures: usize,
    message: &str,
    created_at_ms: u128,
) -> (u64, Option<String>) {
    if !policy.retryable {
        return (0, None);
    }
    let backoff_ms = provider_failure_backoff_ms(policy, consecutive_failures);
    let base_retry_at = created_at_ms.saturating_add(backoff_ms as u128);
    let retry_at = if policy.category == "rate_limit" {
        if let Some(reset_retry_at) =
            autonomous_research_provider_trusted_rate_limit_reset_retry_at_ms(message, created_at_ms)
        {
            reset_retry_at
        } else if autonomous_research_provider_rate_limit_reset_retry_at_ms(message).is_some() {
            created_at_ms.saturating_add(u128::from(
                autonomous_research_provider_rate_limit_ambiguous_reset_probe_ms(
                    consecutive_failures,
                ),
            ))
        } else {
            base_retry_at
        }
    } else {
        base_retry_at
    };
    let scheduled_backoff_ms = retry_at
        .saturating_sub(created_at_ms)
        .min(u128::from(u64::MAX)) as u64;
    (scheduled_backoff_ms, Some(retry_at.to_string()))
}

pub(crate) fn autonomous_research_provider_fault_is_ready(
    fault: &AutonomousResearchProviderFaultState,
) -> bool {
    let Some(next_retry_at_ms) =
        autonomous_research_provider_fault_effective_next_retry_at_ms(fault)
    else {
        return !fault.operator_gate_required;
    };
    timestamp_millis() >= next_retry_at_ms
}

pub(crate) fn autonomous_research_provider_fault_effective_next_retry_at_ms(
    fault: &AutonomousResearchProviderFaultState,
) -> Option<u128> {
    let stored_retry_at = fault
        .next_retry_at
        .as_deref()
        .and_then(|value| value.parse::<u128>().ok());
    if fault.category != "rate_limit" {
        return stored_retry_at;
    }
    let created_at_ms = fault
        .created_at
        .parse::<u128>()
        .unwrap_or_else(|_| timestamp_millis());
    if let Some(reset_retry_at) =
        autonomous_research_provider_trusted_rate_limit_reset_retry_at_ms(
            &fault.message,
            created_at_ms,
        )
    {
        return Some(reset_retry_at);
    }
    if autonomous_research_provider_rate_limit_reset_retry_at_ms(&fault.message).is_some() {
        return Some(created_at_ms.saturating_add(u128::from(
            autonomous_research_provider_rate_limit_ambiguous_reset_probe_ms(
                fault.consecutive_failures,
            ),
        )));
    }
    stored_retry_at
}

pub(crate) fn autonomous_research_provider_operator_gate_can_release_with_profiles(
    fault: &AutonomousResearchProviderFaultState,
    profiles: &BTreeMap<String, crate::config::ProviderProfileConfig>,
) -> bool {
    if !matches!(
        fault.category.as_str(),
        "auth_missing" | "live_provider_required"
    ) {
        return false;
    }
    let Some(provider_id) = fault.provider_id.as_deref() else {
        return false;
    };
    let trace = resolve_provider_trace_with_profiles(
        Some(provider_id),
        None,
        Some("active_provider_fault"),
        None,
        profiles,
    );
    trace
        .map(|trace| trace.auth_status == "configured")
        .unwrap_or(false)
}

pub(crate) fn autonomous_research_provider_operator_gate_can_release(
    resolved: &ResolvedProject,
    fault: &AutonomousResearchProviderFaultState,
) -> bool {
    let Ok(config) = effective_config(resolved) else {
        return false;
    };
    autonomous_research_provider_operator_gate_can_release_with_profiles(
        fault,
        &config.effective.provider_profiles,
    )
}

pub(crate) fn refresh_autonomous_research_active_provider_fault(
    job: &mut AutonomousResearchJobState,
) -> Option<(
    AutonomousResearchProviderFaultState,
    AutonomousResearchProviderFaultState,
)> {
    let active_fault = job.active_provider_fault.clone()?;
    let error = provider_execution_error_from_message(&active_fault.message);
    let policy = classify_provider_execution_error(&error);
    let created_at_ms = active_fault
        .created_at
        .parse::<u128>()
        .unwrap_or_else(|_| timestamp_millis());
    let schedule_origin_ms = if policy.retryable && active_fault.next_retry_at.is_none() {
        timestamp_millis()
    } else {
        created_at_ms
    };
    let (backoff_ms, scheduled_next_retry_at) =
        autonomous_research_provider_fault_schedule_for_message(
            &policy,
            active_fault.consecutive_failures,
            &active_fault.message,
            schedule_origin_ms,
        );
    if active_fault.category == policy.category
        && active_fault.disposition == provider_failure_disposition_name(policy.disposition)
        && active_fault.retryable == policy.retryable
        && active_fault.operator_gate_required == policy.operator_gate_required
        && active_fault.backoff_ms == backoff_ms
        && if policy.retryable {
            active_fault.next_retry_at == scheduled_next_retry_at
        } else {
            active_fault.next_retry_at.is_none()
        }
    {
        return None;
    }

    let now = timestamp_string();
    let reset_timestamp_present = policy.category == "rate_limit"
        && autonomous_research_provider_rate_limit_reset_retry_at_ms(&active_fault.message).is_some();
    let next_retry_at = if policy.retryable && reset_timestamp_present {
        scheduled_next_retry_at
    } else if policy.retryable {
        match (
            active_fault
                .next_retry_at
                .as_deref()
                .and_then(|value| value.parse::<u128>().ok()),
            scheduled_next_retry_at
                .as_deref()
                .and_then(|value| value.parse::<u128>().ok()),
        ) {
            (Some(existing), Some(scheduled)) => Some(existing.max(scheduled).to_string()),
            (Some(existing), None) => Some(existing.to_string()),
            (None, Some(scheduled)) => Some(scheduled.to_string()),
            (None, None) => None,
        }
    } else {
        None
    };
    let refreshed_fault = AutonomousResearchProviderFaultState {
        schema_version: active_fault.schema_version.clone(),
        fault_id: active_fault.fault_id.clone(),
        source: active_fault.source.clone(),
        provider_id: active_fault.provider_id.clone(),
        model: active_fault.model.clone(),
        category: policy.category,
        disposition: provider_failure_disposition_name(policy.disposition).to_string(),
        retryable: policy.retryable,
        operator_gate_required: policy.operator_gate_required,
        message: active_fault.message.clone(),
        consecutive_failures: active_fault.consecutive_failures,
        backoff_ms,
        next_retry_at,
        created_at: active_fault.created_at.clone(),
        updated_at: now.clone(),
    };
    job.active_provider_fault = Some(refreshed_fault.clone());
    if let Some(history_fault) = job
        .provider_faults
        .iter_mut()
        .rev()
        .find(|fault| fault.fault_id == active_fault.fault_id)
    {
        *history_fault = refreshed_fault.clone();
    }
    job.updated_at = now;
    if job.stop_reason.as_deref() == Some("provider_operator_gate_required")
        && !refreshed_fault.operator_gate_required
    {
        job.stop_reason = Some("provider_retry_backoff_active".to_string());
    }
    Some((active_fault, refreshed_fault))
}

pub(crate) fn autonomous_research_provider_fault_count(
    job: &AutonomousResearchJobState,
    source: &str,
    category: &str,
    provider_id: Option<&str>,
    model: Option<&str>,
) -> usize {
    job.provider_faults
        .iter()
        .filter(|fault| {
            fault.source == source
                && fault.category == category
                && fault.provider_id.as_deref() == provider_id
                && fault.model.as_deref() == model
        })
        .count()
        + job
            .active_provider_fault
            .as_ref()
            .filter(|fault| {
                fault.source == source
                    && fault.category == category
                    && fault.provider_id.as_deref() == provider_id
                    && fault.model.as_deref() == model
            })
            .map(|_| 1)
            .unwrap_or(0)
}

pub(crate) fn autonomous_research_main_agent_round_budget() -> AgentLoopBudget {
    AgentLoopBudget::new(
        AUTONOMOUS_RESEARCH_AGENT_ROUND_MAX_ITERATIONS,
        AUTONOMOUS_RESEARCH_AGENT_ROUND_MAX_TOOL_CALLS,
    )
}

pub(crate) fn autonomous_research_build_provider_fault_state(
    job: &AutonomousResearchJobState,
    source: &str,
    provider_id: Option<String>,
    model: Option<String>,
    policy: &crate::providers::ProviderFailurePolicy,
    message: &str,
) -> AutonomousResearchProviderFaultState {
    let consecutive_failures = autonomous_research_provider_fault_count(
        job,
        source,
        &policy.category,
        provider_id.as_deref(),
        model.as_deref(),
    );
    let consecutive_failures = consecutive_failures.saturating_add(1);
    let now = timestamp_string();
    let now_ms = timestamp_millis();
    let (backoff_ms, next_retry_at) = autonomous_research_provider_fault_schedule_for_message(
        policy,
        consecutive_failures,
        message,
        now_ms,
    );
    AutonomousResearchProviderFaultState {
        schema_version: "autonomous_research_provider_fault_state.v1".to_string(),
        fault_id: format!("pf_{}_{}", source, now),
        source: source.to_string(),
        provider_id,
        model,
        category: policy.category.clone(),
        disposition: provider_failure_disposition_name(policy.disposition).to_string(),
        retryable: policy.retryable,
        operator_gate_required: policy.operator_gate_required,
        message: compact_single_line(message, 1_000),
        consecutive_failures,
        backoff_ms,
        next_retry_at,
        created_at: now.clone(),
        updated_at: now,
    }
}

pub(crate) fn autonomous_research_stage_task_semantic_review_provider_fault_from_failure(
    resolved: &ResolvedProject,
    job: &AutonomousResearchJobState,
    failure: &CommandFailureOutcome,
) -> Option<AutonomousResearchProviderFaultState> {
    if job.stage_task_semantic_review_mode != "provider" {
        return None;
    }
    if failure.envelope.error.code != "provider_execution_failed"
        && !autonomous_research_error_looks_like_provider_failure(&failure.envelope.error.message)
    {
        return None;
    }
    let message = &failure.envelope.error.message;
    let error = provider_execution_error_from_message(message);
    let policy = classify_provider_execution_error(&error);
    let route_selection = autonomous_research_select_provider_route_for_review_model(
        resolved,
        &job.review_model,
        Some(job),
    );
    let effective_config = crate::config::effective_config(resolved)
        .map(|config| config.effective)
        .ok();
    let trace = route_selection.as_ref().and_then(|selection| {
        effective_config.as_ref().and_then(|effective| {
            resolve_provider_trace_with_profiles(
                selection.provider.as_deref(),
                selection.model.as_deref(),
                selection.provider_source.as_deref(),
                selection.model_source.as_deref(),
                &effective.provider_profiles,
            )
            .ok()
        })
    });
    let provider_id = trace
        .as_ref()
        .map(|trace| trace.resolved_provider.clone())
        .or_else(|| {
            route_selection
                .as_ref()
                .and_then(|route| route.provider.clone())
        });
    let model = trace
        .as_ref()
        .map(|trace| trace.resolved_model.clone())
        .or_else(|| {
            route_selection
                .as_ref()
                .and_then(|route| route.model.clone())
        })
        .or_else(|| job.review_model.clone());
    Some(autonomous_research_build_provider_fault_state(
        job,
        "stage_task_semantic_review",
        provider_id,
        model,
        &policy,
        message,
    ))
}

pub(crate) fn autonomous_research_agent_team_worker_provider_fault_from_acceptance(
    job: &AutonomousResearchJobState,
    acceptance: &GoalTaskAcceptanceResult,
) -> Option<AutonomousResearchProviderFaultState> {
    if acceptance.status != "provider_failed" {
        return None;
    }
    let provider_failure = acceptance.provider_failure.as_ref()?;
    if provider_failure.failure_code != "provider_execution_failed" {
        return None;
    }
    let error = provider_execution_error_from_message(&provider_failure.message);
    let policy = classify_provider_execution_error(&error);
    Some(autonomous_research_build_provider_fault_state(
        job,
        provider_failure.source.as_str(),
        provider_failure.provider_id.clone(),
        provider_failure.model.clone(),
        &policy,
        &provider_failure.message,
    ))
}

pub(crate) fn autonomous_research_error_looks_like_provider_failure(message: &str) -> bool {
    let lowered = message.to_ascii_lowercase();
    lowered.contains("provider ")
        && (lowered.contains("http ")
            || lowered.contains("missing required credential")
            || lowered.contains("request failed")
            || lowered.contains("request timed out")
            || lowered.contains("timeout")
            || lowered.contains("rate limit")
            || lowered.contains("response parse failed")
            || lowered.contains("response did not contain assistant text")
            || lowered.contains("not configured for live execution")
            || lowered.contains("does not support prompt execution"))
}

pub(crate) fn provider_execution_error_from_message(message: &str) -> ProviderExecutionError {
    let lowered = message.to_ascii_lowercase();
    if autonomous_research_error_needs_provider_backoff(message) {
        return ProviderExecutionError::HttpStatus {
            status: 429,
            body: message.to_string(),
        };
    }
    if lowered.contains("missing required credential") {
        return ProviderExecutionError::AuthMissing {
            provider_id: extract_between(message, "provider ", " is missing")
                .unwrap_or_else(|| "unknown".to_string()),
            env_var: message
                .rsplit(':')
                .next()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .unwrap_or("UNKNOWN_PROVIDER_CREDENTIAL")
                .to_string(),
        };
    }
    if lowered.contains("does not support prompt execution") {
        return ProviderExecutionError::UnsupportedProvider(
            extract_between(message, "provider ", " does not")
                .unwrap_or_else(|| "unknown".to_string()),
        );
    }
    if let Some(status) = parse_provider_http_status(message) {
        return ProviderExecutionError::HttpStatus {
            status,
            body: message.to_string(),
        };
    }
    if lowered.contains("provider response did not contain assistant text") {
        return ProviderExecutionError::EmptyResponse;
    }
    if lowered.contains("provider response parse failed") {
        return ProviderExecutionError::Parse(message.to_string());
    }
    if lowered.contains("not configured for live execution") {
        return ProviderExecutionError::LiveProviderRequired {
            provider_id: extract_between(message, "provider ", " model")
                .unwrap_or_else(|| "unknown".to_string()),
            model: extract_between(message, " model ", " is not")
                .unwrap_or_else(|| "unknown".to_string()),
            hint: message.to_string(),
        };
    }
    ProviderExecutionError::Transport(message.to_string())
}

pub(crate) fn parse_provider_http_status(message: &str) -> Option<u16> {
    let lowered = message.to_ascii_lowercase();
    let marker = "http ";
    let index = lowered.find(marker)?;
    lowered[index + marker.len()..]
        .split(|character: char| !character.is_ascii_digit())
        .next()
        .and_then(|value| value.parse::<u16>().ok())
}

pub(crate) fn extract_between(value: &str, prefix: &str, suffix: &str) -> Option<String> {
    let start = value.find(prefix)? + prefix.len();
    let rest = &value[start..];
    let end = rest.find(suffix)?;
    Some(rest[..end].trim().to_string())
}

pub(crate) fn autonomous_research_main_agent_provider_fault_from_error(
    job: &AutonomousResearchJobState,
    provider_trace: &crate::providers::ProviderResolutionTrace,
    source: &str,
    error: &ProviderExecutionError,
) -> AutonomousResearchMainAgentRoundError {
    let policy = classify_provider_execution_error(error);
    AutonomousResearchMainAgentRoundError::ProviderFault(
        autonomous_research_build_provider_fault_state(
            job,
            source,
            Some(provider_trace.resolved_provider.clone()),
            Some(provider_trace.resolved_model.clone()),
            &policy,
            &error.to_string(),
        ),
    )
}

pub(crate) fn autonomous_research_provider_fault_outcome_from_error(
    job: &AutonomousResearchJobState,
    provider_trace: &crate::providers::ProviderResolutionTrace,
    source: &str,
    error: &ProviderExecutionError,
) -> AutonomousResearchProviderFaultState {
    let policy = classify_provider_execution_error(error);
    autonomous_research_build_provider_fault_state(
        job,
        source,
        Some(provider_trace.resolved_provider.clone()),
        Some(provider_trace.resolved_model.clone()),
        &policy,
        &error.to_string(),
    )
}

pub(crate) fn autonomous_research_should_run_main_agent(
    resolved: &ResolvedProject,
    job: &AutonomousResearchJobState,
    tick: &AutonomousResearchTickSummary,
) -> bool {
    let main_agent_must_handle_exception = job
        .last_review
        .as_ref()
        .map(|review| review.verdict != "pass")
        .unwrap_or(false)
        || autonomous_research_has_pending_worker_artifact_decisions(resolved, job)
        || autonomous_research_has_unresolved_worker_review_failures(resolved, job)
        || !autonomous_research_open_blocking_obligations(job).is_empty()
        || job.phase == "review_readiness_blocked"
        || autonomous_research_tick_requires_main_agent_blocked_decision(job, tick);
    if !main_agent_must_handle_exception
        && autonomous_research_has_active_stage_running_worker_claims(resolved, job)
    {
        return false;
    }
    job.ticks_completed == 1
        || tick.accepted
        || tick.dispatch_count > 0
        || main_agent_must_handle_exception
        || job.provider_rounds.is_empty()
        || !autonomous_research_stage_artifact_has_reviewable_content(resolved, job)
        || autonomous_research_stalled_ticks(job) >= AUTONOMOUS_RESEARCH_STALLED_AGENT_TRIGGER_TICKS
}

pub(crate) fn autonomous_research_worker_acceptance_wait_reason(
    resolved: &ResolvedProject,
    job: &AutonomousResearchJobState,
    tick: &AutonomousResearchTickSummary,
) -> Option<(String, Vec<String>)> {
    let contract = autonomous_research_stage_contract(resolved, job);
    let index = load_autonomous_research_accepted_worker_evidence_index(resolved, job);
    let missing_task_types =
        accepted_worker_evidence_missing_required_task_types(&contract, index.as_ref());
    if autonomous_research_tick_requires_main_agent_blocked_decision(job, tick) {
        return None;
    }
    if autonomous_research_has_pending_worker_artifact_decisions(resolved, job) {
        return None;
    }
    if autonomous_research_has_unresolved_worker_review_failures(resolved, job) {
        return None;
    }
    if tick.dispatch_count > 0 {
        return Some((
            "stage-local worker tasks were dispatched this tick; the next tick must accept completed worker evidence before synthesis or review".to_string(),
            missing_task_types,
        ));
    }
    let active_running_worker_task_types =
        autonomous_research_active_stage_running_worker_task_types(resolved, job);
    if !active_running_worker_task_types.is_empty() {
        return Some((
            "stage-local agent-team worker tasks are still running; runtime must wait for worker output before main-agent synthesis or review".to_string(),
            active_running_worker_task_types,
        ));
    }
    if tick.accepted && !missing_task_types.is_empty() {
        return Some((
            "stage-local worker evidence was accepted this tick, but required task coverage is still incomplete; continue accepting worker evidence before synthesis or review".to_string(),
            missing_task_types,
        ));
    }
    if !missing_task_types.is_empty()
        && autonomous_research_stage_has_dispatchable_worker_debt(
            resolved,
            job,
            &missing_task_types,
        )
    {
        return Some((
            "required stage-local worker evidence is still missing and board-visible stage tasks are available; do not run main-agent synthesis or review until agent-team evidence is accepted".to_string(),
            missing_task_types,
        ));
    }
    None
}

pub(crate) fn autonomous_research_main_agent_published_worker_task_wait_reason(
    resolved: &ResolvedProject,
    job: &AutonomousResearchJobState,
    dispatchable_task_ids_before_round: &[String],
) -> Option<(String, Vec<String>)> {
    let contract = autonomous_research_stage_contract(resolved, job);
    let index = load_autonomous_research_accepted_worker_evidence_index(resolved, job);
    let missing_task_types =
        accepted_worker_evidence_missing_required_task_types(&contract, index.as_ref());
    let dispatchable_tasks = autonomous_research_dispatchable_main_agent_stage_tasks(resolved, job);
    let missing_dispatchable_task_types = dispatchable_tasks
        .iter()
        .filter(|task| {
            missing_task_types
                .iter()
                .any(|missing| missing == &task.task_type)
        })
        .map(|task| task.task_type.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    if !missing_task_types.is_empty() && !missing_dispatchable_task_types.is_empty() {
        return Some((
            "main agent published dispatchable stage-local board tasks; runtime must give the agent team a tick to execute and accept worker evidence before synthesis or review".to_string(),
            missing_dispatchable_task_types,
        ));
    }

    let before = dispatchable_task_ids_before_round
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    let new_dispatchable_task_types = dispatchable_tasks
        .iter()
        .filter(|task| !before.contains(&task.task_id))
        .map(|task| task.task_type.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let revised_dispatchable_task_types =
        autonomous_research_revised_dispatchable_task_types_after_main_agent_round(
            resolved,
            job,
            &dispatchable_tasks,
        );
    if new_dispatchable_task_types.is_empty() && revised_dispatchable_task_types.is_empty() {
        return None;
    }
    let mut task_types = new_dispatchable_task_types;
    merge_unique_strings(&mut task_types, revised_dispatchable_task_types);
    Some((
        "main agent published or revised dispatchable stage-local worker tasks; runtime must give the agent team a tick to execute and accept worker evidence before synthesis or review".to_string(),
        task_types,
    ))
}

pub(crate) fn autonomous_research_revised_dispatchable_task_types_after_main_agent_round(
    resolved: &ResolvedProject,
    _job: &AutonomousResearchJobState,
    dispatchable_tasks: &[crate::goals::GoalStageTaskMetadata],
) -> Vec<String> {
    dispatchable_tasks
        .iter()
        .filter(|task| {
            autonomous_research_board_task_revision_supersedes_agent_claim(resolved, task)
        })
        .map(|task| task.task_type.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

pub(crate) fn autonomous_research_board_task_revision_supersedes_agent_claim(
    resolved: &ResolvedProject,
    task: &crate::goals::GoalStageTaskMetadata,
) -> bool {
    let Some(revision) =
        autonomous_research_main_agent_board_task_revision_timestamp(resolved, task)
    else {
        return false;
    };
    let revision_ms = normalize_runtime_timestamp_millis(revision);
    autonomous_research_agent_claim_timestamps_for_task(resolved, &task.task_id)
        .into_iter()
        .any(|claim_timestamp| revision_ms > normalize_runtime_timestamp_millis(claim_timestamp))
}

pub(crate) fn autonomous_research_main_agent_board_task_revision_timestamp(
    resolved: &ResolvedProject,
    task: &crate::goals::GoalStageTaskMetadata,
) -> Option<u128> {
    let path = resolved
        .data_dir
        .join("main-agent-board")
        .join("tasks")
        .join(format!(
            "{}.json",
            sanitize_file_component_runtime(&task.task_id)
        ));
    let content = std::fs::read_to_string(path).ok()?;
    let value: Value = serde_json::from_str(&content).ok()?;
    value
        .get("updated_at")
        .or_else(|| value.get("created_at"))
        .and_then(|value| value.as_str())
        .and_then(|value| value.parse::<u128>().ok())
}

pub(crate) fn autonomous_research_agent_claim_timestamps_for_task(
    resolved: &ResolvedProject,
    task_id: &str,
) -> Vec<u128> {
    let Ok(Some(run)) = crate::orchestration::load_active_run(&resolved.data_dir) else {
        return Vec::new();
    };
    let claim_prefix = format!("goal_task_claim::{task_id}::agent:agent_");
    let closed_prefix = format!("goal_task_claim_closed::{task_id}::agent:agent_");
    let blocked_prefix = format!("goal_task_claim_blocked::{task_id}::agent:agent_");
    let mut timestamps = Vec::new();
    for step in run.steps {
        for artifact in step.artifacts {
            let Some(agent_suffix) = artifact
                .strip_prefix(&claim_prefix)
                .or_else(|| artifact.strip_prefix(&closed_prefix))
                .or_else(|| artifact.strip_prefix(&blocked_prefix))
                .and_then(|rest| rest.split("::").next())
            else {
                continue;
            };
            let agent_id = format!("agent_{agent_suffix}");
            if let Some(timestamp) = autonomous_research_agent_timestamp(&agent_id) {
                timestamps.push(timestamp);
            }
        }
    }
    timestamps.sort();
    timestamps.dedup();
    timestamps
}

pub(crate) fn autonomous_research_agent_timestamp(agent_id: &str) -> Option<u128> {
    agent_id.strip_prefix("agent_")?.parse::<u128>().ok()
}

pub(crate) fn normalize_runtime_timestamp_millis(timestamp: u128) -> u128 {
    if timestamp > 9_999_999_999_999 {
        timestamp / 1_000
    } else {
        timestamp
    }
}

pub(crate) fn sanitize_file_component_runtime(value: &str) -> String {
    let sanitized = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>();
    if sanitized.trim_matches('_').is_empty() {
        "item".to_string()
    } else {
        sanitized
    }
}
