#!/usr/bin/env python3
"""Normalise only documented implementation differences, not VM-side payloads.

Read-only Tart queries may be cached/repeated or use JSON. Their exact count is
not a compatibility contract; mutations and SSH argv remain strictly ordered.
Lifecycle timing, failures and registry writes are covered by Rust tests.
"""
import os
from pathlib import Path
import re
import sys


def normalise(text, tool):
    for key in ("PROJECT", "HOME", "WORLD"):
        text = text.replace(os.environ[key], "$" + key)
    text = text.replace("tctl/config.toml", "tctl/$GLOBAL_CFG")
    text = re.sub(r"tctl/config\b", "tctl/$GLOBAL_CFG", text)
    text = text.replace(".tart-ctl.toml", "$PROJECT_CFG")
    text = text.replace(".tart-ctl-env", "$PROJECT_CFG")
    text = re.sub(r"(?<![\w/.-])" + re.escape(tool) + r"(?![\w/.-])", "$TOOL", text)
    text = re.sub(r"PID \d+", "PID N", text)
    text = re.sub(r"(background PID N), IP [^)]+", r"\1", text)
    text = text.replace("TCTL_HOST_SHARE_DIR", "$SHARE_KEY").replace("share.dir", "$SHARE_KEY")
    return text


def main():
    root = Path(sys.argv[1])
    tool = sys.argv[2]
    for path in root.rglob("*"):
        if not path.is_file():
            continue
        text = normalise(path.read_text(), tool)
        if path.name == "calls.log":
            text = "".join(
                line for line in text.splitlines(keepends=True)
                if line != "tart list\n" and line != "tart list --format json\n"
                and not line.startswith("tart ip ")
            )
        path.write_text(text)


if __name__ == "__main__":
    main()
