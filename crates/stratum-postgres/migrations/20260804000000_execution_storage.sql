-- Execution-layer storage: durable event journal, agent state, message history.
--
-- durable_events and agent_messages follow openspec change
-- add-postgres-execution-storage design §2/§3. agent_state extends the design
-- §3 sketch additively: the sketch's columns cannot round-trip a full
-- AgentState (name, definition version ids, hook handler order, model_config,
-- location, next_iteration are missing) and cannot express
-- complete_iteration's iteration-frontier precondition, so those fields are
-- materialized as columns here under the same discipline as the other
-- state columns. next_message_seq is the last allocated message sequence
-- (AgentState.last_seq); allocation increments it and returns the new value.

CREATE TABLE durable_events (
    id          bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    session_id  uuid NOT NULL,
    agent_id    uuid NOT NULL,
    turn_id     uuid NOT NULL,
    -- per-run monotone sequence, 1-based to mirror events.jsonl line numbers
    seq         bigint NOT NULL CHECK (seq > 0),
    event_type  text NOT NULL,
    payload     jsonb NOT NULL,
    created_at  timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT durable_events_turn_id_seq_unique UNIQUE (turn_id, seq)
);

CREATE INDEX durable_events_session_id_id_idx ON durable_events (session_id, id);

CREATE TABLE agent_messages (
    agent_id    uuid NOT NULL,
    message_seq bigint NOT NULL CHECK (message_seq > 0),
    session_id  uuid NOT NULL,
    turn_id     uuid NOT NULL,
    -- AgentLocation type tag ("direct" / "workflow_node"); the full location
    -- lives inside the envelope payload
    location    text NOT NULL,
    envelope    jsonb NOT NULL,
    created_at  timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (agent_id, message_seq)
);

CREATE TABLE agent_state (
    agent_id                 uuid PRIMARY KEY,
    state_version            integer NOT NULL,
    name                     text NOT NULL,
    agent_version_id         uuid NOT NULL,
    skill_set_version_id     uuid NOT NULL,
    extension_set_version_id uuid NOT NULL,
    hook_handler_versions    jsonb NOT NULL,
    model_config             jsonb,
    status                   text NOT NULL,
    session_id               uuid,
    active_turn_id           uuid,
    location                 jsonb,
    runtime_snapshot         jsonb,
    next_iteration           bigint NOT NULL DEFAULT 0,
    usage                    jsonb NOT NULL,
    next_message_seq         bigint NOT NULL DEFAULT 0,
    updated_at               timestamptz NOT NULL
);
