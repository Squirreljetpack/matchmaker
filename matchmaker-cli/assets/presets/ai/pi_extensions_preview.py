#!/usr/bin/env python3
"""
Preview pane for the pi extensions manager preset (pi_extensions.toml).

Usage:
  pi_extensions_preview.py <source>        full preview: definition, install
                                          location, and installed scope
  pi_extensions_preview.py --dir <source>  print just the install directory
                                          (used by the README preview pane)

Exits 1 when the source is not installed anywhere (--dir mode).
"""

import json
import os
import re
import sys
from json import JSONDecodeError
from pathlib import Path

AGENT_DIR = Path("~/.pi/agent").expanduser()
PROJECT_DIR = Path(".pi")
MMX_DEFAULT = Path("~/.pi/agent/mm_extensions").expanduser()
GLOBAL = Path("~/.pi/agent/settings.json").expanduser()
LOCAL = Path(".pi/settings.json")

# Tolerate trailing commas from manual edits (jsonc-ish) before parsing.
_TRAILING_COMMA = re.compile(r",\s*([}\]])")

# npm sources keep a version pin out of the package name: `@<seg>` at the end.
_NPM_PIN = re.compile(r"@[^/@]*$")


def git_rel(source):
    """
    <host>/<user>/<repo> for a git source (without the `git:` prefix),
    with the pinned @ref stripped (same first-@ rule pi uses). None when the
    source does not look like a git source.
    """
    if "://" in source:
        # https://host/user/repo@ref, ssh://git@host/user/repo
        rest = source.split("://", 1)[1]
        host = rest.split("/", 1)[0]
        host = host.rsplit("@", 1)[-1]  # drop userinfo (git@, token@)
        host = host.split(":", 1)[0]  # drop :port
        path = rest.split("/", 1)[1] if "/" in rest else ""
    elif source.startswith("git@"):
        # git@host:user/repo@ref (scp-like)
        host = source[4:].split(":", 1)[0]
        path = source.split(":", 1)[1] if ":" in source else ""
    else:
        # host/user/repo@ref (git: shorthand)
        host = source.split("/", 1)[0]
        path = source.split("/", 1)[1] if "/" in source else ""

    # hosts must be sane; anything else (e.g. a colon typo) is not a git source
    if not host or not re.fullmatch(r"[A-Za-z0-9._-]+", host):
        return None
    if not path:
        return None
    path = path.split("@", 1)[0]  # strip the @ref (same first-@ rule pi uses)
    if not path:
        return None
    return f"{host}/{path}"


def npm_name(source):
    """The package name of an npm source without a version pin."""
    return _NPM_PIN.sub("", source.removeprefix("npm:"))


def _first_dir(candidates):
    for candidate in candidates:
        if candidate.is_dir():
            return candidate
    return None


def _first_existing(candidates):
    for candidate in candidates:
        if candidate.exists():
            return candidate
    return None


def extension_dir(source):
    """
    The on-disk install location of a pi package/extension source, or None.

    pi installs packages to different roots depending on scope:
      global settings  -> ~/.pi/agent/
      project settings -> <project>/.pi/        (project wins over global)

    Layout inside the root:
      git sources  -> git/<host>/<user>/<repo>   (a pinned @ref is a checkout
                                                  ref, not a path segment)
      npm sources  -> npm/node_modules/<name>    (a version pin is not part of
                                                  the package name)
      local paths  -> used in place (absolute, ~-relative, or relative to the
                      settings file's base)

    Project scope is checked first for git/npm sources, then global.
    """
    if source.startswith("npm:"):
        name = npm_name(source)
        return _first_dir(root / "npm/node_modules" / name for root in (PROJECT_DIR, AGENT_DIR))

    if source.startswith("git:"):
        rel = git_rel(source[4:])
        roots = (PROJECT_DIR, AGENT_DIR)
    elif re.match(r"^(https?|ssh|git)://", source):
        rel = git_rel(source)
        roots = (PROJECT_DIR, AGENT_DIR)
    else:
        # local path: absolute, ~-relative, or relative to a settings base
        path = Path(source).expanduser()
        if path.is_absolute():
            return path if path.exists() else None
        return _first_existing(root / source for root in (AGENT_DIR, PROJECT_DIR, Path.cwd()))

    if rel is None:
        return None
    return _first_dir(root / "git" / rel for root in roots)


def settings_contains(path, name):
    """True when the settings file defines `name` in packages/extensions."""
    try:
        text = Path(path).read_text(encoding="utf-8")
    except OSError:
        return False
    try:
        data = json.loads(text)
    except JSONDecodeError:
        try:
            data = json.loads(_TRAILING_COMMA.sub(r"\1", text))
        except JSONDecodeError:
            return False
    if not isinstance(data, dict):
        return False
    for key in ("packages", "extensions"):
        for entry in data.get(key, []) if isinstance(data.get(key), list) else []:
            src = entry.get("source") if isinstance(entry, dict) else entry
            if src == name:
                return True
    return False


def mmx_definition(name):
    """
    The first MMX entry matching `name` as a parsed definition value.
    None when there is no MMX, the name is not listed, or the line is not
    parseable.
    """
    mmx = os.environ.get("MM_EXTENSIONS_FILE", MMX_DEFAULT)
    if not Path(mmx).is_file():
        return None
    try:
        lines = Path(mmx).read_text(encoding="utf-8").splitlines()
    except OSError:
        return None
    for line in lines:
        stripped = line[2:] if line.startswith("# ") else (line.removeprefix("#"))
        if not stripped.strip():
            continue
        try:
            value = json.loads(stripped.rstrip(","))
        except JSONDecodeError:
            continue
        src = value.get("source") if isinstance(value, dict) else value
        if src == name:
            return value
    return None


def preview_full(name):
    defn_value = mmx_definition(name)
    if defn_value is not None:
        sys.stdout.write(json.dumps(defn_value, ensure_ascii=False, separators=(",", ":")) + "\n")
    sys.stdout.write("---\n")

    # install location: pi clones git sources to <root>/git/<host>/<user>/<repo>
    # (a pinned @ref is a checkout ref, not a path segment) and npm sources to
    # <root>/npm/node_modules/<name> (a version pin is not part of the name);
    # <root> is ~/.pi/agent for global settings and .pi/ for project settings.
    directory = extension_dir(name)
    if directory:
        sys.stdout.write(f"location: {directory}\n")
        if name.startswith("npm:") and (directory / "package.json").is_file():
            try:
                pkg = json.loads((directory / "package.json").read_text(encoding="utf-8"))
            except (OSError, JSONDecodeError):
                pkg = None
            if pkg is not None:
                version = pkg.get("version", "?")
                description = pkg.get("description", "")
                suffix = f" — {description}" if description else ""
                sys.stdout.write(f"version: {version}{suffix}\n")
    else:
        reason = (
            "(npm)"
            if name.startswith("npm:")
            else ("(lazy)" if isinstance(defn_value, dict) and defn_value.get("lazy") else "")
        )
        sys.stdout.write(f"location: ✗{(' ' + reason) if reason else ''}\n")

    global_defined = settings_contains(GLOBAL, name)
    local_defined = settings_contains(LOCAL, name)
    if global_defined and local_defined:
        sys.stdout.write("installed: global | local\n")
    elif global_defined:
        sys.stdout.write("installed: global\n")
    elif local_defined:
        sys.stdout.write("installed: local\n")
    else:
        sys.stdout.write("installed: ✗\n")


def main():
    args = sys.argv[1:]
    if args and args[0] == "--dir":
        name = args[1] if len(args) > 1 else ""
        if not name:
            return
        directory = extension_dir(name)
        if directory is None:
            sys.exit(1)
        sys.stdout.write(f"{directory}\n")
        return
    if args:
        preview_full(args[0])


if __name__ == "__main__":
    main()
