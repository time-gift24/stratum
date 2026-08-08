COMPOSE ?= podman compose
TEST_COMPOSE_FILE := docker-compose.test.yml

.PHONY: test-integration test-up test-down

test-integration:
	@set -eu; \
	trap '$(COMPOSE) -f $(TEST_COMPOSE_FILE) down -v' EXIT; \
	$(COMPOSE) -f $(TEST_COMPOSE_FILE) up -d --wait; \
	cargo test -p stratum-api --test api -- --ignored --test-threads=1

test-up:
	$(COMPOSE) -f $(TEST_COMPOSE_FILE) up -d

test-down:
	$(COMPOSE) -f $(TEST_COMPOSE_FILE) down -v
