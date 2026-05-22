default:
	@just --list

build:
	cargo build --workspace

# Run the CLI locally; pass extra args after `--`, e.g. `just preview -- --help`
preview *args:
	cargo run -p matchmaker-cli -F experimental -- {{args}}
