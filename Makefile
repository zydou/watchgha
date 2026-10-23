.PHONY: help clean sterile

.DEFAULT_GOAL := help

help: 	## Display this help message.
	@echo "Please use \`make <target>' where <target> is one of:"
	@awk -F ':.*?## ' '/^[a-zA-Z]/ && NF==2 {printf "  %-20s %s\n", $$1, $$2}' $(MAKEFILE_LIST) | sort

clean: 	## Remove stuff we don't need.
	rm -fr target/
	rm -f get_*.json get_index.txt

sterile: clean	## Remove absolutely everything.
	rm -fr target/

.PHONY: test fmt clippy build install

test:	## Run the tests.
	cargo test

fmt:	## Check the formatting.
	cargo fmt --check

clippy:	## Lint the code.
	cargo clippy --all-targets -- -D warnings

build:	## Build the release binary.
	cargo build --release

install: build	## Install the built binary as `watchgha`.
	install -d "$$HOME/.local/bin"
	install target/release/watchgha "$$HOME/.local/bin/watchgha"
