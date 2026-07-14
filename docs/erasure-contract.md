# Lethe erasure adapter contract — POC v1

The contract coordinates deletion across stores without treating one successful
`DELETE` as proof that the whole system forgot. Its implementation lives in
the Rust open core at [`crates/lethe/src/store.rs`](../crates/lethe/src/store.rs).
The executable PostgreSQL, Redis, and LangGraph POCs mirror it in
[`examples/lethe-stores/lethe_contract.py`](../examples/lethe-stores/lethe_contract.py).

The two implementations deliberately share the state machine and field
contracts rather than the receipt hash algorithm. The dependency-free Rust core
uses its existing FNV-1a POC commitment; the Python store proof uses SHA-256.
Production deployments should replace both with a versioned, keyed digest.

## Request

Every erasure operation has two inputs:

- `subject`: the store-specific deletion scope, such as a user identifier.
- `request_id`: a caller-generated idempotency key unique to that intent.

Receipts contain a SHA-256 subject commitment rather than the raw subject. This
is sufficient for the POC; a production service should use a keyed digest so
low-entropy identifiers cannot be guessed offline.

## Adapter protocol

Each `ErasureAdapter` exposes:

| Method | Contract |
|---|---|
| `health()` | Confirm that the adapter can reach its authoritative store. |
| `erase_subject(subject, request_id)` | Delete or replay a previously completed request and return a store result. |
| `verify_subject_absent(subject)` | Query the store after deletion; never infer absence from a delete count. |

Rust adapters also declare `StoreCapabilities`: native TTL support, vector-value
support, and whether scheduled sweeps can produce audit evidence. Policy can
therefore reject a store whose deletion behavior is weaker than the requested
retention contract.

An adapter has one unique `name`. Results returned under another store name,
request ID, or subject commitment are rejected.

## Store result states

| Status | Meaning |
|---|---|
| `erased` | One or more records were deleted and absence was verified. |
| `already_absent` | No records existed and absence was verified. |
| `verification_failed` | The delete call returned, but the subject remains observable. |
| `failed` | The adapter raised, timed out, conflicted, or returned an invalid result. |

Errors expose only their class name in aggregate reports. Store command text,
credentials, subjects, and deleted content do not enter the public error field.

## Aggregate result states

`ErasureCoordinator` calls every configured adapter even when one fails:

- `complete`: every store result is verified complete.
- `partial`: at least one store completed and at least one did not.
- `failed`: no store produced a verified-complete result.

The aggregate receipt commits the request ID, subject commitment, status, and
every store receipt in store-name order. Adapter ordering therefore cannot
change the aggregate receipt.

## Idempotency

PostgreSQL stores successful results in `lethe_erasure_requests`. Redis stores
them under a hashed `lethe:erasure:*` key.

- Replaying the same request ID for the same subject returns the original result.
- Reusing it for another subject produces an `IdempotencyConflict` failure.
- Every replay queries the store again. If new memory appeared after the original
  deletion, the coordinator changes the result to `verification_failed` rather
  than presenting the historical receipt as current proof.

## POC atomicity boundary

Deletion and result-ledger persistence are separate client operations in this
POC. A process crash between them can delete data without preserving the
original receipt. Production adapters must combine those operations atomically
or persist a recoverable intent before deletion. Replicas, WAL/AOF, snapshots,
exports, object storage, and backups remain separate erasure participants.
