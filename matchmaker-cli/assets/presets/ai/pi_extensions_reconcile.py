#!/usr/bin/env python3
"""
Reconcile the extension list (MMX) with pi's settings.

Definitions present in pi's global (~/.pi/agent/settings.json) or project
(.pi/settings.json) settings are enabled in MMX (appended when missing from
the file); definitions not defined anywhere are commented out. If MMX does
not exist yet it is created from the settings (empty when there are none).

Every definition is stored in its canonical form, so the several spellings pi
accepts for one package collapse to a single MMX line.

Usage: pi_extensions_reconcile.py [MMX]
  MMX defaults to $MM_EXTENSIONS_FILE, else ~/.pi/agent/mm_extensions
"""

import json
import os
import re
import sys
import tempfile
from json import JSONDecodeError
from pathlib import Path

from pi_extensions_common import (  # pyright: ignore[reportMissingImports]
    canonical_source,
    canonicalize_definition,
    canonicalize_lines,
    parse_definition,
    set_enabled,
)

MMX_DEFAULT = Path("~/.pi/agent/mm_extensions").expanduser()
GLOBAL = Path("~/.pi/agent/settings.json").expanduser()
LOCAL = Path(".pi/settings.json")

# Tolerate trailing commas from manual edits (jsonc-ish) before parsing.
_TRAILING_COMMA = re.compile(r",\s*([}\]])")


def load_json(path):
    """
    Parse a settings file, tolerating trailing commas. Returns None when
    the file is missing or unparseable.
    """
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


def compact_json(value):
    return json.dumps(value, ensure_ascii=False, separators=(",", ":"))


def entry_source(entry):
    """
    The `source` of a settings entry: the value itself for a plain string,
    `.source` for an object. Matches `if type == "object" then .source else . end`.
    """
    if isinstance(entry, dict):
        src = entry.get("source")
        return None if src is None else str(src)
    return str(entry)


def settings_entries(path):
    """
    Every entry defined in a settings file as (canonical source, compact json)
    pairs, skipping entries without a source (jq: `select($s != "")`).
    """
    data = load_json(path)
    if not isinstance(data, dict):
        return []
    out = []
    for key in ("packages", "extensions"):
        values = data.get(key)
        if isinstance(values, list):
            for entry in values:
                src = entry_source(entry)
                if src == "" or src is None:
                    continue
                value = canonicalize_definition(entry)
                out.append((canonical_source(src), compact_json(value)))
    return out


def count_mmx(lines):
    enabled = sum(1 for ln in lines if not ln.startswith("#") and ln.strip())
    definitions = sum(1 for ln in lines if ln.strip())
    return enabled, definitions


def collect_pairs():
    """
    Every entry defined in the global or project settings as
    (canonical source, compact json) pairs, global first, deduped by source.
    """
    pairs = []
    seen = set()
    for path in (GLOBAL, LOCAL):
        for src, entry in settings_entries(path):
            if src not in seen:
                seen.add(src)
                pairs.append((src, entry))
    return pairs


def reconcile_lines(mmx, pairs):
    """
    Build the reconciled MMX content: canonicalize every line, keep enabled
    what the settings define, comment out the rest, append definitions missing
    from the file.
    """
    lines = []
    if Path(mmx).is_file():
        try:
            lines = Path(mmx).read_text(encoding="utf-8").splitlines()
        except OSError as e:
            sys.stderr.write(f"error: cannot read {mmx}: {e}\n")
            sys.exit(1)

    out = canonicalize_lines(lines)
    defined = {src for src, _ in pairs}
    present = set()
    result = []
    for line in out:
        parsed = parse_definition(line)
        if parsed is None:
            result.append(line)
            continue
        _, _, source = parsed
        present.add(source)
        result.append(set_enabled(line, source in defined))

    # merge in definitions that are active in the settings but missing from mm_extensions
    for src, entry in pairs:
        if src not in present:
            result.append(f"{entry},")
            present.add(src)
    return result


def write_mmx(mmx, out):
    mmx_path = Path(mmx).resolve()
    try:
        mmx_path.parent.mkdir(parents=True, exist_ok=True)
        fd, tmp = tempfile.mkstemp(prefix="mm_reconcile.", suffix=".tmp", dir=str(mmx_path.parent))
        with os.fdopen(fd, "w", encoding="utf-8") as f:
            f.write("\n".join(out))
            if out:
                f.write("\n")
        Path(tmp).replace(mmx_path)
    except OSError as e:
        sys.stderr.write(f"error: failed to write {mmx}: {e}\n")
        sys.exit(1)


def main():
    mmx = sys.argv[1] if len(sys.argv) > 1 else os.environ.get("MM_EXTENSIONS_FILE", MMX_DEFAULT)
    pairs = collect_pairs()
    out = reconcile_lines(mmx, pairs)
    write_mmx(mmx, out)
    enabled, definitions = count_mmx(out)
    sys.stdout.write(f"reconciled: {enabled} enabled of {definitions} definitions\n")


if __name__ == "__main__":
    main()
