#!/usr/bin/env python3
"""Regenerate the website catalogs and translations, then run every check.

Replaces the old bash wrapper (`scripts/localization_update.py` is the single
entry point now). That wrapper existed only to pick a Python interpreter and
chain four commands; Python can do that itself on every platform the project
supports, which is what makes this runnable on the Windows dev host without Git
Bash.

Usage:
    python scripts/localization_update.py           # do the work, then check
    python scripts/localization_update.py --check   # check only (CI gate)
"""

from __future__ import annotations

import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
PY = sys.executable


def run(step: str, *args: str) -> None:
    print(f"== {step} ==", flush=True)
    subprocess.run([PY, *args], cwd=ROOT, check=True)


def main() -> int:
    check_only = "--check" in sys.argv[1:]

    if not check_only:
        # Only strings whose English actually moved are touched; unchanged
        # translations are left byte-for-byte alone (localization-sync.py
        # records the English hash of each one).
        run("regenerating the website catalog", "scripts/extract_web_strings.py")
        run(
            "translating new or changed strings",
            "scripts/localization-sync.py",
            "--retranslate",
            "--report",
            "localization-report.md",
        )
        run("rebuilding the pages the catalogs feed", "scripts/generate_web_locales.py")

    # Every existing gate, in the same order CI runs them.
    run("check: catalog matches the source page", "scripts/extract_web_strings.py", "--check")
    run("check: generated locale pages are current", "scripts/generate_web_locales.py", "--check")
    run("check: translated docs carry the unreviewed banner", "scripts/localized-docs.py", "--check")
    run("check: translations match their English source", "scripts/localization-sync.py", "--check")
    print("localization: all checks passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
