.PHONY: test-session-runtime test-deepseek-session

test-session-runtime:
	cargo test -p stratum-api --test session_runtime session_runtime_survives_restart_and_isolates_agent_history

test-deepseek-session:
	cargo test -p stratum-api --test session_runtime live_deepseek_session_survives_host_restart -- --ignored --nocapture
