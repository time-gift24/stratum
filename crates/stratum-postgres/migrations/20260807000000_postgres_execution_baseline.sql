-- Final execution-storage baseline (openspec change
-- complete-postgres-agent-runtime, design Decision 2).
--
-- Exactly four tables carry all Agent execution truth:
--   agents                 immutable identity + resolved definition snapshot
--   agent_state            thin current/recent-Turn state + high-water
--   durable_events         append-only agent-wide ledger, (agent_id, event_seq)
--   transcript_compactions durable companion for TranscriptCompacted
--
-- There is no sessions table, no message/approval projection, no
-- session-operation claim, no outbox and no rebuild metadata. Enum semantics
-- use TEXT + CHECK (never Postgres enums); core foreign keys are RESTRICT.
-- This baseline is destructive: beta databases are dropped and recreated,
-- including the sqlx migration history.

CREATE TABLE agents (
    agent_id                  uuid PRIMARY KEY,
    agent_version_id          uuid NOT NULL UNIQUE,
    idempotency_key           uuid NOT NULL UNIQUE,
    source_template_name      text NOT NULL,
    creation_model_override   jsonb,
    definition_schema_version integer NOT NULL CHECK (definition_schema_version > 0),
    resolved_definition       jsonb NOT NULL,
    created_at                timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE agent_state (
    agent_id             uuid PRIMARY KEY REFERENCES agents ON DELETE RESTRICT,
    status               text NOT NULL
                         CHECK (status IN ('idle', 'running', 'finished', 'failed', 'cancelled')),
    session_id           uuid,
    current_turn_id      uuid,
    default_model_config jsonb NOT NULL,
    last_event_seq       bigint NOT NULL DEFAULT 0 CHECK (last_event_seq >= 0),
    updated_at           timestamptz NOT NULL DEFAULT now(),
    -- idle agents never started a turn: no session/turn binding and no events
    CONSTRAINT agent_state_idle_shape CHECK (
        status <> 'idle'
        OR (session_id IS NULL AND current_turn_id IS NULL AND last_event_seq = 0)
    ),
    -- running and terminal states always describe one exact recent turn
    CONSTRAINT agent_state_turn_shape CHECK (
        status = 'idle'
        OR (session_id IS NOT NULL AND current_turn_id IS NOT NULL)
    )
);

-- Current Agent-only session single-active: at most one running Agent runtime
-- row per session. This is not a session table or a cross-runtime claim.
CREATE UNIQUE INDEX agent_state_running_session_unique
    ON agent_state (session_id)
    WHERE status = 'running';

CREATE TABLE durable_events (
    agent_id                 uuid NOT NULL REFERENCES agents ON DELETE RESTRICT,
    event_seq                bigint NOT NULL CHECK (event_seq > 0),
    session_id               uuid NOT NULL,
    turn_id                  uuid NOT NULL,
    event_type               text NOT NULL CHECK (event_type IN (
                                 'loop_started',
                                 'message_appended',
                                 'tool_approval_requested',
                                 'tool_approval_resolved',
                                 'tool_execution_started',
                                 'hook_invocation_pending',
                                 'hook_invocation_completed',
                                 'hook_invocation_failed',
                                 'transcript_compacted',
                                 'iteration_completed',
                                 'loop_finished',
                                 'loop_failed',
                                 'loop_cancelled'
                             )),
    event_version            integer NOT NULL CHECK (event_version > 0),
    -- variant-only event data; no nested {type, data} envelope
    payload                  jsonb NOT NULL,
    runtime_snapshot_version integer CHECK (runtime_snapshot_version > 0),
    runtime_snapshot         jsonb,
    created_at               timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (agent_id, event_seq),
    -- snapshot version and snapshot are both null or both present
    CONSTRAINT durable_events_snapshot_pair CHECK (
        (runtime_snapshot IS NULL) = (runtime_snapshot_version IS NULL)
    ),
    -- the runtime snapshot is allowed on, and required on, LoopStarted only
    CONSTRAINT durable_events_snapshot_only_loop_started CHECK (
        (event_type = 'loop_started') = (runtime_snapshot IS NOT NULL)
    ),
    -- TranscriptCompacted duplicates nothing: its companion row owns the facts
    CONSTRAINT durable_events_compacted_payload_empty CHECK (
        event_type <> 'transcript_compacted' OR payload = '{}'::jsonb
    ),
    -- approval payloads are constrained so the identity expression indexes
    -- below always index real identities
    CONSTRAINT durable_events_approval_requested_shape CHECK (
        event_type <> 'tool_approval_requested'
        OR (payload ? 'approval_id' AND payload ? 'hook_invocation_id'
            AND payload ? 'call_id' AND payload ? 'tool_name')
    ),
    CONSTRAINT durable_events_approval_resolved_shape CHECK (
        event_type <> 'tool_approval_resolved'
        OR (payload ? 'approval_id' AND payload ? 'decision')
    )
);

-- Exactly one LoopStarted per (agent_id, turn_id).
CREATE UNIQUE INDEX durable_events_one_loop_started_per_turn
    ON durable_events (agent_id, turn_id)
    WHERE event_type = 'loop_started';

-- At most one terminal event per (agent_id, turn_id).
CREATE UNIQUE INDEX durable_events_one_terminal_per_turn
    ON durable_events (agent_id, turn_id)
    WHERE event_type IN ('loop_finished', 'loop_failed', 'loop_cancelled');

-- Approval ledger: one Requested per exact hook invocation.
CREATE UNIQUE INDEX durable_events_approval_requested_unique
    ON durable_events (agent_id, (payload ->> 'hook_invocation_id'))
    WHERE event_type = 'tool_approval_requested';

-- Approval ledger: one Resolved per approval identity.
CREATE UNIQUE INDEX durable_events_approval_resolved_unique
    ON durable_events (agent_id, (payload ->> 'approval_id'))
    WHERE event_type = 'tool_approval_resolved';

-- Approval identity itself is unique on the Requested side and is also the
-- resolver/read lookup index.
CREATE UNIQUE INDEX durable_events_approval_requested_by_approval
    ON durable_events (agent_id, (payload ->> 'approval_id'))
    WHERE event_type = 'tool_approval_requested';

-- Approval Consumed derivation: matching HookInvocationCompleted lookup.
CREATE INDEX durable_events_hook_completed_by_invocation
    ON durable_events (agent_id, (payload ->> 'invocation_id'))
    WHERE event_type = 'hook_invocation_completed';

-- Per-turn ordered reads (approval ledger scans, turn inspection).
CREATE INDEX durable_events_by_turn_event_seq_idx
    ON durable_events (agent_id, turn_id, event_seq);

-- Product history reads only product-visible event types.
CREATE INDEX durable_events_history_idx
    ON durable_events (agent_id, event_seq)
    WHERE event_type IN ('message_appended', 'transcript_compacted', 'loop_failed', 'loop_cancelled');

CREATE TABLE transcript_compactions (
    agent_id                uuid NOT NULL,
    event_seq               bigint NOT NULL,
    turn_id                 uuid NOT NULL,
    compacted_iteration     bigint NOT NULL CHECK (compacted_iteration >= 0),
    upto                    bigint NOT NULL CHECK (upto > 0),
    retained_from_event_seq bigint NOT NULL CHECK (retained_from_event_seq > 0),
    summary                 jsonb NOT NULL,
    created_at              timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (agent_id, event_seq),
    CONSTRAINT transcript_compactions_event_fk
        FOREIGN KEY (agent_id, event_seq)
        REFERENCES durable_events ON DELETE RESTRICT,
    -- the retained pointer always addresses an earlier durable row
    CONSTRAINT transcript_compactions_pointer_before_event CHECK (
        retained_from_event_seq < event_seq
    )
);

-- Latest-companion lookup for one agent at or below a fixed base.
CREATE INDEX transcript_compactions_latest_idx
    ON transcript_compactions (agent_id, event_seq DESC);
