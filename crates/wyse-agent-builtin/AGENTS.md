# wyse-agent-builtin

- `ModelId` is always `provider:model`; accept it only through `MODEL`.
- `API_KEY` is the only credential environment variable. Never log it.
- Binaries subscribe through `EventStreamBus` and write complete NDJSON
  `StreamEnvelope` values; do not hide reasoning or metadata.
- Keep provider dispatch concrete in `default_agent`; add a registry only when
  more than the current direct match requires one.
- `simple_agent` is intentionally no-tool and one-shot. Add tools or REPL
  behavior only in a separately approved executable.
