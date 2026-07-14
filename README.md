# polysemic

Infrastructure for systems that are alive. Three organs, zero dependencies.

> Digest the contradiction. Own the sediment. Bless the delete key.

## The triad

| Crate | Organ | What it does |
|---|---|---|
| `digest` | **metabolism** | The seam layer. Repairs malformed model output (fences, prose, single quotes, Python literals, bare keys, trailing commas) with every fix *named*; validates against a small schema language; converts genuine ambiguity (`"2 or 3"`) into a `Question` instead of a guess; reconciles multiple samples by field-wise majority. |
| `strata` | **sediment** | Ontology observability. `ProvenanceLabel` is a shipping manifest for training data; `DriftWatch` measures total-variation distance between the model's grown categories and your users' reality; `Legislature` is the explicit, contestable rule layer over the grown one — every resolution names which layer answered. |
| `lethe` | **excretion** | The delete path. Every memory is born with a TTL; salience decays by half-life unless recall reinforces it; `sweep` prunes on schedule; `forget` erases on demand and returns a receipt (`lethe://era/…`). |
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

## Run it

```sh
cargo test              # all crates
cargo run -p seam-demo  # the hero terminal, live
```

The demo pushes one realistic mess through all three organs:

```
{'item': 'espresso', 'qty': '2 or 3', 'gift_wrap': True,}
```

…wrapped in prose and a markdown fence. Digest strips, requotes, and
repairs — then *asks* about the quantity instead of guessing. Strata
prints the provenance label and flags category drift. Lethe remembers,
prunes, and forgets with a receipt.

## Honest limitations

This is a working skeleton, not a product: the schema language is small on
purpose, the erasure receipt uses FNV-1a (swap in a cryptographic hash for
anything real), `lethe` is in-memory, and the repair passes are heuristic
scanners, not a grammar. The seams where you'd extend it are marked in the
doc comments.
