#!/usr/bin/env python3
import os, sys, json, datetime

home = os.path.expanduser("~")

for root, _, files in os.walk("."):
    for name in files:
        if not name.endswith(".jsonl"):
            continue

        path = os.path.join(root, name)
        try:
            mtime = os.path.getmtime(path)
            ts = datetime.datetime.fromtimestamp(mtime).strftime("%Y-%m-%d %H:%M:%S")

            with open(path, "r", encoding="utf-8") as fh:
                first_line = fh.readline()
                cwd = json.loads(first_line).get("cwd", "")

            if cwd.startswith(home):
                cwd = "~" + cwd[len(home):]

            session = name[:-6].rsplit("_", 1)[-1]
            print(f"{path}\t{ts}\t{cwd}\t{session}")
        except Exception:
            continue