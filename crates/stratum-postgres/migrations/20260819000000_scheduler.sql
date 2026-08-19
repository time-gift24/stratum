-- Single-host recurring scheduler control state.
--
-- These tables are durable control-plane facts, not AgentRuntime ledger
-- projections or distributed leases. Running more than one scheduler process
-- against the same database is intentionally unsupported in this version.

CREATE TABLE schedules (
    id                uuid PRIMARY KEY,
    agent_name        text NOT NULL,
    cron_expression   text NOT NULL CHECK (length(cron_expression) > 0),
    created_at        timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX schedules_created_at_idx
    ON schedules (created_at DESC, id DESC);

CREATE TABLE schedule_runs (
    schedule_id       uuid NOT NULL REFERENCES schedules(id) ON DELETE RESTRICT,
    session_id        uuid NOT NULL,
    idempotency_key   uuid NOT NULL UNIQUE,
    agent_runtime_id  uuid REFERENCES agent_states(id) ON DELETE RESTRICT,
    agent_id          uuid REFERENCES agents(id) ON DELETE RESTRICT,
    turn_id           uuid,
    status            text NOT NULL
                      CHECK (status IN ('starting', 'accepted', 'failed')),
    triggered_at      timestamptz NOT NULL,
    updated_at        timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (schedule_id, session_id),
    CONSTRAINT schedule_runs_lifecycle_shape CHECK (
        (
            status = 'starting'
            AND agent_runtime_id IS NULL
            AND agent_id IS NULL
            AND turn_id IS NULL
        )
        OR
        (
            status = 'accepted'
            AND agent_runtime_id IS NOT NULL
            AND agent_id IS NOT NULL
            AND turn_id IS NOT NULL
        )
        OR
        (
            status = 'failed'
            AND turn_id IS NULL
            AND ((agent_runtime_id IS NULL) = (agent_id IS NULL))
        )
    )
);

CREATE INDEX schedule_runs_triggered_at_idx
    ON schedule_runs (schedule_id, triggered_at DESC, session_id DESC);

CREATE INDEX schedule_runs_starting_idx
    ON schedule_runs (triggered_at, schedule_id, session_id)
    WHERE status = 'starting';
