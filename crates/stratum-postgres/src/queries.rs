//! Ledger-derived read queries.
//!
//! No projection tables exist: the Agent view, product history, resume reads
//! and approval facts are all derived from `agents`, `agent_state` and the
//! durable ledger at a fixed high-water barrier.

use sqlx::{PgPool, Row};
use stratum_core::{
    AgentId, AgentVersionId, ChatRole, DurableAgentEvent, HookInvocationId, SessionId, TokenUsage,
    TurnId,
};
use uuid::Uuid;

use crate::codec::{self, CompanionFacts};
use crate::error::PostgresError;
use crate::types::{
    AgentStateView, AgentStatus, AgentView, ApprovalFacts, ApprovalLookup, ApprovalResolution,
    CreateKeyLookup, DurableEventRow, HISTORY_MAX_LIMIT, HISTORY_SOFT_PAGE_BUDGET_BYTES,
    HistoryItem, HistoryPage, HistoryQuery, HookInvocationLookup, LoopStartedRecord,
    PendingApproval, ResumeSliceQuery, TranscriptCompaction, u64_to_bigint,
};

const APPROVAL_DERIVATION_EVENT_TYPES: [&str; 3] = [
    "tool_approval_requested",
    "tool_approval_resolved",
    "hook_invocation_completed",
];
const HOOK_DERIVATION_EVENT_TYPES: [&str; 3] = [
    "hook_invocation_pending",
    "hook_invocation_completed",
    "hook_invocation_failed",
];
const TELEMETRY_FLOOR_SCAN_PAGE_SIZE: i64 = 32;

/// Converts a persisted `bigint` sequence into the crate's `u64` domain.
fn seq_from_i64(value: i64) -> Result<u64, PostgresError> {
    u64::try_from(value)
        .map_err(|_| PostgresError::corrupt_invariant("persisted bigint sequence is negative"))
}

/// Validates the version and one-to-zero companion relation of a specialized
/// selector row whose event type is known to be non-compaction.
pub(crate) fn decode_non_compaction_event_version(
    row: &sqlx::postgres::PgRow,
) -> Result<i32, PostgresError> {
    let event_version: i32 = row
        .try_get("event_version")
        .map_err(PostgresError::StoreUnavailable)?;
    codec::ensure_supported_event_version(event_version)?;
    let has_compaction_companion: bool = row
        .try_get("has_compaction_companion")
        .map_err(PostgresError::StoreUnavailable)?;
    codec::ensure_non_compaction_companion_absent(has_compaction_companion)?;
    Ok(event_version)
}

/// Strictly decodes one non-compaction fact before a specialized derivation
/// uses its JSON fields for inclusion or exclusion.
fn decode_derivation_fact(row: &sqlx::postgres::PgRow) -> Result<(), PostgresError> {
    let event_version = decode_non_compaction_event_version(row)?;
    let event_type: String = row
        .try_get("event_type")
        .map_err(PostgresError::StoreUnavailable)?;
    let payload: serde_json::Value = row
        .try_get("payload")
        .map_err(PostgresError::StoreUnavailable)?;
    match event_type.as_str() {
        "tool_approval_requested" => {
            codec::RequestedApprovalPayload::decode(event_version, payload)?;
        }
        "tool_approval_resolved" => {
            codec::ResolvedApprovalPayload::decode(event_version, payload)?;
        }
        _ => {
            codec::decode_event(&event_type, event_version, payload, None)?;
        }
    }
    Ok(())
}

/// Guards every non-compaction fact participating in an existence-based
/// derivation before a `NOT EXISTS` clause can hide it.
async fn ensure_derivation_rows_compatible(
    tx: &mut sqlx::PgConnection,
    agent_id: AgentId,
    turn_id: TurnId,
    through_event_seq: Option<u64>,
    event_types: &[&str],
) -> Result<(), PostgresError> {
    let through_event_seq = through_event_seq
        .map(|value| u64_to_bigint(value, "derivation barrier exceeds bigint range"))
        .transpose()?;
    let rows = sqlx::query(
        "SELECT d.event_type, d.event_version, d.payload, \
             tc.event_seq IS NOT NULL AS has_compaction_companion \
         FROM durable_events d \
         LEFT JOIN transcript_compactions tc \
             ON tc.agent_id = d.agent_id AND tc.event_seq = d.event_seq \
         WHERE d.agent_id = $1 AND d.turn_id = $2 \
             AND ($3::bigint IS NULL OR d.event_seq <= $3) \
             AND d.event_type = ANY($4)",
    )
    .bind(agent_id.as_uuid())
    .bind(turn_id.as_uuid())
    .bind(through_event_seq)
    .bind(event_types)
    .fetch_all(&mut *tx)
    .await
    .map_err(PostgresError::StoreUnavailable)?;
    for row in &rows {
        decode_derivation_fact(row)?;
    }
    Ok(())
}

/// Strictly decodes one approval-request row selected by a specialized
/// derivation.
fn decode_requested_approval_row(
    row: &sqlx::postgres::PgRow,
) -> Result<(u64, codec::RequestedApprovalPayload), PostgresError> {
    let event_seq: i64 = row
        .try_get("event_seq")
        .map_err(PostgresError::StoreUnavailable)?;
    let event_version = decode_non_compaction_event_version(row)?;
    let payload: serde_json::Value = row
        .try_get("payload")
        .map_err(PostgresError::StoreUnavailable)?;
    Ok((
        seq_from_i64(event_seq)?,
        codec::RequestedApprovalPayload::decode(event_version, payload)?,
    ))
}

/// Strictly decodes one approval-resolution row selected by a specialized
/// derivation.
fn decode_resolved_approval_row(
    row: &sqlx::postgres::PgRow,
) -> Result<(u64, codec::ResolvedApprovalPayload), PostgresError> {
    let event_seq: i64 = row
        .try_get("event_seq")
        .map_err(PostgresError::StoreUnavailable)?;
    let event_version = decode_non_compaction_event_version(row)?;
    let payload: serde_json::Value = row
        .try_get("payload")
        .map_err(PostgresError::StoreUnavailable)?;
    Ok((
        seq_from_i64(event_seq)?,
        codec::ResolvedApprovalPayload::decode(event_version, payload)?,
    ))
}

/// Converts an optional persisted UUID identity.
fn optional_id<T: From<Uuid>>(value: Option<Uuid>) -> Option<T> {
    value.map(T::from)
}

/// Approximate serialized size of one typed event, for the soft page budget.
fn event_size(event: &DurableAgentEvent) -> usize {
    serde_json::to_vec(event).map_or(0, |bytes| bytes.len())
}

/// Reads the thin durable Agent state.
#[tracing::instrument(skip_all, fields(agent_id = %agent_id))]
pub(crate) async fn read_agent_state(
    pool: &PgPool,
    agent_id: AgentId,
) -> Result<AgentStateView, PostgresError> {
    let row = sqlx::query(
        "SELECT status, session_id, current_turn_id, default_model_config, last_event_seq \
         FROM agent_state WHERE agent_id = $1",
    )
    .bind(agent_id.as_uuid())
    .fetch_optional(pool)
    .await
    .map_err(PostgresError::StoreUnavailable)?
    .ok_or(PostgresError::AgentNotFound { agent_id })?;

    decode_state_row(&row)
}

fn decode_state_row(row: &sqlx::postgres::PgRow) -> Result<AgentStateView, PostgresError> {
    let status_text: String = row
        .try_get("status")
        .map_err(PostgresError::StoreUnavailable)?;
    let default_model_config: serde_json::Value = row
        .try_get("default_model_config")
        .map_err(PostgresError::StoreUnavailable)?;
    let last_event_seq: i64 = row
        .try_get("last_event_seq")
        .map_err(PostgresError::StoreUnavailable)?;
    Ok(AgentStateView {
        status: status_text.parse()?,
        session_id: optional_id(
            row.try_get("session_id")
                .map_err(PostgresError::StoreUnavailable)?,
        ),
        current_turn_id: optional_id(
            row.try_get("current_turn_id")
                .map_err(PostgresError::StoreUnavailable)?,
        ),
        default_model_config: codec::decode_model_config(
            default_model_config,
            "agent_state.default_model_config failed v1 decode",
        )?,
        last_event_seq: seq_from_i64(last_event_seq)?,
    })
}

/// Reads the Agent view in one MVCC snapshot: identity and thin state plus
/// the barrier-derived pending approvals, latest usage and telemetry floor.
#[tracing::instrument(skip_all, fields(agent_id = %agent_id))]
pub(crate) async fn read_agent_view(
    pool: &PgPool,
    agent_id: AgentId,
) -> Result<AgentView, PostgresError> {
    let mut tx = pool
        .begin()
        .await
        .map_err(PostgresError::StoreUnavailable)?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY")
        .execute(&mut *tx)
        .await
        .map_err(PostgresError::StoreUnavailable)?;

    let row = sqlx::query(
        "SELECT a.agent_version_id, a.source_template_name, a.creation_model_override, \
             a.definition_schema_version, a.resolved_definition, a.created_at, \
             s.status, s.session_id, s.current_turn_id, s.default_model_config, s.last_event_seq \
         FROM agents a JOIN agent_state s ON s.agent_id = a.agent_id \
         WHERE a.agent_id = $1",
    )
    .bind(agent_id.as_uuid())
    .fetch_optional(&mut *tx)
    .await
    .map_err(PostgresError::StoreUnavailable)?
    .ok_or(PostgresError::AgentNotFound { agent_id })?;

    let state = decode_state_row(&row)?;
    let barrier = state.last_event_seq;

    let pending_approvals = match (state.status, state.current_turn_id) {
        (AgentStatus::Running, Some(turn_id)) => {
            read_pending_approvals(&mut tx, agent_id, turn_id, barrier).await?
        }
        (AgentStatus::Running, None) => {
            return Err(PostgresError::corrupt_invariant(
                "running agent_state lacks current_turn_id",
            ));
        }
        // A terminal Turn invalidates every unconsumed approval; an idle
        // Agent never had one.
        (
            AgentStatus::Idle
            | AgentStatus::Finished
            | AgentStatus::Failed
            | AgentStatus::Cancelled,
            _,
        ) => Vec::new(),
    };
    let latest_usage = match state.current_turn_id {
        Some(turn_id) => read_latest_usage(&mut tx, agent_id, turn_id, barrier).await?,
        None => None,
    };
    let telemetry_floor_event_seq = read_telemetry_floor(&mut tx, agent_id, barrier).await?;

    tx.commit().await.map_err(PostgresError::StoreUnavailable)?;

    let creation_model_override: Option<serde_json::Value> = row
        .try_get("creation_model_override")
        .map_err(PostgresError::StoreUnavailable)?;
    Ok(AgentView {
        agent_id,
        agent_version_id: AgentVersionId::from(
            row.try_get::<Uuid, _>("agent_version_id")
                .map_err(PostgresError::StoreUnavailable)?,
        ),
        source_template_name: row
            .try_get("source_template_name")
            .map_err(PostgresError::StoreUnavailable)?,
        creation_model_override: codec::decode_optional_model_config(
            creation_model_override,
            "agents.creation_model_override failed v1 decode",
        )?,
        definition_schema_version: row
            .try_get("definition_schema_version")
            .map_err(PostgresError::StoreUnavailable)?,
        resolved_definition: row
            .try_get("resolved_definition")
            .map_err(PostgresError::StoreUnavailable)?,
        created_at: row
            .try_get("created_at")
            .map_err(PostgresError::StoreUnavailable)?,
        status: state.status,
        session_id: state.session_id,
        current_turn_id: state.current_turn_id,
        default_model_config: state.default_model_config,
        snapshot_event_seq: barrier,
        telemetry_floor_event_seq,
        pending_approvals,
        latest_usage,
    })
}

/// Latest assistant `MessageAppended` sequence within the fixed AgentView
/// barrier. Rows are decoded newest-first instead of filtering on JSON role:
/// an unsupported or malformed newer message must fail closed rather than be
/// skipped and produce a stale telemetry floor. Server-side keyset pages keep
/// both PostgreSQL work per query and Rust allocation bounded for Agents with
/// a long tool/user suffix.
async fn read_telemetry_floor(
    tx: &mut sqlx::PgConnection,
    agent_id: AgentId,
    barrier: u64,
) -> Result<u64, PostgresError> {
    let barrier = u64_to_bigint(barrier, "telemetry floor barrier exceeds bigint range")?;
    let statement = format!(
        "{EVENT_ROW_SELECT} \
         WHERE d.agent_id = $1 AND d.event_seq <= $2 \
             AND ($3::bigint IS NULL OR d.event_seq < $3) \
             AND d.event_type = 'message_appended' \
         ORDER BY d.event_seq DESC \
         LIMIT $4"
    );

    let mut before_event_seq = None;
    loop {
        let rows = sqlx::query(&statement)
            .bind(agent_id.as_uuid())
            .bind(barrier)
            .bind(before_event_seq)
            .bind(TELEMETRY_FLOOR_SCAN_PAGE_SIZE)
            .fetch_all(&mut *tx)
            .await
            .map_err(PostgresError::StoreUnavailable)?;
        if rows.is_empty() {
            return Ok(0);
        }

        let mut oldest_event_seq = None;
        for row in &rows {
            let decoded = decode_event_row(row)?;
            oldest_event_seq = Some(u64_to_bigint(
                decoded.event_seq,
                "telemetry floor row sequence exceeds bigint range",
            )?);
            let DurableAgentEvent::MessageAppended { message } = decoded.event else {
                return Err(PostgresError::corrupt_invariant(
                    "telemetry floor query selected a non-message event",
                ));
            };
            if message.role == ChatRole::Assistant {
                return Ok(decoded.event_seq);
            }
        }
        before_event_seq = oldest_event_seq;
    }
}

/// Pending approvals of the current Turn: Requested minus Resolved minus
/// Consumed, ordered by requested sequence.
async fn read_pending_approvals(
    tx: &mut sqlx::PgConnection,
    agent_id: AgentId,
    turn_id: TurnId,
    barrier: u64,
) -> Result<Vec<PendingApproval>, PostgresError> {
    // Validate every fact that can participate in either exclusion first. An
    // unsupported version or illegal companion must be explicit; it cannot
    // silently hide an approval through raw JSON existence matching.
    ensure_derivation_rows_compatible(
        tx,
        agent_id,
        turn_id,
        Some(barrier),
        &APPROVAL_DERIVATION_EVENT_TYPES,
    )
    .await?;
    let rows = sqlx::query(
        "SELECT r.event_seq, r.event_version, r.payload, \
             rc.event_seq IS NOT NULL AS has_compaction_companion \
         FROM durable_events r \
         LEFT JOIN transcript_compactions rc \
             ON rc.agent_id = r.agent_id AND rc.event_seq = r.event_seq \
         WHERE r.agent_id = $1 AND r.turn_id = $2 \
             AND r.event_type = 'tool_approval_requested' AND r.event_seq <= $3 \
             AND NOT EXISTS ( \
                 SELECT 1 FROM durable_events x \
                 WHERE x.agent_id = r.agent_id AND x.turn_id = r.turn_id \
                     AND x.event_type = 'tool_approval_resolved' \
                     AND x.payload ->> 'approval_id' = r.payload ->> 'approval_id') \
             AND NOT EXISTS ( \
                 SELECT 1 FROM durable_events c \
                 WHERE c.agent_id = r.agent_id AND c.turn_id = r.turn_id \
                     AND c.event_type = 'hook_invocation_completed' \
                     AND c.payload ->> 'invocation_id' = r.payload ->> 'hook_invocation_id') \
         ORDER BY r.event_seq ASC",
    )
    .bind(agent_id.as_uuid())
    .bind(turn_id.as_uuid())
    .bind(u64_to_bigint(
        barrier,
        "approval barrier exceeds bigint range",
    )?)
    .fetch_all(&mut *tx)
    .await
    .map_err(PostgresError::StoreUnavailable)?;

    let mut approvals = Vec::with_capacity(rows.len());
    for row in rows {
        let (requested_event_seq, requested) = decode_requested_approval_row(&row)?;
        approvals.push(PendingApproval {
            requested_event_seq,
            approval_id: requested.approval_id,
            hook_invocation_id: requested.hook_invocation_id,
            call_id: requested.call_id,
            tool_name: requested.tool_name,
            arguments: requested.arguments,
            tool_kind: requested.tool_kind,
            danger_level: requested.danger_level,
        });
    }
    Ok(approvals)
}

/// Usage of the most recent usage-carrying durable event of the Turn within
/// the barrier; this is the last provider response, not a lifetime total.
async fn read_latest_usage(
    tx: &mut sqlx::PgConnection,
    agent_id: AgentId,
    turn_id: TurnId,
    barrier: u64,
) -> Result<Option<TokenUsage>, PostgresError> {
    let row = sqlx::query(
        "SELECT d.event_type, d.event_version, d.payload, \
             tc.event_seq IS NOT NULL AS has_compaction_companion \
         FROM durable_events d \
         LEFT JOIN transcript_compactions tc \
             ON tc.agent_id = d.agent_id AND tc.event_seq = d.event_seq \
         WHERE d.agent_id = $1 AND d.turn_id = $2 AND d.event_seq <= $3 \
             AND d.event_type = ANY($4) \
         ORDER BY d.event_seq DESC LIMIT 1",
    )
    .bind(agent_id.as_uuid())
    .bind(turn_id.as_uuid())
    .bind(u64_to_bigint(
        barrier,
        "usage barrier exceeds bigint range",
    )?)
    .bind(&codec::USAGE_EVENT_TYPES[..])
    .fetch_optional(&mut *tx)
    .await
    .map_err(PostgresError::StoreUnavailable)?;

    let Some(row) = row else {
        return Ok(None);
    };
    let event_type: String = row
        .try_get("event_type")
        .map_err(PostgresError::StoreUnavailable)?;
    let event_version = decode_non_compaction_event_version(&row)?;
    let payload: serde_json::Value = row
        .try_get("payload")
        .map_err(PostgresError::StoreUnavailable)?;
    let usage = match codec::decode_event(&event_type, event_version, payload, None)? {
        DurableAgentEvent::IterationCompleted { usage, .. }
        | DurableAgentEvent::LoopFinished { usage, .. }
        | DurableAgentEvent::LoopFailed { usage, .. }
        | DurableAgentEvent::LoopCancelled { usage } => usage,
        _ => {
            return Err(PostgresError::corrupt_invariant(
                "usage query selected a non-usage event",
            ));
        }
    };
    Ok(Some(usage))
}

/// Reads one product-history page: database-side reverse pagination, budget
/// trimming from the older side, ascending response.
#[tracing::instrument(skip_all, fields(agent_id = %query.agent_id))]
pub(crate) async fn read_history_page(
    pool: &PgPool,
    query: HistoryQuery,
) -> Result<HistoryPage, PostgresError> {
    let limit = query.limit.clamp(1, HISTORY_MAX_LIMIT);
    let limit_usize = usize::try_from(limit)
        .map_err(|_| PostgresError::InvalidCommand("history limit exceeds platform usize"))?;
    let sql_limit = i64::from(limit)
        .checked_add(1)
        .ok_or(PostgresError::InvalidCommand("history limit overflow"))?;
    let through_event_seq = u64_to_bigint(
        query.through_event_seq,
        "history barrier exceeds bigint range",
    )?;
    let before_event_seq = query
        .before_event_seq
        .map(|value| u64_to_bigint(value, "history cursor exceeds bigint range"))
        .transpose()?;
    // One extra row distinguishes "exactly filled" from "more rows exist".
    let statement = history_statement();
    let rows = sqlx::query(&statement)
        .bind(query.agent_id.as_uuid())
        .bind(through_event_seq)
        .bind(before_event_seq)
        .bind(&codec::HISTORY_EVENT_TYPES[..])
        .bind(sql_limit)
        .fetch_all(pool)
        .await
        .map_err(PostgresError::StoreUnavailable)?;

    let mut page = Vec::with_capacity(rows.len().min(limit_usize));
    for row in &rows {
        page.push(decode_event_row(row)?);
    }
    let has_extra = page.len() > limit_usize;
    page.truncate(limit_usize);

    // Soft byte budget: accumulate from the newest item; the first item is
    // always kept whole even when it alone exceeds the budget.
    let mut bytes = 0usize;
    let mut budget_len = page.len();
    for (index, row) in page.iter().enumerate() {
        let size = event_size(&row.event);
        let Some(next_bytes) = bytes.checked_add(size) else {
            budget_len = index.max(1);
            break;
        };
        if index > 0 && next_bytes > HISTORY_SOFT_PAGE_BUDGET_BYTES {
            budget_len = index;
            break;
        }
        bytes = next_bytes;
    }
    let budget_trimmed = budget_len < page.len();
    page.truncate(budget_len);

    let has_more = budget_trimmed || has_extra;
    // Rows are newest-first; the oldest kept row is the next exclusive cursor.
    let next_before_event_seq = page.last().map(|row| row.event_seq);

    // The response is ascending.
    page.reverse();
    let items = page
        .into_iter()
        .map(|row| HistoryItem {
            event_seq: row.event_seq,
            event_version: row.event_version,
            session_id: row.session_id,
            turn_id: row.turn_id,
            event: row.event,
            created_at: row.created_at,
        })
        .collect();

    Ok(HistoryPage {
        items,
        next_before_event_seq,
        has_more,
    })
}

fn history_statement() -> String {
    format!(
        "{EVENT_ROW_SELECT} \
         WHERE d.agent_id = $1 AND d.event_seq <= $2 \
             AND ($3::bigint IS NULL OR d.event_seq < $3) \
             AND d.event_type = ANY($4) \
         ORDER BY d.event_seq DESC LIMIT $5"
    )
}

/// Reads the single `LoopStarted` row of one exact Turn with its runtime
/// snapshot.
#[tracing::instrument(skip_all, fields(agent_id = %agent_id, turn_id = %turn_id))]
pub(crate) async fn read_loop_started(
    pool: &PgPool,
    agent_id: AgentId,
    turn_id: TurnId,
) -> Result<LoopStartedRecord, PostgresError> {
    let row = sqlx::query(
        "SELECT d.event_seq, d.session_id, d.event_version, d.runtime_snapshot_version, \
             d.runtime_snapshot, d.created_at, \
             tc.event_seq IS NOT NULL AS has_compaction_companion \
         FROM durable_events d \
         LEFT JOIN transcript_compactions tc \
             ON tc.agent_id = d.agent_id AND tc.event_seq = d.event_seq \
         WHERE d.agent_id = $1 AND d.turn_id = $2 AND d.event_type = 'loop_started'",
    )
    .bind(agent_id.as_uuid())
    .bind(turn_id.as_uuid())
    .fetch_optional(pool)
    .await
    .map_err(PostgresError::StoreUnavailable)?
    .ok_or(PostgresError::TurnNotFound { agent_id, turn_id })?;

    decode_non_compaction_event_version(&row)?;

    let event_seq: i64 = row
        .try_get("event_seq")
        .map_err(PostgresError::StoreUnavailable)?;
    let snapshot_version: Option<i32> = row
        .try_get("runtime_snapshot_version")
        .map_err(PostgresError::StoreUnavailable)?;
    let snapshot: Option<serde_json::Value> = row
        .try_get("runtime_snapshot")
        .map_err(PostgresError::StoreUnavailable)?;
    let (snapshot_version, snapshot) =
        snapshot_version
            .zip(snapshot)
            .ok_or(PostgresError::corrupt_invariant(
                "loop_started row lacks its runtime snapshot",
            ))?;

    Ok(LoopStartedRecord {
        event_seq: seq_from_i64(event_seq)?,
        session_id: SessionId::from(
            row.try_get::<Uuid, _>("session_id")
                .map_err(PostgresError::StoreUnavailable)?,
        ),
        turn_id,
        snapshot_version,
        snapshot: codec::decode_runtime_snapshot(snapshot_version, snapshot)?,
        created_at: row
            .try_get("created_at")
            .map_err(PostgresError::StoreUnavailable)?,
    })
}

/// Reads the complete gapless `(base, through]` current-Turn slice for
/// resume, verifying exact Session/Turn identity and continuity.
#[tracing::instrument(skip_all, fields(agent_id = %query.agent_id, turn_id = %query.turn_id))]
pub(crate) async fn read_resume_slice(
    pool: &PgPool,
    query: ResumeSliceQuery,
) -> Result<Vec<DurableEventRow>, PostgresError> {
    let rows = read_events_range(
        pool,
        query.agent_id,
        query.base_event_seq,
        query.through_event_seq,
    )
    .await?;

    let expected_len = query
        .through_event_seq
        .checked_sub(query.base_event_seq)
        .ok_or(PostgresError::InvalidCommand(
            "resume through sequence precedes base sequence",
        ))?;
    let actual_len = u64::try_from(rows.len())
        .map_err(|_| PostgresError::corrupt_invariant("resume truth slice length exceeds u64"))?;
    if actual_len != expected_len {
        return Err(PostgresError::corrupt_invariant(
            "current-turn truth slice has missing rows",
        ));
    }
    let mut expected_seq = query.base_event_seq;
    for row in &rows {
        expected_seq = expected_seq.checked_add(1).ok_or_else(|| {
            PostgresError::corrupt_invariant("current-turn truth sequence overflowed")
        })?;
        if row.event_seq != expected_seq {
            return Err(PostgresError::corrupt_invariant(
                "current-turn truth slice is not gapless",
            ));
        }
        if row.session_id != query.session_id || row.turn_id != query.turn_id {
            return Err(PostgresError::corrupt_invariant(
                "current-turn truth slice contains a row with foreign identity",
            ));
        }
    }
    if !matches!(
        rows.first().map(|row| &row.event),
        Some(DurableAgentEvent::LoopStarted { .. })
    ) {
        return Err(PostgresError::corrupt_invariant(
            "current-turn truth slice does not start at loop_started",
        ));
    }
    Ok(rows)
}

/// Reads decoded durable rows in `(from_event_seq, to_event_seq]` for one
/// Agent, in order. Used by the realtime dispatcher and by full-replay
/// recovery (`from = 0`).
#[tracing::instrument(skip_all, fields(agent_id = %agent_id))]
pub(crate) async fn read_events_range(
    pool: &PgPool,
    agent_id: AgentId,
    from_event_seq: u64,
    to_event_seq: u64,
) -> Result<Vec<DurableEventRow>, PostgresError> {
    let from_event_seq_db = u64_to_bigint(from_event_seq, "range start exceeds bigint range")?;
    let to_event_seq_db = u64_to_bigint(to_event_seq, "range end exceeds bigint range")?;
    if from_event_seq >= to_event_seq {
        return Ok(Vec::new());
    }
    let statement = format!(
        "{EVENT_ROW_SELECT} \
         WHERE d.agent_id = $1 AND d.event_seq > $2 AND d.event_seq <= $3 \
         ORDER BY d.event_seq ASC"
    );
    let rows = sqlx::query(&statement)
        .bind(agent_id.as_uuid())
        .bind(from_event_seq_db)
        .bind(to_event_seq_db)
        .fetch_all(pool)
        .await
        .map_err(PostgresError::StoreUnavailable)?;

    let mut events = Vec::with_capacity(rows.len());
    for row in &rows {
        events.push(decode_event_row(row)?);
    }
    Ok(events)
}

/// Reads the latest compaction companion at or below `base_event_seq`,
/// validating its discriminator identity.
#[tracing::instrument(skip_all, fields(agent_id = %agent_id))]
pub(crate) async fn read_latest_companion(
    pool: &PgPool,
    agent_id: AgentId,
    base_event_seq: u64,
) -> Result<Option<TranscriptCompaction>, PostgresError> {
    let row = sqlx::query(
        "SELECT c.event_seq, c.turn_id, c.compacted_iteration, c.upto, c.retained_from_event_seq, \
             c.summary, c.created_at, d.turn_id AS discriminator_turn_id, d.event_type, \
             d.event_version, d.payload \
         FROM transcript_compactions c \
         JOIN durable_events d ON d.agent_id = c.agent_id AND d.event_seq = c.event_seq \
         WHERE c.agent_id = $1 AND c.event_seq <= $2 \
         ORDER BY c.event_seq DESC LIMIT 1",
    )
    .bind(agent_id.as_uuid())
    .bind(u64_to_bigint(
        base_event_seq,
        "companion barrier exceeds bigint range",
    )?)
    .fetch_optional(pool)
    .await
    .map_err(PostgresError::StoreUnavailable)?;

    row.map(|row| decode_companion_row(agent_id, &row))
        .transpose()
}

fn decode_companion_row(
    agent_id: AgentId,
    row: &sqlx::postgres::PgRow,
) -> Result<TranscriptCompaction, PostgresError> {
    let discriminator_type: String = row
        .try_get("event_type")
        .map_err(PostgresError::StoreUnavailable)?;
    if discriminator_type != codec::TYPE_TRANSCRIPT_COMPACTED {
        return Err(PostgresError::corrupt_invariant(
            "companion row is attached to a non-compaction discriminator",
        ));
    }
    let turn_id: Uuid = row
        .try_get("turn_id")
        .map_err(PostgresError::StoreUnavailable)?;
    let discriminator_turn_id: Uuid = row
        .try_get("discriminator_turn_id")
        .map_err(PostgresError::StoreUnavailable)?;
    if turn_id != discriminator_turn_id {
        return Err(PostgresError::corrupt_invariant(
            "companion turn identity disagrees with its discriminator",
        ));
    }

    let compacted_iteration = seq_from_i64(
        row.try_get("compacted_iteration")
            .map_err(PostgresError::StoreUnavailable)?,
    )?;
    let upto = seq_from_i64(
        row.try_get("upto")
            .map_err(PostgresError::StoreUnavailable)?,
    )?;
    let summary: serde_json::Value = row
        .try_get("summary")
        .map_err(PostgresError::StoreUnavailable)?;
    let event_version: i32 = row
        .try_get("event_version")
        .map_err(PostgresError::StoreUnavailable)?;
    let payload: serde_json::Value = row
        .try_get("payload")
        .map_err(PostgresError::StoreUnavailable)?;
    let event = codec::decode_event(
        codec::TYPE_TRANSCRIPT_COMPACTED,
        event_version,
        payload,
        Some(CompanionFacts {
            upto,
            compacted_iteration,
            summary,
        }),
    )?;
    let DurableAgentEvent::TranscriptCompacted {
        upto,
        summary,
        compacted_iteration,
    } = event
    else {
        return Err(PostgresError::corrupt_invariant(
            "compaction discriminator decoded as a different event",
        ));
    };
    Ok(TranscriptCompaction {
        agent_id,
        event_seq: seq_from_i64(
            row.try_get("event_seq")
                .map_err(PostgresError::StoreUnavailable)?,
        )?,
        turn_id: TurnId::from(turn_id),
        compacted_iteration,
        upto,
        retained_from_event_seq: seq_from_i64(
            row.try_get("retained_from_event_seq")
                .map_err(PostgresError::StoreUnavailable)?,
        )?,
        summary,
        created_at: row
            .try_get("created_at")
            .map_err(PostgresError::StoreUnavailable)?,
    })
}

/// Reads ledger facts about one approval request for the decide Handler.
#[tracing::instrument(skip_all, fields(agent_id = %agent_id, turn_id = %turn_id))]
pub(crate) async fn read_approval(
    pool: &PgPool,
    agent_id: AgentId,
    turn_id: TurnId,
    lookup: ApprovalLookup,
) -> Result<Option<ApprovalFacts>, PostgresError> {
    let (clause, key) = match lookup {
        ApprovalLookup::ByApprovalId(approval_id) => (
            "r.payload ->> 'approval_id'",
            approval_id.as_uuid().to_string(),
        ),
        ApprovalLookup::ByHookInvocationId(hook_invocation_id) => (
            "r.payload ->> 'hook_invocation_id'",
            hook_invocation_id.as_uuid().to_string(),
        ),
    };
    let statement = format!(
        "SELECT r.event_seq, r.event_version, r.payload, \
             rc.event_seq IS NOT NULL AS has_compaction_companion \
         FROM durable_events r \
         LEFT JOIN transcript_compactions rc \
             ON rc.agent_id = r.agent_id AND rc.event_seq = r.event_seq \
         WHERE r.agent_id = $1 AND r.turn_id = $2 \
             AND r.event_type = 'tool_approval_requested' \
             AND {clause} = $3"
    );
    let requested_rows = sqlx::query(&statement)
        .bind(agent_id.as_uuid())
        .bind(turn_id.as_uuid())
        .bind(key)
        .fetch_all(pool)
        .await
        .map_err(PostgresError::StoreUnavailable)?;
    let mut requested_match = None;
    for row in requested_rows {
        let (event_seq, requested) = decode_requested_approval_row(&row)?;
        let identity_matches = match lookup {
            ApprovalLookup::ByApprovalId(approval_id) => requested.approval_id == approval_id,
            ApprovalLookup::ByHookInvocationId(hook_invocation_id) => {
                requested.hook_invocation_id == hook_invocation_id
            }
        };
        if !identity_matches {
            return Err(PostgresError::corrupt_invariant(
                "approval lookup index disagrees with decoded identity",
            ));
        }
        if requested_match.replace((event_seq, requested)).is_some() {
            return Err(PostgresError::corrupt_invariant(
                "approval lookup matched multiple request rows",
            ));
        }
    }
    let Some((requested_seq, requested)) = requested_match else {
        return Ok(None);
    };

    let resolved_rows = sqlx::query(
        "SELECT r.event_seq, r.turn_id, r.event_version, r.payload, \
             rc.event_seq IS NOT NULL AS has_compaction_companion \
         FROM durable_events r \
         LEFT JOIN transcript_compactions rc \
             ON rc.agent_id = r.agent_id AND rc.event_seq = r.event_seq \
         WHERE r.agent_id = $1 AND r.event_type = 'tool_approval_resolved' \
             AND r.payload ->> 'approval_id' = $2",
    )
    .bind(agent_id.as_uuid())
    .bind(requested.approval_id.as_uuid().to_string())
    .fetch_all(pool)
    .await
    .map_err(PostgresError::StoreUnavailable)?;
    let mut resolution = None;
    for row in resolved_rows {
        let resolved_turn_id: Uuid = row
            .try_get("turn_id")
            .map_err(PostgresError::StoreUnavailable)?;
        if resolved_turn_id != turn_id.as_uuid() {
            return Err(PostgresError::corrupt_invariant(
                "approval resolution belongs to a different turn",
            ));
        }
        let (resolved_event_seq, resolved) = decode_resolved_approval_row(&row)?;
        if resolved.approval_id != requested.approval_id {
            return Err(PostgresError::corrupt_invariant(
                "approval resolution index disagrees with decoded identity",
            ));
        }
        if resolution
            .replace(ApprovalResolution {
                resolved_event_seq,
                decision: resolved.decision,
            })
            .is_some()
        {
            return Err(PostgresError::corrupt_invariant(
                "approval lookup matched multiple resolution rows",
            ));
        }
    }

    Ok(Some(ApprovalFacts {
        requested_event_seq: requested_seq,
        approval_id: requested.approval_id,
        hook_invocation_id: requested.hook_invocation_id,
        call_id: requested.call_id,
        tool_name: requested.tool_name,
        arguments: requested.arguments,
        tool_kind: requested.tool_kind,
        danger_level: requested.danger_level,
        resolution,
    }))
}

/// Reads the stored create request behind one idempotency key, without any
/// template access.
#[tracing::instrument(skip_all)]
pub(crate) async fn find_agent_by_idempotency_key(
    pool: &PgPool,
    idempotency_key: Uuid,
) -> Result<Option<CreateKeyLookup>, PostgresError> {
    let row = sqlx::query(
        "SELECT agent_id, source_template_name, creation_model_override \
         FROM agents WHERE idempotency_key = $1",
    )
    .bind(idempotency_key)
    .fetch_optional(pool)
    .await
    .map_err(PostgresError::StoreUnavailable)?;

    row.map(|row| {
        let creation_model_override: Option<serde_json::Value> = row
            .try_get("creation_model_override")
            .map_err(PostgresError::StoreUnavailable)?;
        Ok(CreateKeyLookup {
            agent_id: AgentId::from(
                row.try_get::<Uuid, _>("agent_id")
                    .map_err(PostgresError::StoreUnavailable)?,
            ),
            source_template_name: row
                .try_get("source_template_name")
                .map_err(PostgresError::StoreUnavailable)?,
            creation_model_override: codec::decode_optional_model_config(
                creation_model_override,
                "agents.creation_model_override failed v1 decode",
            )?,
        })
    })
    .transpose()
}

/// Finds the one open journaled hook invocation at an exact address: a
/// `HookInvocationPending` row without a matching Completed/Failed row.
#[tracing::instrument(skip_all, fields(agent_id = %lookup.agent_id, turn_id = %lookup.turn_id))]
pub(crate) async fn read_open_hook_invocation(
    pool: &PgPool,
    lookup: HookInvocationLookup,
) -> Result<Option<HookInvocationId>, PostgresError> {
    let point = serde_json::to_value(lookup.point)
        .map_err(|source| PostgresError::EventEncode {
            event_type: "hook_point",
            source,
        })?
        .as_str()
        .map(str::to_owned)
        .ok_or(PostgresError::corrupt_invariant(
            "hook point did not serialize to its wire name",
        ))?;
    let iteration = lookup.iteration.to_string();
    let call_id = lookup.call_id.as_ref().map(ToString::to_string);
    let mut tx = pool
        .begin()
        .await
        .map_err(PostgresError::StoreUnavailable)?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY")
        .execute(&mut *tx)
        .await
        .map_err(PostgresError::StoreUnavailable)?;
    // Validate Pending and every fact that can consume it in the same snapshot
    // used by the NOT EXISTS derivation. Unknown versions and illegal
    // companions are explicit errors, never silent consumption.
    ensure_derivation_rows_compatible(
        &mut tx,
        lookup.agent_id,
        lookup.turn_id,
        None,
        &HOOK_DERIVATION_EVENT_TYPES,
    )
    .await?;
    let rows = sqlx::query(
        "SELECT p.event_version, p.payload, \
             pc.event_seq IS NOT NULL AS has_compaction_companion \
         FROM durable_events p \
         LEFT JOIN transcript_compactions pc \
             ON pc.agent_id = p.agent_id AND pc.event_seq = p.event_seq \
         WHERE p.agent_id = $1 AND p.turn_id = $2 AND p.event_type = 'hook_invocation_pending' \
             AND p.payload ->> 'point' = $3 \
             AND p.payload ->> 'iteration' = $4 \
             AND COALESCE(p.payload ->> 'call_id', '') = COALESCE($5, '') \
             AND NOT EXISTS ( \
                 SELECT 1 FROM durable_events c \
                 WHERE c.agent_id = p.agent_id AND c.turn_id = p.turn_id \
                     AND c.event_type IN ('hook_invocation_completed', 'hook_invocation_failed') \
                     AND c.payload ->> 'invocation_id' = p.payload ->> 'invocation_id') \
         ORDER BY p.event_seq ASC",
    )
    .bind(lookup.agent_id.as_uuid())
    .bind(lookup.turn_id.as_uuid())
    .bind(point)
    .bind(iteration)
    .bind(call_id)
    .fetch_all(&mut *tx)
    .await
    .map_err(PostgresError::StoreUnavailable)?;
    let mut invocation = None;
    for row in rows {
        let event_version = decode_non_compaction_event_version(&row)?;
        let payload: serde_json::Value = row
            .try_get("payload")
            .map_err(PostgresError::StoreUnavailable)?;
        let event = codec::decode_event("hook_invocation_pending", event_version, payload, None)?;
        let DurableAgentEvent::HookInvocationPending {
            invocation_id,
            point,
            iteration,
            call_id,
            ..
        } = event
        else {
            return Err(PostgresError::corrupt_invariant(
                "pending-hook query decoded as a different event",
            ));
        };
        if point != lookup.point || iteration != lookup.iteration || call_id != lookup.call_id {
            return Err(PostgresError::corrupt_invariant(
                "open-hook address index disagrees with decoded payload",
            ));
        }
        if invocation.replace(invocation_id).is_some() {
            return Err(PostgresError::corrupt_invariant(
                "multiple open hook invocations share one address",
            ));
        }
    }
    tx.commit().await.map_err(PostgresError::StoreUnavailable)?;
    Ok(invocation)
}

/// Shared SELECT list joining each `TranscriptCompacted` discriminator with
/// its companion; call sites append their own WHERE/ORDER/LIMIT.
const EVENT_ROW_SELECT: &str = "\
    SELECT d.event_seq, d.session_id, d.turn_id, d.event_type, d.event_version, d.payload, \
        d.created_at, c.upto AS companion_upto, c.compacted_iteration AS companion_iteration, \
        c.summary AS companion_summary, c.turn_id AS companion_turn_id \
    FROM durable_events d \
    LEFT JOIN transcript_compactions c \
        ON c.agent_id = d.agent_id AND c.event_seq = d.event_seq";

/// Strictly decodes one ledger row (with companion join columns) into a
/// typed event row.
fn decode_event_row(row: &sqlx::postgres::PgRow) -> Result<DurableEventRow, PostgresError> {
    let raw_seq: i64 = row
        .try_get("event_seq")
        .map_err(PostgresError::StoreUnavailable)?;
    let event_type: String = row
        .try_get("event_type")
        .map_err(PostgresError::StoreUnavailable)?;
    let event_version: i32 = row
        .try_get("event_version")
        .map_err(PostgresError::StoreUnavailable)?;
    let payload: serde_json::Value = row
        .try_get("payload")
        .map_err(PostgresError::StoreUnavailable)?;

    let companion_turn_id: Option<Uuid> = row
        .try_get("companion_turn_id")
        .map_err(PostgresError::StoreUnavailable)?;
    let companion = companion_turn_id
        .map(|turn_id| {
            let upto: i64 = row
                .try_get("companion_upto")
                .map_err(PostgresError::StoreUnavailable)?;
            let iteration: i64 = row
                .try_get("companion_iteration")
                .map_err(PostgresError::StoreUnavailable)?;
            let summary: serde_json::Value = row
                .try_get("companion_summary")
                .map_err(PostgresError::StoreUnavailable)?;
            let row_turn: Uuid = row
                .try_get("turn_id")
                .map_err(PostgresError::StoreUnavailable)?;
            if turn_id != row_turn {
                return Err(PostgresError::corrupt_invariant(
                    "companion turn identity disagrees with its discriminator",
                ));
            }
            Ok(CompanionFacts {
                upto: seq_from_i64(upto)?,
                compacted_iteration: seq_from_i64(iteration)?,
                summary,
            })
        })
        .transpose()?;

    let event = codec::decode_event(&event_type, event_version, payload, companion)?;
    Ok(DurableEventRow {
        event_seq: seq_from_i64(raw_seq)?,
        event_version,
        session_id: SessionId::from(
            row.try_get::<Uuid, _>("session_id")
                .map_err(PostgresError::StoreUnavailable)?,
        ),
        turn_id: TurnId::from(
            row.try_get::<Uuid, _>("turn_id")
                .map_err(PostgresError::StoreUnavailable)?,
        ),
        event,
        created_at: row
            .try_get("created_at")
            .map_err(PostgresError::StoreUnavailable)?,
    })
}
