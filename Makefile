.DEFAULT_GOAL := help

COMPOSE ?= docker compose -f compose.yaml
# 当前 Maitu 栈（用户日常运行的那套）。它与开发栈是不同 compose 项目，容器与卷都
# 分开，所以操作它必须显式用这个变量，避免把两套栈混在一起。
MAITU_COMPOSE ?= docker compose -f compose.maitu.yaml

# 生产镜像打上构建它的源码修订，备份元数据才能证明归档源码与运行镜像同源；
# 与 scripts/start-local.ps1 同一约定（脏树加 -dirty），外部已设置时不覆盖。
FUDIAN_SOURCE_REVISION ?= $(shell r=$$(git rev-parse HEAD 2>/dev/null) && [ -n "$$r" ] && { git status --porcelain 2>/dev/null | grep -q . && echo $$r-dirty || echo $$r; })
export FUDIAN_SOURCE_REVISION

.PHONY: help dev build start check docs test fmt lint db-shell maitu-db-shell backup logs down

help:
	@echo "make dev       启动 Rust 开发服务和 PostgreSQL"
	@echo "make build     构建生产镜像"
	@echo "make start     启动生产镜像并复用现有数据卷"
	@echo "make check     运行文档、格式、Clippy 和测试"
	@echo "make docs      检查 Markdown 结构和本地链接"
	@echo "make db-shell  进入开发栈 PostgreSQL"
	@echo "make maitu-db-shell  进入当前 Maitu 栈 PostgreSQL"
	@echo "make backup    备份 Maitu 栈的数据库、四个内容卷与可构建源码"

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

docs:
	./scripts/check-docs.py

check: docs fmt lint test

db-shell:
	$(COMPOSE) exec postgres sh -ec 'psql -U "$$POSTGRES_USER" -d "$$POSTGRES_DB"'

maitu-db-shell:
	$(MAITU_COMPOSE) exec postgres sh -ec 'psql -U "$$POSTGRES_USER" -d "$$POSTGRES_DB"'

backup:
	./scripts/backup-maitu.sh

logs:
	$(COMPOSE) logs -f app app-prod postgres

down:
	$(COMPOSE) --profile production down
