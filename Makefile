# BenchVault — Tauri 2 (Rust backend + Vite/React frontend)
# Run all targets from the repo root.

.DEFAULT_GOAL := help
.PHONY: help install dev build frontend frontend-build \
        check fmt clippy test bindings clean \
        install-host clean-install-host reinstall uninstall \
        publish publish-only publish-link

DEB := $(wildcard src-tauri/target/release/bundle/deb/*.deb)

help: ## Show this help
	@grep -E '^[a-zA-Z_-]+:.*?## .*$$' $(MAKEFILE_LIST) \
		| awk 'BEGIN {FS = ":.*?## "}; {printf "  \033[36m%-16s\033[0m %s\n", $$1, $$2}'

## --- App (Tauri orchestrates both halves) ---

install: ## Install JS dependencies
	npm install

dev: ## Run the full app (Vite + Rust, hot reload on both sides)
	npm run tauri dev

build: ## Production bundles (.deb / .rpm / .AppImage on Linux)
	npm run tauri build

## --- Install on this host (Ubuntu/Debian — uses the .deb bundle) ---

install-host: build ## Build the .deb and install/upgrade it on this host
	$(eval DEB := $(abspath $(wildcard src-tauri/target/release/bundle/deb/*.deb)))
	@test -n "$(DEB)" || { echo "No .deb found under src-tauri/target/release/bundle/deb/"; exit 1; }
	@echo "Installing $(DEB)"
	sudo apt install -y --reinstall "$(DEB)"

clean-install-host: ## Full clean rebuild from scratch, then reinstall on this host
	$(MAKE) clean
	$(MAKE) build
	-sudo apt remove -y bench-vault
	$(eval DEB := $(abspath $(wildcard src-tauri/target/release/bundle/deb/*.deb)))
	@test -n "$(DEB)" || { echo "No .deb found under src-tauri/target/release/bundle/deb/"; exit 1; }
	@echo "Installing $(DEB)"
	sudo apt install -y "$(DEB)"

reinstall: install-host ## Alias for install-host (apt reinstalls/upgrades in place)

uninstall: ## Remove BenchVault from this host
	sudo apt remove -y bench-vault

## --- Publish to the Codeberg package registry (needs CODEBERG_TOKEN) ---

publish: ## Build the bundles, upload them to Codeberg packages and link them to the repo
	scripts/publish-to-codeberg.sh

publish-only: ## Upload the already-built bundles to Codeberg and link them
	scripts/publish-to-codeberg.sh --no-build

publish-link: ## Only (re)link the already-uploaded packages to the repo
	scripts/publish-to-codeberg.sh --link-only

## --- Frontend only (most invoke() calls fail without the Tauri shell) ---

frontend: ## Frontend-only Vite dev server
	npm run dev

frontend-build: ## tsc -b && vite build (frontend only)
	npm run build

## --- Rust backend (src-tauri/) ---

check: ## cargo check
	cd src-tauri && cargo check

fmt: ## cargo fmt
	cd src-tauri && cargo fmt

clippy: ## cargo clippy
	cd src-tauri && cargo clippy

test: ## cargo test (pass NAME=substr to filter)
	cd src-tauri && cargo test $(NAME)

bindings: ## Regenerate src/lib/bindings.ts from Rust types/commands
	cd src-tauri && cargo run --bin export_bindings

## --- Housekeeping ---

clean: ## Remove build artifacts
	cd src-tauri && cargo clean
	rm -rf dist node_modules/.vite
