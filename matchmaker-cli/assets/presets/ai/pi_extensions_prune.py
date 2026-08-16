#!/usr/bin/env python3
"""
Uninstall pi extensions installed on disk but not listed in the extension
list (MMX).

pi's own `uninstall` only works for sources configured in settings.json, so
leftover installs (removed from the list, installed manually, ...) have to be
found by scanning the managed install trees:

  global  -> $HOME/.pi/agent/git and $HOME/.pi/agent/npm
  local   -> $PWD/.pi/git and $PWD/.pi/npm

The run is split into two steps:

1. plan — collect the main folders that would be deleted (and the npm
   manifest cleanup): `plan_scope()` returns a `Plan`.
2. apply — delete exactly those folders (`delete_tree` + empty-parent
   cleanup) and drop pruned packages from the managed npm project's
   package.json (`drop_manifest_deps`).

The plan (the main folders + manifest edits) is always listed on stdout
first; a real run pauses for a y/N confirmation before applying it. Git
checkouts are matched by their <host>/<path> identity, npm packages by name.
Only packages that ship pi extension resources are candidates, packages that
only exist as dependencies of listed extensions are kept, and disabled
(`# ...`) MMX lines still count as listed.

Usage: pi_extensions_prune.py [-n|--dry-run] [-y|--yes] [MMX]
  MMX       defaults to $MM_EXTENSIONS_FILE, else ~/.pi/agent/mm_extensions
  -n        list the plan without deleting anything
  -y        delete without the interactive confirmation
"""

import contextlib
import json
import os
import re
import shutil
import sys
import tempfile
from collections import deque
from dataclasses import dataclass
from json import JSONDecodeError
from pathlib import Path
from urllib.parse import urlparse

MMX_DEFAULT = Path("~/.pi/agent/mm_extensions").expanduser()
AGENT_DIR = Path("~/.pi/agent").expanduser()
PROJECT_DIR = Path(".pi")

# A package counts as an extension when it declares any of these in its
# `pi` manifest or ships the matching resource directory (same fields pi's
# collectPackageResources checks).
RESOURCE_FIELDS = ("extensions", "skills", "prompts", "themes")

# Tolerate trailing commas from manual edits (jsonc-ish) before parsing.
_TRAILING_COMMA = re.compile(r",\s*([}\]])")
# npm sources keep a version pin out of the package name: `@<seg>` at the end.
_NPM_PIN = re.compile(r"@[^/@]*$")
_GIT_SCHEME = re.compile(r"^(https?|ssh|git)://", re.IGNORECASE)
# A plausible hostname (also matches IPs); rejects `..`, `.` and empty heads.
_HOSTNAME = re.compile(r"[a-z0-9]([a-z0-9._-]*[a-z0-9])?")


@dataclass
class Deletion:
    """One main folder to delete, plus now-empty parents to tidy up after."""

    scope: str          # "global" | "local"
    kind: str           # "git" | "npm"
    label: str          # git identity or npm package name
    path: Path          # the main folder
    empty_parents: tuple = ()   # dirs to rmdir (innermost first), if left empty


@dataclass
class ManifestUpdate:
    """Drop package names from a managed npm project's package.json."""

    scope: str
    path: Path
    names: list


@dataclass
class Plan:
    """Everything a real prune run would do."""

    deletions: list
    manifests: list
    kept: int


def _looks_like_host(host):
    return host == "localhost" or bool(_HOSTNAME.fullmatch(host))


def load_json(path):
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


def line_source(line):
    """
    The parsed value of an MMX line (comment prefix and trailing comma
    stripped, JSON parsed) and its display source. None when not parseable.
    """
    stripped = line[2:] if line.startswith("# ") else line.removeprefix("#")
    if not stripped.strip():
        return None
    try:
        value = json.loads(stripped.rstrip(","))
    except JSONDecodeError:
        return None
    if isinstance(value, dict):
        source = value.get("source")
        return value if source is None else source
    return value


def npm_name(source):
    """The package name of an npm source without a version pin."""
    return _NPM_PIN.sub("", source.removeprefix("npm:"))


def is_git_ish(source):
    """
    True when pi could have installed `source` into a git/ tree: `git:`
    prefix, an explicit git URL, an scp-like git@ URL, or a shorthand
    whose first segment looks like a host.
    """
    if source.startswith(("git:", "git@")) or _GIT_SCHEME.match(source):
        return True
    head = source.split("/", 1)[0]
    return "/" in source and _looks_like_host(head)


def git_identity(source):
    """
    The `host/path` identity pi uses for a git source checkout, or None when
    the source does not parse as a git source. Handles the `git:` prefix,
    git@ and protocol URLs, plus the bare host/user/repo shorthand; pinned
    `@ref`s and a trailing `.git` are stripped.
    """
    text = source.strip().removeprefix("git:")
    host = None
    path = None
    if _GIT_SCHEME.match(text):
        try:
            parsed = urlparse(text)
        except ValueError:
            return None
        host, path = parsed.hostname, parsed.path
    elif text.startswith("git@"):
        match = re.match(r"^git@([^:]+):(.*)$", text)
        if match:
            host, path = match.group(1), match.group(2)
    else:
        head, sep, rest = text.partition("/")
        if not sep:
            return None
        host, path = head, rest
        # pi only accepts bare shorthand hosts that look like hosts
        if not host or ("." not in host and host != "localhost"):
            return None
    if not host or not path:
        return None
    host = host.lower()
    path = path.strip("/").split("@", 1)[0]  # strip the @ref (first-@ rule)
    if path.lower().endswith(".git"):
        path = path[:-4]
    if not _looks_like_host(host) or not path:
        return None
    return f"{host}/{path}"


def read_mmx(mmx):
    """
    The listed sources: ({npm names}, {git identities}, failed git-ish
    lines). Disabled lines count as listed. `failed` collecting any git-ish
    line that could not be mapped to an identity; when non-empty the git
    pass must not run (pruning could remove a listed checkout).
    """
    try:
        lines = Path(mmx).read_text(encoding="utf-8").splitlines()
    except OSError as error:
        sys.stderr.write(f"error: cannot read {mmx}: {error}\n")
        sys.exit(1)
    npm_names = set()
    git_ids = set()
    failed = []
    for line in lines:
        if not line.strip():
            continue
        source = line_source(line)
        if not isinstance(source, str) or not source:
            continue
        if source.startswith("npm:"):
            npm_names.add(npm_name(source))
        elif is_git_ish(source):
            identity = git_identity(source)
            if identity:
                git_ids.add(identity)
            else:
                failed.append(source)
    return npm_names, git_ids, failed


def is_pi_extension(pkg_dir):
    """True when the package at `pkg_dir` ships pi extension resources."""
    pkg = load_json(pkg_dir / "package.json")
    if isinstance(pkg, dict) and isinstance(pkg.get("pi"), dict):
        for field in RESOURCE_FIELDS:
            entries = pkg["pi"].get(field)
            if isinstance(entries, list) and entries:
                return True
    return any((pkg_dir / field).is_dir() for field in RESOURCE_FIELDS)


def dependency_names(pkg):
    """The declared dependency names of a package (deps, peers, optional)."""
    names = set()
    for key in ("dependencies", "peerDependencies", "optionalDependencies"):
        value = pkg.get(key)
        if isinstance(value, dict):
            names.update(value.keys())
    return names


def dep_closure(npm_root, names):
    """
    Every package reachable as a dependency of the listed npm names in the
    given npm tree. Missing dependency dirs are skipped.
    """
    seen = set(names)
    queue = deque(names)
    node_modules = npm_root / "node_modules"
    while queue:
        name = queue.popleft()
        pkg_dir = node_modules / name
        pkg = load_json(pkg_dir / "package.json") if (pkg_dir / "package.json").is_file() else None
        if not isinstance(pkg, dict):
            continue
        for dep in dependency_names(pkg):
            if dep not in seen:
                seen.add(dep)
                queue.append(dep)
    return seen


def npm_package_entries(npm_root):
    """The top-level `node_modules/<name>` packages as (name, path) pairs."""
    node_modules = npm_root / "node_modules"
    if not node_modules.is_dir():
        return
    for entry in sorted(node_modules.iterdir(), key=lambda p: p.name):
        if entry.name.startswith(".") or not entry.is_dir():
            continue
        if entry.name.startswith("@"):
            for sub in sorted(entry.iterdir(), key=lambda p: p.name):
                if sub.name.startswith(".") or not sub.is_dir():
                    continue
                yield f"{entry.name}/{sub.name}", sub
        else:
            yield entry.name, entry


def git_repo_entries(git_root):
    """
    The extension checkouts under a git root as (identity, path) pairs.
    Repos are detected by their `.git` dir or a root package.json; parents
    are traversed depth-first.
    """
    if not git_root.is_dir():
        return
    stack = [git_root]
    while stack:
        directory = stack.pop()
        for entry in sorted(directory.iterdir(), key=lambda p: p.name):
            if entry.name.startswith(".") or not entry.is_dir():
                continue
            if (entry / ".git").exists() or (entry / "package.json").is_file():
                rel = entry.relative_to(git_root).as_posix()
                parts = rel.split("/")
                yield f"{parts[0].lower()}/{'/'.join(parts[1:])}", entry
            else:
                stack.append(entry)


def delete_tree(path):
    """Remove a directory or a symlink to one, exiting on failure."""
    try:
        if path.is_symlink():
            path.unlink()
        else:
            shutil.rmtree(path)
    except OSError as error:
        sys.stderr.write(f"error: failed to remove {path}: {error}\n")
        sys.exit(1)


def drop_manifest_deps(manifest, names, scope):
    """
    Drop pruned packages from the managed npm project's package.json so a
    later plain `npm install` in that tree does not reinstall them.
    """
    data = load_json(manifest)
    if not isinstance(data, dict) or not isinstance(data.get("dependencies"), dict):
        return
    deps = data["dependencies"]
    stale = sorted(name for name in names if name in deps)
    if not stale:
        return
    for name in stale:
        del deps[name]
    try:
        fd, tmp = tempfile.mkstemp(prefix="mm_prune.", suffix=".tmp", dir=str(manifest.parent))
        with os.fdopen(fd, "w", encoding="utf-8") as f:
            json.dump(data, f, ensure_ascii=False, indent=2)
            f.write("\n")
        Path(tmp).replace(manifest)
    except OSError as error:
        sys.stderr.write(f"error: failed to update {manifest}: {error}\n")
        sys.exit(1)
    sys.stdout.write(f"dropped {scope} npm manifest dependencies: {', '.join(stale)}\n")


def plan_scope(root, listed, scope):
    """
    The Plan for one pi root (global or project): which main folders to
    delete, which manifests to update, and how many checkouts/packages were
    kept. Nothing is deleted here.
    """
    npm_names, git_ids, failed_git = listed
    deletions = []
    manifests = []
    kept = 0

    git_root = root / "git"
    if failed_git and git_root.is_dir():
        sys.stderr.write(
            "warning: skipping git prunes — cannot map these mm_extensions "
            f"lines to an install identity: {', '.join(failed_git)}\n"
        )
    else:
        for identity, path in git_repo_entries(git_root):
            if identity in git_ids:
                kept += 1
                continue
            # now-empty host/user parents to tidy up after the removal
            parents = []
            current = path.parent
            while current != git_root and str(current).startswith(str(git_root)):
                parents.append(current)
                current = current.parent
            deletions.append(Deletion(scope, "git", identity, path, tuple(parents)))

    npm_root = root / "npm"
    if npm_root.is_dir():
        closure = dep_closure(npm_root, npm_names)
        extensions = [
            (name, path)
            for name, path in npm_package_entries(npm_root)
            if is_pi_extension(path)
        ]
        for name, path in extensions:
            if name in closure:
                kept += 1
                continue
            # the scope dir (node_modules/@<scope>) is tidied when left empty
            parents = (path.parent,) if path.parent.name.startswith("@") else ()
            deletions.append(Deletion(scope, "npm", name, path, parents))
        pruned_names = sorted(d.label for d in deletions if d.kind == "npm")
        if pruned_names:
            manifests.append(ManifestUpdate(scope, npm_root / "package.json", pruned_names))

    return Plan(deletions, manifests, kept)


def apply_plan(plan):
    """Delete the planned folders and apply the manifest updates."""
    for d in plan.deletions:
        sys.stdout.write(f"removed {d.scope} {d.kind} package: {d.label}\n")
        delete_tree(d.path)
        for parent in d.empty_parents:
            with contextlib.suppress(OSError):
                parent.rmdir()
    for update in plan.manifests:
        drop_manifest_deps(update.path, update.names, update.scope)


def main():
    args = sys.argv[1:]
    dry_run = "-n" in args or "--dry-run" in args
    yes = "-y" in args or "--yes" in args
    positional = [a for a in args if a not in ("-n", "--dry-run", "-y", "--yes")]
    mmx = positional[0] if positional else os.environ.get("MM_EXTENSIONS_FILE", MMX_DEFAULT)

    listed = read_mmx(mmx)
    plans = [
        plan_scope(root, listed, scope)
        for scope, root in (("global", AGENT_DIR), ("local", PROJECT_DIR))
    ]
    deletions = [d for p in plans for d in p.deletions]
    manifests = [m for p in plans for m in p.manifests]
    kept = sum(p.kept for p in plans)

    if not deletions:
        sys.stdout.write("nothing to prune\n")
        sys.exit(1)

    verb = "would delete" if dry_run else "about to delete"
    sys.stdout.write(f"{verb} {len(deletions)} main folder(s):\n")
    for d in deletions:
        sys.stdout.write(f"  {d.scope} {d.kind}: {d.path}\n")
    for m in manifests:
        sys.stdout.write(f"  {m.scope} npm manifest: {m.path} (drop {', '.join(m.names)})\n")

    if dry_run:
        sys.stdout.write(f"dry run: no files changed, {kept} kept\n")
        return

    if not yes:
        if not sys.stdin.isatty():
            sys.stderr.write(
                "refusing to run non-interactively: pass -y/--yes to delete "
                "or -n/--dry-run to preview\n"
            )
            sys.exit(2)
        answer = input(f"delete these {len(deletions)} folder(s)? [y/N] ").strip().lower()
        if answer not in ("y", "yes"):
            sys.stdout.write("aborted, nothing deleted\n")
            sys.exit(0)

    apply_plan(Plan(deletions, manifests, kept))
    sys.stdout.write(f"pruned: {len(deletions)} folder(s) removed, {kept} kept\n")


if __name__ == "__main__":
    main()
