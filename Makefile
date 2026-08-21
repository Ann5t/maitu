.DEFAULT_GOAL := help

COMPOSE ?= docker compose -f compose.yaml

.PHONY: help dev build start check test fmt lint db-shell backup logs down

help:
	@echo "make dev       启动 Rust 开发服务和 PostgreSQL"
	@echo "make build     构建生产镜像"
	@echo "make start     启动生产镜像并复用现有数据卷"
	@echo "make check     运行格式、Clippy 和测试"
	@echo "make db-shell  进入 PostgreSQL"
	@echo "make backup    备份数据库、产物和源码"

dev:
	$(COMPOSE) up --build app

build:
	$(COMPOSE) --profile production build app-prod

start:
	$(COMPOSE) --profile production up --build -d app-prod postgres

fmt:
	$(COMPOSE) run --rm --no-deps app cargo fmt --all -- --check

lint:
	$(COMPOSE) run --rm --no-deps app cargo clippy --all-targets --all-features -- -D warnings

test:
	$(COMPOSE) run --rm --no-deps app cargo test --all-targets

check: fmt lint test

db-shell:
	$(COMPOSE) exec postgres psql -U fudian_nextgen -d fudian_nextgen

backup:
	./scripts/backup.sh

logs:
	$(COMPOSE) logs -f app app-prod postgres

down:
	$(COMPOSE) --profile production down
