SHELL := /bin/sh
ENGINE := ./target/release/crownline
LAB := ./target/release/crownline-lab
SOLVE_DIR ?= evidence/local-table
PIECES ?= 4
DATA_DIR ?= evidence/local-selfplay
GAMES ?= 96
SEED ?= 101
EPOCHS ?= 8
MODEL_DIR ?= evidence/local-model
MODEL ?= $(MODEL_DIR)/model.json
ARENA_DIR ?= evidence/local-arena
PAIRS ?= 12
BUDGET ?= nodes768
VARIANT ?= blend
PROFILE ?= diagnostic
RELEASE_VERSION ?= 0.1.0-preview
HISTORICAL_SOURCES ?= evidence/historical-sources
HISTORICAL_MODELS ?= evidence/preview-final
HISTORICAL_RUN ?= evidence/historical-evaluation

.PHONY: historical-data historical bootstrap doctor build test perft data selfplay train solve arena review-demo demo bench stress docs acceptance verify release ci

bootstrap:
	cargo fetch --locked

doctor:
	python3 tools/doctor.py

build:
	cargo build --locked --release

test: build
	cargo test --locked --release
	node --test web/tests/*.test.mjs

perft: build
	$(ENGINE) perft 8

data selfplay: build
	$(LAB) selfplay "$(DATA_DIR)" "$(GAMES)"
	$(LAB) verify-data "$(DATA_DIR)/selfplay.jsonl"

train: build
	$(LAB) train "$(DATA_DIR)/selfplay.jsonl" "$(MODEL_DIR)" "$(SEED)" "$(EPOCHS)" 100
	$(LAB) verify-model "$(DATA_DIR)/selfplay.jsonl" "$(MODEL_DIR)"

solve: build
	$(ENGINE) solve "$(SOLVE_DIR)" "$(PIECES)"
	$(ENGINE) verify-table "$(SOLVE_DIR)"

arena: build
	$(LAB) arena "$(MODEL)" "$(ARENA_DIR)" "$(VARIANT)" "$(BUDGET)" test "$(PAIRS)"
	$(LAB) verify-arena "$(MODEL)" "$(ARENA_DIR)"

historical-data:
	python3 tools/acquire_historical.py "$(HISTORICAL_SOURCES)"

historical: build historical-data
	$(LAB) historical config/historical-v1.json "$(HISTORICAL_SOURCES)" "$(HISTORICAL_MODELS)/data/selfplay.jsonl" "$(HISTORICAL_MODELS)" "$(HISTORICAL_RUN)"
	$(LAB) verify-historical config/historical-v1.json "$(HISTORICAL_SOURCES)" "$(HISTORICAL_MODELS)/data/selfplay.jsonl" "$(HISTORICAL_MODELS)" "$(HISTORICAL_RUN)"

review-demo: solve
	$(ENGINE) review examples/branch-study.pdn "$(SOLVE_DIR)"
	$(ENGINE) witness "$(SOLVE_DIR)" 'B:BK2,K8:W11,K22'

demo: build
	CROWNLINE_TABLE="$(SOLVE_DIR)" node web/server.mjs

bench: build
	$(ENGINE) perft 9
	$(ENGINE) search 24 100000

stress: build
	$(LAB) tactics

docs:
	python3 tools/check_docs.py

acceptance:
	@test -n "$(RUN_ID)" || { echo 'Set RUN_ID to a new identifier' >&2; exit 2; }
	python3 tools/run_campaign.py "$(RUN_ID)" "$(PROFILE)"

verify: build
	@test -n "$(EVIDENCE)" || { echo 'Set EVIDENCE to a bundle directory' >&2; exit 2; }
	python3 tools/verify_campaign.py "$(EVIDENCE)" $(if $(EXPECTED_COMMIT),--expected "$(EXPECTED_COMMIT)",) $(if $(filter 1,$(INTEGRITY_ONLY)),--integrity-only,)

release: build docs
	python3 tools/package_release.py "$(RELEASE_VERSION)"

ci: test docs
	cargo fmt --check
	cargo clippy --locked --all-targets -- -D warnings
	python3 tools/smoke_verifier.py
	python3 tools/package_release.py "$(RELEASE_VERSION)"
	python3 tools/test_package.py dist/crownline-$(RELEASE_VERSION)-$$(uname -s | tr A-Z a-z)-$$(uname -m).tar.gz
