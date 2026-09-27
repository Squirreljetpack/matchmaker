#!/usr/bin/env python3
"""
Shared helpers for the pi extensions manager preset (pi_extensions.toml).

mm_extensions stores pi package definitions. pi accepts several spellings for
the same git package, so every helper normalizes them to
`git:<host>/<user>/<repo>[@ref]`, the identity pi itself uses.

Spellings whose transport is HTTPS are folded; sources with an explicit
non-HTTPS transport keep it, because the scheme decides how pi clones the
package on install/update:

  folded   git:host/user/repo          (HTTPS)
           git:host:user/repo          (HTTPS; redundant colon)
           https://host/user/repo      (HTTPS)
  kept     ssh://git@host/user/repo    (SSH)
           git:git@host:user/repo      (SSH)
           git://host/user/repo        (git protocol)
           http://host/user/repo       (plain HTTP)

Trailing `.git` suffixes and `@ref`s follow pi's first-`@` rule.
"""

import json
import re
from json import JSONDecodeError
from urllib.parse import urlparse

# Explicit git transports.
_GIT_SCHEME = re.compile(r"^(https?|ssh|git)://", re.IGNORECASE)
# A plausible hostname (also matches IPs); rejects `..`, `.` and empty heads.
_HOSTNAME = re.compile(r"[a-z0-9]([a-z0-9._-]*[a-z0-9])?")


def _looks_like_host(host):
    return host == "localhost" or bool(_HOSTNAME.fullmatch(host))


def _canonical_shorthand(rest):
    """
    `host/user/repo` (or the `host:user/repo` typo) -> `host/user/repo[@ref]`.
    None when `rest` is not a bare shorthand or is malformed.
    """
    head, sep, path = rest.partition("/")
    if not sep or not path:
        return None
    if ":" in head:
        host, _, user = head.partition(":")
        # `host:port/path` carries a port; only `host:user/path` is the typo.
        if not host or not user or user.isdigit():
            return None
        path = f"{user}/{path}"
    else:
        host = head
    path, _, ref = path.partition("@")
    path = path.strip("/")
    if path.lower().endswith(".git"):
        path = path[:-4]
    if not _looks_like_host(host.lower()) or not path:
        return None
    return f"{host.lower()}/{path}" + (f"@{ref}" if ref else "")


def _https_shorthand(text):
    """
    `https://host/user/repo[@ref]` -> `host/user/repo[@ref]`. None when the URL
    carries userinfo or a port (dropping either would change how it clones) or
    does not parse.
    """
    try:
        parsed = urlparse(text)
    except ValueError:
        return None
    if parsed.username or parsed.password or parsed.port is not None:
        return None
    host = parsed.hostname
    if not host:
        return None
    path, _, ref = parsed.path.lstrip("/").partition("@")
    path = path.strip("/")
    if path.lower().endswith(".git"):
        path = path[:-4]
    if not _looks_like_host(host.lower()) or not path:
        return None
    return f"{host.lower()}/{path}" + (f"@{ref}" if ref else "")


def canonical_source(source):
    """
    The canonical `git:<host>/<user>/<repo>[@ref]` spelling of a git source.
    HTTPS-equivalent spellings are folded; other transports and every non-git
    source are returned unchanged.
    """
    if not isinstance(source, str):
        return source
    text = source.strip()
    if text.startswith("git:"):
        rest = text[4:].strip()
        if "://" not in rest and not rest.startswith("git@"):
            canonical = _canonical_shorthand(rest)
            if canonical is not None:
                return "git:" + canonical
        elif rest.lower().startswith("https://"):
            canonical = _https_shorthand(rest)
            if canonical is not None:
                return "git:" + canonical
        return text
    if text.lower().startswith("https://") and _GIT_SCHEME.match(text):
        canonical = _https_shorthand(text)
        if canonical is not None:
            return "git:" + canonical
    return source


def git_identity(source):
    """
    The `host/user/repo` identity pi uses for a git checkout, or None when the
    source does not parse as a git source. Handles the `git:` prefix, git@ and
    protocol URLs, plus the bare host/user/repo shorthand; pinned `@ref`s and a
    trailing `.git` are stripped.
    """
    canonical = canonical_source(source)
    if not isinstance(canonical, str):
        return None
    text = canonical.strip()
    if text.startswith("npm:"):
        return None
    text = text.removeprefix("git:")
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


def is_git_ish(source):
    """
    True when pi could have installed `source` into a git/ tree: `git:`
    prefix, an explicit git URL, an scp-like git@ URL, or a shorthand whose
    first segment looks like a host.
    """
    if not isinstance(source, str):
        return False
    if source.startswith(("git:", "git@")) or _GIT_SCHEME.match(source):
        return True
    head = source.split("/", 1)[0]
    return "/" in source and _looks_like_host(head)


def split_comment(line):
    """`(enabled, payload)` — strip a single comment prefix from an MMX line."""
    if line.startswith("# "):
        return False, line[2:]
    if line.startswith("#"):
        return False, line[1:]
    return True, line


def parse_definition(line):
    """
    `(enabled, value, source)` for an MMX line. None for blank or unparseable
    lines, and for definitions without a string source.
    """
    enabled, payload = split_comment(line)
    text = payload.rstrip(",").strip()
    if not text:
        return None
    try:
        value = json.loads(text)
    except JSONDecodeError:
        return None
    source = value.get("source") if isinstance(value, dict) else value
    if not isinstance(source, str) or not source:
        return None
    return enabled, value, source


def render_definition(value, source, enabled):
    """A canonical MMX line for `value` with `source` set, trailing comma included."""
    if isinstance(value, dict):
        definition = {**value, "source": source}
    else:
        definition = source
    payload = json.dumps(definition, ensure_ascii=False, separators=(",", ":")) + ","
    return payload if enabled else "# " + payload


def set_enabled(line, enabled):
    """`line` with its comment state forced to `enabled`."""
    _, payload = split_comment(line)
    return payload if enabled else "# " + payload


def canonicalize_definition(value):
    """`value` with its `source` replaced by the canonical spelling."""
    if isinstance(value, dict):
        source = value.get("source")
        if isinstance(source, str):
            return {**value, "source": canonical_source(source)}
        return value
    if isinstance(value, str):
        return canonical_source(value)
    return value


def canonicalize_lines(lines):
    """
    Rewrite every parseable line to its canonical source and drop later
    duplicates of the same source, keeping the first (upgraded to enabled when
    a duplicate is enabled). Blank and unparseable lines are preserved in
    place.
    """
    kept = []  # [canonical-source-or-None, value-or-raw-line, enabled]
    index = {}  # canonical source -> position in kept
    for line in lines:
        parsed = parse_definition(line)
        if parsed is None:
            kept.append([None, line, None])
            continue
        enabled, value, source = parsed
        canonical = canonical_source(source)
        position = index.get(canonical)
        if position is not None:
            if enabled and not kept[position][2]:
                kept[position][2] = True
            continue
        index[canonical] = len(kept)
        kept.append([canonical, value, enabled])
    return [entry[1] if entry[0] is None else render_definition(entry[1], entry[0], entry[2]) for entry in kept]
