# ============================================================================
# Fluxor 构建 Makefile
# ----------------------------------------------------------------------------
# 目录结构:
#   frontend/  Vue 3 + Vite + TypeScript 前端源码
#   backend/   Go 后端源码（以 //go:embed dist 内嵌前端构建产物）
#
# 构建链路（make 会按依赖自动补齐，无需分步执行）:
#   deps ────────> frontend ────> sync ────> backend
#   npm install    npm run build   cp -r       go build
#
# 常用命令（make help 可查看全部）:
#   make           完整构建：前端 + 同步 + 后端，输出 ./fluxor
#   make frontend  仅构建前端 → frontend/dist
#   make sync      仅同步 frontend/dist → backend/dist
#   make backend   同步并编译后端二进制 → ./fluxor
#   make run       构建后以前台方式运行后端
#   make dev       启动前端热更新开发服务器
#   make check     格式校验 + go vet + 编译（CI 友好）
#   make clean     清理所有构建产物
#
# 环境变量:
#   GOOS / GOARCH  Go 交叉编译目标（留空则跟随本机）
#   GOFLAGS        Go 构建附加参数（默认 -ldflags="-s -w"）
#   BIN            输出二进制名称与路径（默认 ./fluxor）
#   NPM_INSTALL    前端依赖安装命令（默认 npm install；CI 可设为 "npm ci"）
#
# 增量构建说明:
#   各阶段通过内嵌在产物目录中的戳记文件（.fluxor-*-stamp）判断是否需要重跑：
#     frontend/node_modules/.fluxor-install-stamp  由 package.json / lock 决定
#     frontend/dist/.fluxor-build-stamp            由 frontend/src 源文件决定
#     backend/dist/.fluxor-sync-stamp              由 frontend/dist 决定
#   戳记位于产物目录内部，因此删除该目录（或执行 make clean）即会自动触发重建。
#   前置依赖中包含 frontend/src 目录本身，故「删除源文件」同样能触发重建。
# ============================================================================

SHELL := /bin/bash

# ---------- 可覆盖变量 ----------
FRONTEND_DIR ?= frontend
BACKEND_DIR  ?= backend
BIN          ?= fluxor
GO           ?= go
NPM          ?= npm
# 默认使用 npm install：它不会先删除 node_modules，因此在缓存/网络不可用时
# 不会破坏已有的开发环境。CI 等需要严格复现 lockfile 的场景可覆盖为 npm ci：
#     make deps NPM_INSTALL="npm ci"
NPM_INSTALL  ?= $(NPM) install
GOFLAGS      ?= -ldflags="-s -w"

# ---------- 派生路径 ----------
FRONTEND_DIST := $(FRONTEND_DIR)/dist
BACKEND_DIST  := $(BACKEND_DIR)/dist
FRONTEND_PKG  := $(FRONTEND_DIR)/package.json
FRONTEND_LOCK := $(FRONTEND_DIR)/package-lock.json

# ---------- 增量戳记 ----------
NPM_STAMP := $(FRONTEND_DIR)/node_modules/.fluxor-install-stamp
FE_STAMP  := $(FRONTEND_DIST)/.fluxor-build-stamp
BE_STAMP  := $(BACKEND_DIST)/.fluxor-sync-stamp
# 交叉编译目标跟踪：.fluxor.target 记录上次编译所用的 GOOS/GOARCH，
# 仅在目标发生变化时更新其时间戳，从而既能在切换架构时强制重编译
# （避免复用旧架构二进制），又能在目标未变时保持真正的 no-op。
TARGET_ID     := $(if $(GOOS),$(GOOS),$(shell $(GO) env GOOS))_$(if $(GOARCH),$(GOARCH),$(shell $(GO) env GOARCH))
TARGET_FILE   := $(CURDIR)/.fluxor.target
TARGET_STAMP  := $(CURDIR)/.fluxor.target.stamp

# ---------- 源文件集合 ----------
# 目录本身也作为前置依赖，使得删除源文件同样能触发重建
FRONTEND_SOURCES := $(wildcard $(FRONTEND_DIR)/src) \
                    $(shell find $(FRONTEND_DIR)/src -type f 2>/dev/null) \
                    $(wildcard $(FRONTEND_DIR)/index.html) \
                    $(wildcard $(FRONTEND_DIR)/vite.config.*) \
                    $(wildcard $(FRONTEND_DIR)/tsconfig*.json) \
                    $(FRONTEND_PKG)

BACKEND_SOURCES := $(shell find $(BACKEND_DIR) -name '*.go' 2>/dev/null) \
                   $(BACKEND_DIR)/go.mod $(BACKEND_DIR)/go.sum

.DEFAULT_GOAL := all
.PHONY: all build deps frontend sync backend run dev fmt vet check clean help FORCE

# ============================ 顶层入口 ============================

all: build

build: backend

# ============================ 分阶段目标 ============================

# 安装前端依赖
deps: $(NPM_STAMP)

# 构建前端产物
frontend: $(FE_STAMP)

# 同步前端产物到 backend/dist 供 //go:embed 内嵌
sync: $(BE_STAMP)

# 编译后端二进制
backend: $(BIN)

# ============================ 实际规则 ============================

$(NPM_STAMP): $(FRONTEND_PKG) $(FRONTEND_LOCK)
	@echo "==> [1/3] 安装前端依赖"
	@cd $(FRONTEND_DIR) && $(NPM_INSTALL) --no-audit --no-fund
	@touch $@

$(FE_STAMP): $(NPM_STAMP) $(FRONTEND_SOURCES)
	@echo "==> [2/3] 构建前端 → $(FRONTEND_DIST)"
	@cd $(FRONTEND_DIR) && $(NPM) run build
	@touch $@

$(BE_STAMP): $(FE_STAMP)
	@echo "==> [3/3] 同步 $(FRONTEND_DIST) → $(BACKEND_DIST)"
	@rm -rf $(BACKEND_DIST)
	@mkdir -p $(BACKEND_DIR)
	@cp -r $(FRONTEND_DIST) $(BACKEND_DIST)
	@touch $@

$(BIN): $(BE_STAMP) $(BACKEND_SOURCES) $(TARGET_STAMP)
	@echo "==> 编译后端 → ./$(BIN)  [$(TARGET_ID)]"
	@cd $(BACKEND_DIR) && CGO_ENABLED=0 GOOS=$(GOOS) GOARCH=$(GOARCH) \
		$(GO) build $(GOFLAGS) -buildvcs=false -o $(CURDIR)/$(BIN)
	@echo "==> 构建完成: $(CURDIR)/$(BIN)"

# 每次 make 都会执行，但仅在目标平台变化时才 touch 时间戳；
# 时间戳变新会使 $(BIN) 成为过期目标而重新编译。
$(TARGET_STAMP): FORCE
	@if [ "$$(cat $(TARGET_FILE) 2>/dev/null)" != "$(TARGET_ID)" ]; then \
		echo "==> 交叉编译目标变更，重新编译: $(TARGET_ID)"; \
		echo "$(TARGET_ID)" > $(TARGET_FILE); \
		rm -f $(CURDIR)/$(BIN); \
		touch $@; \
	fi

# 无条件执行用的辅助目标
FORCE:

# ============================ 开发与检查 ============================

# 前台运行后端（自动确保 dist 已同步）
run: $(BE_STAMP)
	@cd $(BACKEND_DIR) && $(GO) run .

# 启动前端热更新开发服务器
dev: $(NPM_STAMP)
	@cd $(FRONTEND_DIR) && $(NPM) run dev

# 格式化后端 Go 代码
fmt:
	@cd $(BACKEND_DIR) && gofmt -w .

# 静态检查
vet:
	@cd $(BACKEND_DIR) && $(GO) vet ./...

# CI 检查：格式 + vet + 编译
check: $(BE_STAMP)
	@echo "==> gofmt"
	@cd $(BACKEND_DIR) && out=$$(gofmt -l .); \
		if [ -n "$$out" ]; then echo "以下文件未格式化:"; echo "$$out"; exit 1; fi
	@echo "==> go vet"
	@cd $(BACKEND_DIR) && $(GO) vet ./...
	@echo "==> go build"
	@cd $(BACKEND_DIR) && $(GO) build -o /dev/null .
	@echo "==> 检查通过"

# ============================ 清理 ============================

clean:
	@echo "==> 清理构建产物"
	@rm -rf $(FRONTEND_DIST)
	@rm -rf $(BACKEND_DIST)
	@rm -f $(CURDIR)/$(BIN)
	@rm -f $(CURDIR)/.fluxor.target $(CURDIR)/.fluxor.target.stamp
	@rm -f $(CURDIR)/fluxor-*
	@echo "==> 已清理: $(FRONTEND_DIST) $(BACKEND_DIST) ./$(BIN)"

# ============================ 帮助 ============================

help:
	@echo "Fluxor 构建系统"
	@echo ""
	@echo "  make            完整构建（前端 → 同步 → 后端），输出 ./$(BIN)"
	@echo "  make frontend   仅构建前端 → $(FRONTEND_DIST)"
	@echo "  make sync       仅同步 $(FRONTEND_DIST) → $(BACKEND_DIST)"
	@echo "  make backend    同步并编译后端 → ./$(BIN)"
	@echo "  make deps       仅安装前端依赖"
	@echo "  make run        构建后前台运行后端"
	@echo "  make dev        启动前端热更新开发服务器"
	@echo "  make fmt        格式化后端 Go 代码"
	@echo "  make vet        后端静态检查"
	@echo "  make check      格式校验 + vet + 编译"
	@echo "  make clean      清理所有构建产物"
	@echo "  make help       显示本帮助"
	@echo ""
	@echo "可用变量: GOOS GOARCH GOFLAGS BIN=$(BIN) GO=$(GO) NPM=$(NPM)"
	@echo "          NPM_INSTALL=\"$(NPM_INSTALL)\"（CI 可设为 npm ci）"
