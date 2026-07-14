# Digest seam POC

This proof puts a machine contract around unreliable model output. It repairs
only named syntactic defects, converts unresolved meaning into structured
questions, and applies answers only to paths it previously questioned.

## Run the proof

From the repository root:

```sh
cargo run --quiet -p digest-poc -- demo
```

The first pass emits `"status":"clarify"` and exit status `2`. The second
applies `$.qty=2`, records that decision separately from parser repairs, and
emits `"status":"resolved"` with exit status `0`.

## Put your own model output through the seam

```sh
cargo run --quiet -p digest-poc -- check \
  --schema examples/digest-poc/order.schema.json \
  --input examples/digest-poc/model-output.txt
```

Or pipe output directly from an agent or model call:

```sh
printf '%s' '{"item":"espresso","qty":"2 or 3"}' | \
  cargo run --quiet -p digest-poc -- check \
  --schema examples/digest-poc/order.schema.json
```

Route the emitted question to a human or policy layer, then replay the
original output with the answer:

```sh
cargo run --quiet -p digest-poc -- answer \
  --schema examples/digest-poc/order.schema.json \
  --input examples/digest-poc/model-output.txt \
  --answer '$.qty=2'
```

Answers are strict JSON values, so a string answer is written as
`--answer '$.size="double"'`. Multiple `--answer` arguments are allowed.

## Contract

Every invocation writes exactly one JSON object for automation:

- `resolved` (exit `0`): `value` is safe to hand to deterministic code.
- `clarify` (exit `2`): `questions` contains paths, prompts, and candidates.
- `rejected` (exit `1`): schema, output, or answer was invalid.

`repairs` contains a stable code and human description for every transform.
`answers` is a separate decision ledger; clarification is never disguised as
parser repair.

## Supported JSON Schema subset

The POC supports strings, booleans, numbers with `minimum`/`maximum`, arrays
with `items`, objects with `properties`/`required`, and string `enum`s. Empty
schemas mean `any`. Unsupported constraint keywords are rejected rather than
silently ignored. Object property names cannot contain `.` or `[` because the
POC uses simple `$.field` answer paths.

This is intentionally not a complete JSON Schema implementation. It is the
smallest proof of the runtime seam and its escalation contract.
