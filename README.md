# polysemic

[![CI](https://github.com/Polysemic-Systems/polysemic/actions/workflows/ci.yml/badge.svg)](https://github.com/Polysemic-Systems/polysemic/actions/workflows/ci.yml)

Infrastructure for systems that are alive. Three organs, zero dependencies.

> Digest the contradiction. Own the sediment. Bless the delete key.

## The triad

| Crate | Organ | What it does |
|---|---|---|
| `digest` | **metabolism** | The seam layer. Repairs malformed model output with every fix *named*; converts genuine ambiguity into a `Question`; applies answers only to requested paths with a separate answer ledger; reconciles multiple samples by field-wise majority. |
| `strata` | **sediment** | Ontology observability. `ProvenanceLabel` names the corpus; `DriftWatch` measures distribution drift; versioned `OntologySnapshot`s expose added, removed, and redefined concepts; `Legislature` is the contestable constraint layer. |
| `lethe` | **excretion** | The delete path. Every memory has a TTL and can carry a named `RetentionPolicy`; salience decays unless recall reinforces it; scheduled sweeps and on-demand erasure both return receipts; the open-core `ErasureAdapter` contract coordinates verified deletion across durable stores. |
| `polysemic-core` | — | The univocal interior: a strict JSON `Value` and parser. Polysemy lives at the boundary, where it belongs. |

## Design commitments

1. **Repair, don't throw — and never silently.** Every transformation is
   an entry in the `Repair` ledger.
2. **Ambiguity becomes a question, not a coin flip.** There is no state
   where you receive a value you can't trust.
3. **Extra meaning is kept, not refused.** Unknown keys pass through.
4. **Nothing enters without knowing how it will leave.** TTLs are
   mandatory in `lethe`; immortality is opt-in.
5. **Every answer names its layer.** Grown or legislated — the source is
   part of the resolution.
6. **Zero dependencies.** `std` only. Nothing to hoard.

## Diagnose the missing organ

Every AI system drifts toward one of three deaths:

| Diagnosis | Evidence | Prescription |
|---|---|---|
| **The Hoarder** | Unbounded memory, no retention owner, deletion cannot be demonstrated | Lethe |
| **The Amnesiac** | Categories and decisions lack provenance; ontology changes are invisible | Strata |
| **The Museum** | Formally clean output contracts reject or silently flatten unexpected meaning | Digest |

The diagnosis is evidence-led, not branding-led: a system can suffer more than one death, and a missing fact becomes a question rather than a forced label.

## Run it

```sh
cargo test              # all crates
cargo run -p seam-demo  # the hero terminal, live
cargo run -p digest-poc -- demo  # Digest as a standalone seam
```

The demo pushes one realistic mess through all three organs:

```
{'item': 'espresso', 'qty': '2 or 3', 'gift_wrap': True,}
```

…wrapped in prose and a markdown fence. Digest strips, requotes, and
repairs — then *asks* about the quantity instead of guessing and applies the
answer through a separate ledger. Strata prints provenance, flags distribution
drift, and diffs ontology versions. Lethe remembers under a named policy,
prunes on schedule, and produces sweep and erasure receipts.

## Digest in five minutes: a machine-usable seam

```sh
cargo run --quiet -p digest-poc -- demo
```

The standalone POC accepts a strict JSON Schema subset and arbitrary model
text. It emits one JSON envelope with `resolved`, `clarify`, or `rejected`
status; stable repair codes; structured questions; and a separate answer
ledger. Clarification has a distinct exit status, so an agent runner can route
the question instead of crashing or guessing. See the
[`Digest seam quickstart`](examples/digest-poc/README.md).

## Lethe in five minutes: two real stores

With Docker and Python 3 installed, run:

```sh
python3 examples/lethe-stores/lethe_poc.py demo
```

The dependency-free POC starts PostgreSQL+pgvector and Redis, writes the same
subject to both, erases it from both, verifies unrelated data survived, and
prints an independent SHA-256 receipt per store plus an aggregate receipt. It
also demonstrates idempotent request replay, partial-failure reporting, and an
optional no-LLM LangGraph lifecycle. See the
[`examples/lethe-stores` quickstart](examples/lethe-stores/README.md) and the
[`erasure contract`](docs/erasure-contract.md), plus the
[`store-selection` evidence](docs/store-selection.md).

## Honest limitations

This is a working skeleton, not a product: the schema language is small on
purpose and the CLI implements only an explicit JSON Schema subset, core
receipt hashes use FNV-1a (the store POC uses SHA-256), the Lethe store adapters
shell into dedicated local containers rather than native client libraries,
ontology definitions are compared structurally rather than with embeddings,
and the repair passes are heuristic scanners, not a grammar. The seams where
you'd extend them are marked in the doc comments.
