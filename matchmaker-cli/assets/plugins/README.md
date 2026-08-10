# Matchmaker preset skill

[`SKILL.md`](./SKILL.md) teaches coding agents how to create, review, port, and test [Matchmaker](https://github.com/Squirreljetpack/matchmaker) presets.

It follows the [Agent Skills](https://agentskills.io/specification) format and can be installed in Codex, pi, OpenCode, or Claude Code. The same file works in all four hosts; only the discovery directory differs.

## Install with curl

The commands below download the reviewed file without executing remote content. Choose one or more destinations:

```sh
URL='https://raw.githubusercontent.com/Squirreljetpack/matchmaker/main/assets/plugins/SKILL.md'

# OpenAI Codex
mkdir -p "$HOME/.agents/skills/matchmaker-presets"
curl -fsSL "$URL" -o "$HOME/.agents/skills/matchmaker-presets/SKILL.md"

# pi coding agent
mkdir -p "$HOME/.pi/agent/skills/matchmaker-presets"
curl -fsSL "$URL" -o "$HOME/.pi/agent/skills/matchmaker-presets/SKILL.md"

# OpenCode
mkdir -p "$HOME/.config/opencode/skills/matchmaker-presets"
curl -fsSL "$URL" -o "$HOME/.config/opencode/skills/matchmaker-presets/SKILL.md"

# Claude Code
mkdir -p "$HOME/.claude/skills/matchmaker-presets"
curl -fsSL "$URL" -o "$HOME/.claude/skills/matchmaker-presets/SKILL.md"
```

Restart the host, or reload its skills, after installing or updating the file. To update an existing installation, run the corresponding `curl` command again; it overwrites only this skill's `SKILL.md`.

### Project-local installation

For a skill that should travel with one project instead of being globally available, replace the home-directory destinations with:

```text
.agents/skills/matchmaker-presets/SKILL.md
.pi/skills/matchmaker-presets/SKILL.md
.opencode/skills/matchmaker-presets/SKILL.md
.claude/skills/matchmaker-presets/SKILL.md
```

For example:

```sh
mkdir -p .pi/skills/matchmaker-presets
curl -fsSL 'https://raw.githubusercontent.com/Squirreljetpack/matchmaker/main/assets/plugins/SKILL.md' \
  -o .pi/skills/matchmaker-presets/SKILL.md
```

Review remote instructions before installing them in an environment where the agent can modify files or run commands.

## What it covers

- Preset structure, inheritance, and layering
- `start`, columns, matching, UI, preview, and bind sections
- Matchmaker templates and shell quoting
- Python `-c` presets, `shlex.split`, raw-string injection, and TOML literal-string pitfalls
- `MM_OVERRIDE` and preset-local helper scripts
- Safe file updates, stdout/stderr separation, portability, and idempotency
- Non-interactive checks with `mm --doc`, `mm --dump-config`, and `mm --list`

## References

- [Matchmaker repository](https://github.com/Squirreljetpack/matchmaker)
- [Preset collection](https://github.com/Squirreljetpack/matchmaker/tree/main/matchmaker-cli/assets/presets)
- [Matchmaker options](https://github.com/Squirreljetpack/matchmaker/blob/main/matchmaker-cli/assets/docs/options.md)
- [Matchmaker binds](https://github.com/Squirreljetpack/matchmaker/blob/main/matchmaker-cli/assets/docs/binds.md)
- [Matchmaker templates](https://github.com/Squirreljetpack/matchmaker/blob/main/matchmaker-cli/assets/docs/template.md)
- [Codex skills](https://developers.openai.com/codex/build-skills)
- [pi skills](https://github.com/badlogic/pi-mono/blob/main/packages/coding-agent/docs/skills.md)
- [OpenCode skills](https://opencode.ai/docs/skills)
- [Claude Code skills](https://code.claude.com/docs/en/skills)
