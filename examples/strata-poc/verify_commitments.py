#!/usr/bin/env python3
"""Verify every artifact, corpus, and behavior-case Strata commitment."""

from __future__ import annotations

import hashlib
import json
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]


def verify(envelope_path: Path, verified: set[Path]) -> None:
    envelope = json.loads(envelope_path.read_text(encoding="utf-8"))
    for section in (
        envelope["artifact"],
        envelope["provenance"],
        envelope["behavior"],
    ):
        source = ROOT / section["source_uri"]
        if source in verified:
            continue
        observed = hashlib.sha256(source.read_bytes()).hexdigest()
        expected = section["sha256"]
        if observed != expected:
            raise SystemExit(
                f"{envelope_path.name}: {source} is {observed}, expected {expected}"
            )
        print(f"verified {source.relative_to(ROOT)} {observed}")
        verified.add(source)


def main() -> None:
    verified: set[Path] = set()
    for envelope_path in sorted(HERE.glob("*.envelope.json")):
        verify(envelope_path, verified)


if __name__ == "__main__":
    main()
