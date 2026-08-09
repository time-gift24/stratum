-- Destructive beta baseline for complete-postgres-agent-runtime.
--
-- Exactly four plural tables carry immutable Agent definitions, runtime state,
-- the append-only AgentRuntime ledger, and compaction companions. There are no
-- projections, claims, sessions, outbox, or rebuild tables. Deployments must
-- recreate both the database and sqlx migration history for this cutover.

CREATE TABLE agents (
    id                          uuid PRIMARY KEY,
    name                        text NOT NULL,
    version                     text COLLATE "C" NOT NULL,
    definition_schema_version   integer NOT NULL
                                CHECK (definition_schema_version > 0),
    resolved_definition         jsonb NOT NULL
                                CHECK (jsonb_typeof(resolved_definition) = 'object'),
    created_at                  timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT agents_name_version_unique UNIQUE (name, version),
    CONSTRAINT agents_version_tag_valid CHECK (
        octet_length(version) BETWEEN 1 AND 128
        AND version !~ '[[:cntrl:]]'
        AND version !~ '^[[:space:]]'
        AND version !~ '[[:space:]]$'
    )
);

CREATE TABLE agent_states (
    id                  uuid PRIMARY KEY,
    agent_id            uuid NOT NULL REFERENCES agents(id) ON DELETE RESTRICT,
    idempotency_key     uuid NOT NULL,
    status              text NOT NULL
                        CHECK (status IN ('idle', 'running', 'finished', 'failed', 'cancelled')),
    session_id          uuid,
    current_turn_id     uuid,
    model_config        jsonb NOT NULL CHECK (jsonb_typeof(model_config) = 'object'),
    last_event_seq      bigint NOT NULL DEFAULT 0 CHECK (last_event_seq >= 0),
    created_at          timestamptz NOT NULL DEFAULT now(),
    updated_at          timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT agent_states_idempotency_key_unique UNIQUE (idempotency_key),
    CONSTRAINT agent_states_lifecycle_shape CHECK (
        (
            status = 'idle'
            AND session_id IS NULL
            AND current_turn_id IS NULL
            AND last_event_seq = 0
        )
        OR
        (
            status <> 'idle'
            AND session_id IS NOT NULL
            AND current_turn_id IS NOT NULL
            AND last_event_seq > 0
        )
    )
);

-- Current AgentRuntime-only Session single-active constraint. This is not a
-- Session owner, scheduler lease, or cross-process claim.
CREATE UNIQUE INDEX agent_states_running_session_unique
    ON agent_states (session_id)
    WHERE status = 'running';

CREATE TABLE durable_events (
    agent_runtime_id          uuid NOT NULL
                              REFERENCES agent_states(id) ON DELETE RESTRICT,
    event_seq                 bigint NOT NULL CHECK (event_seq > 0),
    session_id                uuid NOT NULL,
    turn_id                   uuid NOT NULL,
    event_type                text NOT NULL CHECK (event_type IN (
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
    event_version             integer NOT NULL CHECK (event_version > 0),
    payload                   jsonb NOT NULL CHECK (jsonb_typeof(payload) = 'object'),
    runtime_snapshot_version  integer CHECK (runtime_snapshot_version > 0),
    runtime_snapshot          jsonb,
    created_at                timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (agent_runtime_id, event_seq),
    CONSTRAINT durable_events_snapshot_pair CHECK (
        (runtime_snapshot IS NULL) = (runtime_snapshot_version IS NULL)
    ),
    CONSTRAINT durable_events_snapshot_only_loop_started CHECK (
        (event_type = 'loop_started') = (runtime_snapshot IS NOT NULL)
    ),
    CONSTRAINT durable_events_snapshot_object CHECK (
        runtime_snapshot IS NULL OR jsonb_typeof(runtime_snapshot) = 'object'
    ),
    CONSTRAINT durable_events_loop_started_agent_pin CHECK (
        event_type <> 'loop_started' OR runtime_snapshot ? 'agent_id'
    ),
    CONSTRAINT durable_events_compacted_payload_empty CHECK (
        event_type <> 'transcript_compacted' OR payload = '{}'::jsonb
    ),
    CONSTRAINT durable_events_approval_requested_shape CHECK (
        event_type <> 'tool_approval_requested'
        OR (
            payload ? 'approval_id'
            AND payload ? 'hook_invocation_id'
            AND payload ? 'call_id'
            AND payload ? 'tool_name'
            AND (payload ->> 'approval_id')::uuid IS NOT NULL
            AND (payload ->> 'hook_invocation_id')::uuid IS NOT NULL
        )
    ),
    CONSTRAINT durable_events_approval_resolved_shape CHECK (
        event_type <> 'tool_approval_resolved'
        OR (
            payload ? 'approval_id'
            AND payload ? 'decision'
            AND (payload ->> 'approval_id')::uuid IS NOT NULL
            AND payload ->> 'decision' IN ('approve', 'reject')
        )
    )
);

CREATE UNIQUE INDEX durable_events_one_loop_started_per_turn
    ON durable_events (agent_runtime_id, turn_id)
    WHERE event_type = 'loop_started';

CREATE UNIQUE INDEX durable_events_one_terminal_per_turn
    ON durable_events (agent_runtime_id, turn_id)
    WHERE event_type IN ('loop_finished', 'loop_failed', 'loop_cancelled');

CREATE UNIQUE INDEX durable_events_approval_requested_unique
    ON durable_events (agent_runtime_id, ((payload ->> 'hook_invocation_id')::uuid))
    WHERE event_type = 'tool_approval_requested';

CREATE UNIQUE INDEX durable_events_approval_requested_by_approval
    ON durable_events (agent_runtime_id, ((payload ->> 'approval_id')::uuid))
    WHERE event_type = 'tool_approval_requested';

CREATE UNIQUE INDEX durable_events_approval_resolved_unique
    ON durable_events (agent_runtime_id, ((payload ->> 'approval_id')::uuid))
    WHERE event_type = 'tool_approval_resolved';

CREATE INDEX durable_events_hook_completed_by_invocation
    ON durable_events (agent_runtime_id, ((payload ->> 'invocation_id')::uuid))
    WHERE event_type = 'hook_invocation_completed';

CREATE INDEX durable_events_by_turn_event_seq_idx
    ON durable_events (agent_runtime_id, turn_id, event_seq);

CREATE INDEX durable_events_history_idx
    ON durable_events (agent_runtime_id, event_seq)
    WHERE event_type IN (
        'loop_started',
        'message_appended',
        'tool_approval_requested',
        'tool_approval_resolved',
        'transcript_compacted',
        'iteration_completed',
        'loop_finished',
        'loop_failed',
        'loop_cancelled'
    );

CREATE INDEX durable_events_assistant_floor_scan_idx
    ON durable_events (agent_runtime_id, event_seq DESC)
    WHERE event_type = 'message_appended';

CREATE INDEX durable_events_usage_scan_idx
    ON durable_events (agent_runtime_id, turn_id, event_seq DESC)
    WHERE event_type IN ('iteration_completed', 'loop_finished', 'loop_failed', 'loop_cancelled');

CREATE TABLE transcript_compactions (
    agent_runtime_id          uuid NOT NULL,
    event_seq                 bigint NOT NULL,
    turn_id                   uuid NOT NULL,
    compacted_iteration       bigint NOT NULL CHECK (compacted_iteration >= 0),
    upto                      bigint NOT NULL CHECK (upto > 0),
    retained_from_event_seq   bigint NOT NULL CHECK (retained_from_event_seq > 0),
    summary                   jsonb NOT NULL CHECK (jsonb_typeof(summary) = 'object'),
    created_at                timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (agent_runtime_id, event_seq),
    CONSTRAINT transcript_compactions_event_fk
        FOREIGN KEY (agent_runtime_id, event_seq)
        REFERENCES durable_events(agent_runtime_id, event_seq) ON DELETE RESTRICT,
    CONSTRAINT transcript_compactions_pointer_before_event CHECK (
        retained_from_event_seq < event_seq
    )
);

CREATE INDEX transcript_compactions_latest_idx
    ON transcript_compactions (agent_runtime_id, event_seq DESC);
