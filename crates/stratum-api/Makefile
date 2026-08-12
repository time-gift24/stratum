COMPOSE ?= podman compose
TEST_COMPOSE_FILE := docker-compose.test.yml

.PHONY: test-integration test-up test-down

test-integration:
	@set -eu; \
	STRATUM_API_TEST_PG_HOST_PORT=; \
	STRATUM_API_TEST_NATS_HOST_PORT=; \
	export STRATUM_API_TEST_PG_HOST_PORT STRATUM_API_TEST_NATS_HOST_PORT; \
	trap '$(COMPOSE) -f $(TEST_COMPOSE_FILE) down -v' EXIT; \
	$(COMPOSE) -f $(TEST_COMPOSE_FILE) up -d --wait; \
	pg_binding=$$($(COMPOSE) -f $(TEST_COMPOSE_FILE) port postgres 5432); \
	nats_binding=$$($(COMPOSE) -f $(TEST_COMPOSE_FILE) port nats 4222); \
	pg_port=$${pg_binding##*:}; \
	nats_port=$${nats_binding##*:}; \
	STRATUM_API_TEST_PG_URL="postgres://stratum:stratum@127.0.0.1:$${pg_port}/stratum_test" \
	STRATUM_API_TEST_NATS_URL="nats://127.0.0.1:$${nats_port}" \
	cargo test -p stratum-api --test api -- --ignored --test-threads=1

test-up:
	$(COMPOSE) -f $(TEST_COMPOSE_FILE) up -d

test-down:
	$(COMPOSE) -f $(TEST_COMPOSE_FILE) down -v
