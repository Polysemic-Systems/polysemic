# Strata provenance and ontology-drift POC

This proof makes ontology change inspectable without detaching it from the
corpora that produced each version. It compares two provenance envelopes,
reports added, removed, and redefined concepts, and probes one concept on both
sides of the version boundary. It also replays the same recorded case through
both artifact snapshots so a changed classification and downstream route are
observable behavior, not an inference from edited prose.

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
- the same `chosen-caregiver` case changes from `other` / `manual-review` to
  `chosen` / `family-support`.
- SHA-256 commitments resolve to committed artifact and corpus-manifest files;
  `verify_commitments.py` recomputes all four.

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

Each input is strict JSON with three required objects:

- `artifact`: stable `id`, revision-specific `version`, source URI, and SHA-256;
- `provenance`: corpus name, integer token count, vintage, language shares,
  annotator-labor status, notes, source URI, and corpus SHA-256;
- `ontology`: name, version, and a string definition for every concept.
- `behavior`: named cases with a concrete input, classification, and route.

Input objects are closed: unknown fields are rejected rather than accepted and
then silently omitted from the report. Extend the contract deliberately before
adding new provenance fields.

Language codes must be unique and their finite shares must total `1.0`.
Annotator labor must be explicitly `credited` or `uncredited`. The comparison
rejects different artifact IDs and ontology names rather than producing a
plausible but meaningless diff. A probe absent from both snapshots is rejected
as `unknown_probe`, so a misspelling cannot masquerade as a stable result.

## Output contract

- `stable` (exit `0`): no concepts were added, removed, or redefined.
- `drift_detected` (exit `2`): the report contains a structural ontology diff
  and the requested concept's definition on both sides.
- `rejected` (exit `1`): an envelope or comparison contract was invalid.

Both complete accepted input envelopes are preserved under `envelopes`, so every
definition remains attached to corpus vintage, language distribution, and
labor provenance. The definition probe reports whether the requested concept
changed. The behavior probe requires identical case input on both sides and
reports the concrete classification and route change.

## Honest boundary

The ontology diff still compares normalized definition text: case and
whitespace-only edits do not count as drift, while any other text change does.
The separate behavior probe demonstrates a recorded decision change; it is not
a general semantic similarity model. SHA-256 binds the fixture bytes to their
envelopes but does not authenticate who produced them. Production envelopes
need trusted registry identity or signatures in addition to these integrity
commitments.
