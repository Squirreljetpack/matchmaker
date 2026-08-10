#!/usr/bin/env python3
"""
Apply the enabled extension definitions to pi's settings.

Replaces the `packages` array in the target settings file with the enabled
(uncommented) definitions from the extension list (MMX), keeping the rest
of the file intact. Trailing commas from manual edits are tolerated.

Usage: pi_extensions_set.py global|local [MMX]
  global  -> write $HOME/.pi/agent/settings.json
  local   -> write $PWD/.pi/settings.json
  MMX     defaults to $MM_EXTENSIONS_FILE, else ~/.pi/agent/mm_extensions
"""

import json
import os
import re
import sys
import tempfile
from json import JSONDecodeError
from pathlib import Path

MMX_DEFAULT = Path("~/.pi/agent/mm_extensions").expanduser()
GLOBAL = Path("~/.pi/agent/settings.json").expanduser()
LOCAL = Path(".pi/settings.json")

# Tolerate trailing commas from manual edits (jsonc-ish) before parsing.
_TRAILING_COMMA = re.compile(r",\s*([}\]])")


def parse_json(path):
    """Parse a JSON file, tolerating trailing commas. None when unparseable."""
    try:
        text = Path(path).read_text(encoding="utf-8")
    except OSError:
        return None
    try:
        return json.loads(text)
    except JSONDecodeError:
        try:
            return json.loads(_TRAILING_COMMA.sub(r"\1", text))
        except JSONDecodeError:
            return None


def enabled_definitions(mmx):
    """The enabled (uncommented) MMX lines as a JSON array of definitions."""
    lines = Path(mmx).read_text(encoding="utf-8").splitlines()
    lines = [line for line in lines if not re.match(r"^\s*#", line) and line.strip()]
    if lines:
        # `sed -E '$ s/,[[:space:]]*$//'`: strip a trailing comma on the last line
        lines[-1] = re.sub(r",\s*$", "", lines[-1])
    text = "[\n" + "\n".join(lines) + "\n]"
    try:
        return json.loads(text)
    except JSONDecodeError:
        return None


def create_local_settings(path):
    """Create an empty project-local pi settings file."""
    try:
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text('{"packages": []}\n', encoding="utf-8")
    except OSError as error:
        sys.stderr.write(f"error: failed to create pi settings {path}: {error}\n")
        return False
    sys.stderr.write(f"created pi settings: {path}\n")
    return True


def main():
    args = sys.argv[1:]
    target = args[0] if args else ""
    if target == "global":
        settings_path = GLOBAL
    elif target == "local":
        settings_path = LOCAL
    else:
        sys.stderr.write(f"usage: {sys.argv[0]} global|local [MMX]\n")
        sys.exit(2)
    mmx = Path(args[1]) if len(args) > 1 else Path(os.environ.get("MM_EXTENSIONS_FILE", MMX_DEFAULT))

    if not mmx.is_file():
        sys.stderr.write(f"error: mm_extensions not found: {mmx}\n")
        sys.exit(1)
    if not settings_path.is_file():
        if target == "local":
            sys.stderr.write(f"pi settings not found: {settings_path}\n")
            sys.stderr.write("create it? [y/N] ")
            sys.stderr.flush()
            answer = sys.stdin.readline().strip().lower()
            if answer not in {"y", "yes"}:
                sys.stderr.write(f"not creating pi settings: {settings_path}\n")
                sys.exit(1)
            if not create_local_settings(settings_path):
                sys.exit(1)
        else:
            sys.stderr.write(f"error: pi settings not found: {settings_path}\n")
            sys.exit(1)

    enabled = enabled_definitions(mmx)
    if enabled is None:
        sys.stderr.write("error: enabled extensions do not form valid JSON\n")
        sys.exit(1)

    settings = parse_json(settings_path)
    if settings is None:
        sys.stderr.write(f"error: failed to update {settings_path}\n")
        sys.exit(1)

    settings["packages"] = enabled
    settings_path = settings_path.resolve()
    try:
        fd, tmp = tempfile.mkstemp(prefix="mm_settings.", suffix=".tmp", dir=str(settings_path.parent))
        with os.fdopen(fd, "w", encoding="utf-8") as f:
            json.dump(settings, f, ensure_ascii=False, indent=2)
            f.write("\n")
        Path(tmp).replace(settings_path)
    except OSError as e:
        sys.stderr.write(f"error: failed to update {settings_path}: {e}\n")
        sys.exit(1)

    sys.stdout.write(f"wrote {len(enabled)} extension(s) to {settings_path}\n")


if __name__ == "__main__":
    main()
