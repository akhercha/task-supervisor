lint:
	cargo fmt -- --check
	cargo clippy --locked --all-targets -- -D warnings

test:
	cargo test --locked
