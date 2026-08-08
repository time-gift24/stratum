COMPOSE ?= podman compose
TEST_COMPOSE_FILE := docker-compose.test.yml

.PHONY: test-integration test-up test-down

test-integration:
	@set -eu; \
	trap '$(COMPOSE) -f $(TEST_COMPOSE_FILE) down -v' EXIT; \
	$(COMPOSE) -f $(TEST_COMPOSE_FILE) up -d --wait; \
	cargo test -p stratum-infra --test agent_tail_nats agent_tail_publish_subscribe_preserves_order -- --ignored --exact; \
	cargo test -p stratum-infra --test agent_tail_nats agent_tail_no_cursor_subscription_receives_only_new_frames -- --ignored --exact; \
	cargo test -p stratum-infra --test agent_tail_nats agent_tail_cursor_subscription_resumes_after_cursor -- --ignored --exact; \
	cargo test -p stratum-infra --test agent_tail_nats agent_tail_reports_expired_cursor_after_retention_eviction -- --ignored --exact; \
	cargo test -p stratum-infra --test agent_tail_nats agent_tail_isolates_frames_per_agent -- --ignored --exact; \
	cargo test -p stratum-infra --test agent_tail_nats agent_tail_seed_frames_before_restart -- --ignored --exact; \
	$(COMPOSE) -f $(TEST_COMPOSE_FILE) restart nats-stream; \
	cargo test -p stratum-infra --test agent_tail_nats agent_tail_restart_does_not_redeliver_history_to_new_subscription -- --ignored --exact

test-up:
	$(COMPOSE) -f $(TEST_COMPOSE_FILE) up -d

test-down:
	$(COMPOSE) -f $(TEST_COMPOSE_FILE) down -v
