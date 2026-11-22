.DEFAULT_GOAL := help
.PHONY: help build build-tools build-artalde test test-tools test-artalde lint lint-tools lint-artalde fmt check check-tools check-artalde e2e install
help:
	@echo 'build, test, lint, check: both Rust workspaces'
	@echo 'build-tools, test-tools, lint-tools, check-tools: graph/watcher/viewer'
	@echo 'build-artalde, test-artalde, lint-artalde, check-artalde: agent UI'
	@echo 'fmt: format both workspaces; e2e: watcher chains + live GitHub reads'
	@echo 'install: build and install (INSTALL_ARGS="--wiki DIR --no-timer")'
build: build-tools build-artalde
build-tools:
	cargo build --manifest-path tools/Cargo.toml --release
build-artalde:
	cargo build --manifest-path artalde/Cargo.toml --workspace --release
test: test-tools test-artalde
test-tools:
	cargo test --manifest-path tools/Cargo.toml --workspace --all-features
test-artalde:
	cargo test --manifest-path artalde/Cargo.toml --workspace
lint: lint-tools lint-artalde
lint-tools:
	cargo clippy --manifest-path tools/Cargo.toml --workspace --all-targets --all-features -- -D warnings
lint-artalde:
	cargo fmt --manifest-path artalde/Cargo.toml --all --check
	cargo clippy --manifest-path artalde/Cargo.toml --workspace --all-targets -- -D warnings
fmt:
	cargo fmt --manifest-path tools/Cargo.toml --all
	cargo fmt --manifest-path artalde/Cargo.toml --all
check: check-tools check-artalde
check-tools: lint-tools test-tools
	cargo machete tools
check-artalde: lint-artalde test-artalde
e2e:
	bash tools/e2e-zaindu.sh
install:
	./install.sh $(INSTALL_ARGS)
