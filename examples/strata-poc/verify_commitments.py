#!/usr/bin/env python3
"""Verify every artifact and corpus SHA-256 commitment in the Strata fixtures."""

from __future__ import annotations

import hashlib
import json
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]


def verify(envelope_path: Path) -> None:
    envelope = json.loads(envelope_path.read_text(encoding="utf-8"))
    for section in (envelope["artifact"], envelope["provenance"]):
        source = ROOT / section["source_uri"]
        observed = hashlib.sha256(source.read_bytes()).hexdigest()
        expected = section["sha256"]
        if observed != expected:
            raise SystemExit(
                f"{envelope_path.name}: {source} is {observed}, expected {expected}"
            )
        print(f"verified {source.relative_to(ROOT)} {observed}")


def main() -> None:
    for envelope_path in sorted(HERE.glob("*.envelope.json")):
        verify(envelope_path)


if __name__ == "__main__":
    main()
