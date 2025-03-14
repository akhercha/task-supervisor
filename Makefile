format:
	cargo fmt -- --check
	cargo clippy --locked --all-targets --all-features -- -D warnings --no-deps
	cargo clippy --tests --no-deps -- -D warnings

test:
	cargo nextest run
