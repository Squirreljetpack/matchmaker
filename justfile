default:
	@just --list

build:
	cargo build --workspace

# Run the CLI locally; pass extra args after `--`, e.g. `just preview -- --help`
run *args:
	cargo run -p matchmaker-cli -F experimental -- {{args}}

alias dev := devcontainer

# Start the Dev Container (supports Rust 'dev' CLI or npm 'devcontainer' CLI)
devcontainer:
	@if command -v dev >/dev/null; then \
		dev up; \
	elif command -v devcontainer >/dev/null; then \
		devcontainer up --workspace-folder .; \
	fi

# Open a shell inside the running Dev Container
devcontainer-shell:
	@if command -v dev >/dev/null; then \
		dev shell; \
	elif command -v devcontainer >/dev/null; then \
		devcontainer exec --workspace-folder . bash; \
	fi

