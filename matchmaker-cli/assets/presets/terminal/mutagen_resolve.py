#!/usr/bin/env python3
import json
import os
import shlex
import shutil
import subprocess
import sys
import tempfile

class Endpoint:
    def __init__(self, data):
        self.protocol = data.get("protocol", "local")
        self.path = data.get("path", "")
        self.user = data.get("user")
        self.host = data.get("host")
        self.port = data.get("port")

    @property
    def is_local(self):
        return self.protocol == "local"

    @property
    def ssh_target(self):
        if self.user:
            return f"{self.user}@{self.host}"
        return self.host

    def _ssh_cmd(self, remote_command):
        cmd = ["ssh"]
        if self.port:
            cmd.extend(["-p", str(self.port)])
        cmd.extend([self.ssh_target, remote_command])
        return cmd

    def full_path(self, rel_path):
        return os.path.normpath(os.path.join(self.path, rel_path))

    def exists(self, rel_path):
        target = self.full_path(rel_path)
        if self.is_local:
            return os.path.lexists(target)
        else:
            cmd = self._ssh_cmd(f"test -e {shlex.quote(target)} || test -L {shlex.quote(target)}")
            res = subprocess.run(cmd, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            return res.returncode == 0

    def remove(self, rel_path):
        target = self.full_path(rel_path)
        if self.is_local:
            if os.path.islink(target) or os.path.isfile(target):
                os.remove(target)
            elif os.path.isdir(target):
                shutil.rmtree(target, ignore_errors=True)
        else:
            cmd = self._ssh_cmd(f"rm -rf -- {shlex.quote(target)}")
            subprocess.run(cmd, check=True)

    def fetch_to_temp(self, rel_path):
        """Fetch file content into a local temp file for diffing."""
        temp = tempfile.NamedTemporaryFile(delete=False)
        temp.close()
        target = self.full_path(rel_path)
        if self.is_local:
            if os.path.exists(target):
                shutil.copy2(target, temp.name)
            else:
                open(temp.name, "w").close()
        else:
            scp_cmd = ["scp"]
            if self.port:
                scp_cmd.extend(["-P", str(self.port)])
            scp_cmd.extend([f"{self.ssh_target}:{target}", temp.name])
            res = subprocess.run(scp_cmd, capture_output=True)
            if res.returncode != 0:
                open(temp.name, "w").close()
        return temp.name

    def copy_from(self, src_endpoint, rel_path):
        """Copy rel_path from src_endpoint to self."""
        src_path = src_endpoint.full_path(rel_path)
        dst_path = self.full_path(rel_path)

        if src_endpoint.is_local and self.is_local:
            if os.path.isdir(src_path):
                if os.path.exists(dst_path):
                    shutil.rmtree(dst_path, ignore_errors=True)
                shutil.copytree(src_path, dst_path, symlinks=True)
            else:
                os.makedirs(os.path.dirname(dst_path), exist_ok=True)
                shutil.copy2(src_path, dst_path, follow_symlinks=False)

        elif src_endpoint.is_local and not self.is_local:
            # Local -> Remote (Overwrite Remote)
            # Ensure destination parent directory exists
            remote_dir = os.path.dirname(dst_path)
            mkdir_cmd = self._ssh_cmd(f"mkdir -p -- {shlex.quote(remote_dir)}")
            subprocess.run(mkdir_cmd, check=True)

            scp_cmd = ["scp", "-r"]
            if self.port:
                scp_cmd.extend(["-P", str(self.port)])
            scp_cmd.extend([src_path, f"{self.ssh_target}:{dst_path}"])
            subprocess.run(scp_cmd, check=True)

        elif not src_endpoint.is_local and self.is_local:
            # Remote -> Local (Overwrite from Remote)
            os.makedirs(os.path.dirname(dst_path), exist_ok=True)
            if os.path.exists(dst_path):
                if os.path.isdir(dst_path) and not os.path.islink(dst_path):
                    shutil.rmtree(dst_path, ignore_errors=True)
                else:
                    os.remove(dst_path)

            scp_cmd = ["scp", "-r"]
            if src_endpoint.port:
                scp_cmd.extend(["-P", str(src_endpoint.port)])
            scp_cmd.extend([f"{src_endpoint.ssh_target}:{src_path}", dst_path])
            subprocess.run(scp_cmd, check=True)
        else:
            raise NotImplementedError("Direct remote-to-remote sync without local intermediary is not supported.")


def get_sessions(session_names):
    sessions = []
    for name in session_names:
        try:
            res = subprocess.run(
                ["mutagen", "sync", "list", name, "--template", "{{json .}}"],
                capture_output=True,
                text=True,
                check=True
            )
            data = json.loads(res.stdout)
            sessions.extend(data)
        except Exception as e:
            print(f"Error fetching session {name}: {e}", file=sys.stderr)
    return sessions


def analyze_conflict(conflict):
    root = conflict.get("root", "")
    alpha_changes = conflict.get("alphaChanges", [])
    beta_changes = conflict.get("betaChanges", [])

    # Check for orphaned untracked folder:
    # 1. Beta deleted folder, Alpha only has untracked
    beta_deleted = any(x.get("new") is None and x.get("path") == root for x in beta_changes)
    alpha_only_untracked = (
        bool(alpha_changes) and
        all((x.get("new") or {}).get("kind") == "untracked" for x in alpha_changes)
    )
    if beta_deleted and alpha_only_untracked:
        return "ORPHAN_UNTRACKED_LOCAL", root

    # 2. Alpha deleted folder, Beta only has untracked
    alpha_deleted = any(x.get("new") is None and x.get("path") == root for x in alpha_changes)
    beta_only_untracked = (
        bool(beta_changes) and
        all((x.get("new") or {}).get("kind") == "untracked" for x in beta_changes)
    )
    if alpha_deleted and beta_only_untracked:
        return "ORPHAN_UNTRACKED_REMOTE", root

    # Modification vs deletion or concurrent modification
    return "GENERAL_CONFLICT", root


def show_diff(alpha, beta, rel_path):
    f_alpha = alpha.fetch_to_temp(rel_path)
    f_beta = beta.fetch_to_temp(rel_path)
    try:
        pager = os.environ.get("PAGER", "less -R")
        diff_cmd = f"diff -u --color=always -L 'local (alpha)' -L 'remote (beta)' {shlex.quote(f_alpha)} {shlex.quote(f_beta)} | {pager}"
        subprocess.run(diff_cmd, shell=True)
    finally:
        if os.path.exists(f_alpha):
            os.remove(f_alpha)
        if os.path.exists(f_beta):
            os.remove(f_beta)


def clean_untracked_noninteractive(sessions):
    """Automatically cleans orphaned folders containing untracked/ignored files."""
    cleaned = False
    affected_sessions = set()

    for s in sessions:
        sess_name = s.get("name") or s.get("identifier")
        alpha = Endpoint(s.get("alpha", {}))
        beta = Endpoint(s.get("beta", {}))

        for c in s.get("conflicts", []):
            ctype, root = analyze_conflict(c)
            if ctype == "ORPHAN_UNTRACKED_LOCAL":
                print(f"[{sess_name}] Cleaning local orphaned untracked folder: {alpha.full_path(root)}")
                alpha.remove(root)
                cleaned = True
                affected_sessions.add(sess_name)
            elif ctype == "ORPHAN_UNTRACKED_REMOTE":
                print(f"[{sess_name}] Cleaning remote orphaned untracked folder: {beta.full_path(root)}")
                beta.remove(root)
                cleaned = True
                affected_sessions.add(sess_name)

    if cleaned:
        for name in affected_sessions:
            print(f"Flushing session {name}...")
            subprocess.run(["mutagen", "sync", "flush", name])
        print("Orphaned untracked folder conflicts resolved.")
    else:
        print("No orphaned untracked directory conflicts found.")


def interactive_resolve(sessions):
    """Interactively walk through conflicts one by one."""
    conflicts_to_resolve = []

    for s in sessions:
        sess_name = s.get("name") or s.get("identifier")
        alpha = Endpoint(s.get("alpha", {}))
        beta = Endpoint(s.get("beta", {}))

        for c in s.get("conflicts", []):
            ctype, root = analyze_conflict(c)
            conflicts_to_resolve.append((sess_name, alpha, beta, ctype, root, c))

    if not conflicts_to_resolve:
        print("No conflicts to resolve.")
        return

    # Use /dev/tty explicitly if available so stdin redirection inside wrappers does not break input
    try:
        tty_in = open("/dev/tty", "r")
    except Exception:
        tty_in = sys.stdin

    total = len(conflicts_to_resolve)
    affected_sessions = set()

    for idx, (sess_name, alpha, beta, ctype, root, raw_c) in enumerate(conflicts_to_resolve, 1):
        print("\n" + "=" * 70)
        print(f"[{idx}/{total}] Session: {sess_name} | Path: {root}")

        if ctype == "ORPHAN_UNTRACKED_LOCAL":
            print("Type: Orphaned directory (deleted remotely, untracked/ignored files locally)")
            print(f"Local (Alpha):  {alpha.full_path(root)} (Contains untracked content)")
            print(f"Remote (Beta):  <Deleted>")
        elif ctype == "ORPHAN_UNTRACKED_REMOTE":
            print("Type: Orphaned directory (deleted locally, untracked/ignored files remotely)")
            print(f"Local (Alpha):  <Deleted>")
            print(f"Remote (Beta):  {beta.full_path(root)} (Contains untracked content)")
        else:
            print("Type: File / directory modification conflict")
            print(f"Local (Alpha):  {alpha.full_path(root)}")
            print(f"Remote (Beta):  {beta.full_path(root)}")

        print("-" * 70)
        print("  [a] Keep Alpha      (overwrite remote)")
        print("  [b] Keep Beta       (overwrite local)")
        if ctype in ("ORPHAN_UNTRACKED_LOCAL", "ORPHAN_UNTRACKED_REMOTE"):
            print("  [c] Clean untracked folder (delete orphaned folder to resolve)")
        print("  [d] View diff")
        print("  [s] Skip")
        print("  [q] Quit")

        valid_choices = "a/b/c/d/s/q" if ctype in ("ORPHAN_UNTRACKED_LOCAL", "ORPHAN_UNTRACKED_REMOTE") else "a/b/d/s/q"
        while True:
            print(f"Choice [{valid_choices}]: ", end="", flush=True)
            choice = tty_in.readline().strip().lower()
            if choice == "a":
                # Alpha -> Beta: Local wins
                if alpha.exists(root):
                    beta.copy_from(alpha, root)
                    print(f"Overwrote remote with local: {beta.full_path(root)}")
                else:
                    beta.remove(root)
                    print(f"Removed from remote: {beta.full_path(root)}")
                affected_sessions.add(sess_name)
                break
            elif choice == "b":
                # Beta -> Alpha: Remote wins
                if beta.exists(root):
                    alpha.copy_from(beta, root)
                    print(f"Overwrote local with remote: {alpha.full_path(root)}")
                else:
                    alpha.remove(root)
                    print(f"Removed locally: {alpha.full_path(root)}")
                affected_sessions.add(sess_name)
                break
            elif choice == "c" and ctype == "ORPHAN_UNTRACKED_LOCAL":
                alpha.remove(root)
                print(f"Cleaned local orphaned folder: {alpha.full_path(root)}")
                affected_sessions.add(sess_name)
                break
            elif choice == "c" and ctype == "ORPHAN_UNTRACKED_REMOTE":
                beta.remove(root)
                print(f"Cleaned remote orphaned folder: {beta.full_path(root)}")
                affected_sessions.add(sess_name)
                break
            elif choice == "d":
                show_diff(alpha, beta, root)
            elif choice == "s":
                print("Skipped.")
                break
            elif choice == "q":
                print("Aborted.")
                return

    if affected_sessions:
        for name in affected_sessions:
            print(f"Flushing session {name}...")
            subprocess.run(["mutagen", "sync", "flush", name])
        print("Done. Flushed affected sessions.")


def main():
    args = sys.argv[1:]
    clean_untracked_mode = False
    session_names = []

    for arg in args:
        if arg == "--clean-untracked":
            clean_untracked_mode = True
        elif not arg.startswith("-"):
            session_names.append(arg)

    if not session_names:
        session_names = ["gh"]

    sessions = get_sessions(session_names)

    if clean_untracked_mode:
        clean_untracked_noninteractive(sessions)
    else:
        interactive_resolve(sessions)


if __name__ == "__main__":
    main()
