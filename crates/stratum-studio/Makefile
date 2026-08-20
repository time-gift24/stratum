COMPOSE ?= podman compose
TEST_COMPOSE_FILE := docker-compose.test.yml

.PHONY: test-integration test-up test-down

test-integration:
	@set -eu; \
	STRATUM_STUDIO_TEST_PG_HOST_PORT=; \
	export STRATUM_STUDIO_TEST_PG_HOST_PORT; \
	trap '$(COMPOSE) -f $(TEST_COMPOSE_FILE) down -v' EXIT; \
	$(COMPOSE) -f $(TEST_COMPOSE_FILE) up -d --wait; \
	pg_binding=$$($(COMPOSE) -f $(TEST_COMPOSE_FILE) port postgres 5432); \
	pg_port=$${pg_binding##*:}; \
	STRATUM_STUDIO_TEST_DATABASE_URL="postgres://stratum_studio:stratum_studio@127.0.0.1:$${pg_port}/stratum_studio_test" \
	cargo test -p stratum-studio --test provider_deletion -- --ignored --test-threads=1

test-up:
	$(COMPOSE) -f $(TEST_COMPOSE_FILE) up -d --wait

test-down:
	$(COMPOSE) -f $(TEST_COMPOSE_FILE) down -v
