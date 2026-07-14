# Strata provenance and ontology-drift POC

This proof makes ontology change inspectable without detaching it from the
corpora that produced each version. It compares two provenance envelopes,
reports added, removed, and redefined concepts, and probes one concept on both
sides of the version boundary. It verifies and executes both committed router
artifacts against the same separately committed case corpus, so a changed
classification and downstream route are observed behavior rather than an
assertion copied from the envelopes.

## Run the proof

From the repository root:

```sh
cargo run --quiet -p strata-poc -- demo
python3 examples/strata-poc/verify_commitments.py
```

The committed example compares two revisions of `family-support-router`:

- the baseline inherited uncredited labels from 2018–2019 support tickets;
- the candidate was reviewed by credited support staff and affected users;
- `chosen` is added, `single_parent` is removed, and `nuclear` is redefined;
- probing `nuclear` exposes both definitions, so the changed social boundary
  is visible rather than hidden behind a stable field name.
- executing the same `chosen-caregiver` input changes from `other` /
  `manual-review` to `chosen` / `family-support`;
- SHA-256 commitments resolve to two executable router artifacts, two corpus
  manifests, and one shared behavior-case corpus; both `compare` and
  `verify_commitments.py` recompute them.

## Compare your own envelopes

```sh
cargo run --quiet -p strata-poc -- compare \
  --baseline examples/strata-poc/model-v1.envelope.json \
  --observed examples/strata-poc/candidate-v2.envelope.json \
  --probe nuclear \
  --case chosen-caregiver
```

`compare` writes exactly one JSON object. Its exit status is intentionally `2`
for this example because drift is a routable finding, not a successful
no-change comparison. A caller can send that report to an ontology owner or
policy review instead of silently accepting the new meanings.

## Envelope contract

Each input is strict JSON with four required objects:

- `artifact`: stable `id`, revision-specific `version`, source URI, and SHA-256;
- `provenance`: corpus name, integer token count, vintage, language shares,
  annotator-labor status, notes, source URI, and corpus SHA-256;
- `ontology`: name, version, and a string definition for every concept;
- `behavior`: source URI and SHA-256 for a `strata-cases/v1` input corpus.

Input objects are closed: unknown fields are rejected rather than accepted and
then silently omitted from the report. Extend the contract deliberately before
adding new provenance fields.

Language codes must be unique and their finite shares must total `1.0`.
Annotator labor must be explicitly `credited` or `uncredited`. The comparison
rejects different artifact IDs and ontology names rather than producing a
plausible but meaningless diff. A probe absent from both snapshots is rejected
as `unknown_probe`, so a misspelling cannot masquerade as a stable result.
Every referenced source is read and hashed before comparison. Artifact ID and
version must also agree with the committed `strata-router/v1` bytes.

## Output contract

- `stable` (exit `0`): no concepts were added, removed, or redefined.
- `drift_detected` (exit `2`): the report contains a structural ontology diff
  and the requested concept's definition on both sides.
- `rejected` (exit `1`): an envelope or comparison contract was invalid.

Both verified envelopes are preserved under `envelopes`, so every definition
remains attached to corpus vintage, language distribution, labor provenance,
and the shared case-corpus commitment. The definition probe reports whether the
requested concept changed. The behavior probe contains classifications and
routes produced by executing the two committed artifacts; input envelopes do
not supply those outputs. `verification.commitments_verified` and
`verification.behavior_engine` make that evidence machine-readable.

## Honest boundary

The ontology diff still compares normalized definition text: case and
whitespace-only edits do not count as drift, while any other text change does.
The behavior engine is a deliberately small, deterministic substring router,
not a general model runtime or semantic similarity model. It makes this drift
reproducible and independently checkable without claiming model equivalence.
SHA-256 binds the fixture bytes to their envelopes but does not authenticate who
produced them. Production envelopes need trusted registry identity or
signatures in addition to these integrity commitments.
