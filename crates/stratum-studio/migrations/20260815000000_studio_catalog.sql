CREATE TABLE studio_catalog (
    singleton BOOLEAN PRIMARY KEY DEFAULT TRUE CHECK (singleton),
    revision UUID NOT NULL
);

CREATE TABLE studio_providers (
    kind TEXT PRIMARY KEY CHECK (kind IN ('openai', 'deepseek')),
    revision UUID NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE studio_provider_credentials (
    provider_kind TEXT PRIMARY KEY REFERENCES studio_providers(kind) ON DELETE RESTRICT,
    secret TEXT NOT NULL CHECK (length(secret) > 0),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE studio_models (
    provider_kind TEXT NOT NULL REFERENCES studio_providers(kind) ON DELETE RESTRICT,
    name TEXT NOT NULL CHECK (length(name) > 0),
    revision UUID NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (provider_kind, name)
);

CREATE TABLE studio_agent_definitions (
    agent_name TEXT PRIMARY KEY,
    version TEXT NOT NULL,
    model_id TEXT NOT NULL,
    model_parameters JSONB NOT NULL DEFAULT '{}'::JSONB,
    tools JSONB NOT NULL DEFAULT '[]'::JSONB,
    prompt TEXT NOT NULL CHECK (length(btrim(prompt)) > 0),
    revision UUID NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
