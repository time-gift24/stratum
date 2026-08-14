COMPOSE ?= podman compose
TEST_COMPOSE_FILE := docker-compose.test.yml
DATABASE_URL ?= postgres://stratum_ontology:stratum_ontology@127.0.0.1:54329/stratum_ontology_test

.PHONY: test-integration test-up test-down

test-integration:
	@set -eu; \
	trap '$(COMPOSE) -f $(TEST_COMPOSE_FILE) down -v || true' EXIT; \
	$(COMPOSE) -f $(TEST_COMPOSE_FILE) up -d --wait; \
	DATABASE_URL='$(DATABASE_URL)' cargo test -p stratum-ontology --tests -- --ignored --nocapture

test-up:
	$(COMPOSE) -f $(TEST_COMPOSE_FILE) up -d --wait

test-down:
	$(COMPOSE) -f $(TEST_COMPOSE_FILE) down -v
