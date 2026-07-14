# Lethe store POC: PostgreSQL+pgvector and Redis

Erase one subject from two real stores and receive a receipt from each. The
demo needs Docker and Python 3; it installs no Python or Rust packages.

## Five-minute run

From the repository root:

```sh
python3 examples/lethe-stores/lethe_poc.py demo
```

The command starts version-pinned containers bound only to localhost, creates
the PostgreSQL schema, seeds the same subject into PostgreSQL+pgvector and
Redis, erases it from both, verifies that an unrelated subject survived, and
prints two SHA-256 receipts.

The first run pulls both container images and shows Docker's progress. Once the
images are local, subsequent runs normally start in seconds. Service health has
a 120-second startup timeout, so a broken container does not look like a hang.
Individual adapter commands have a 30-second timeout and the demo names the
store it is operating on, so a client-side failure has a visible boundary.

Expected shape:

```text
Erasing user:8842 from every configured store (demo-…)
  postgres-pgvector: erased, 2 erased, verified absent — lethe://store/postgres-pgvector/…
  redis: erased, 2 erased, verified absent — lethe://store/redis/…
  aggregate: complete — lethe://request/demo-…/…

Cross-store erasure complete: yes
Idempotent request replay: yes
Conflicting subject protected: yes
Concurrent aggregate conflict protected: yes
Unrelated memory preserved: yes
```

## LangGraph example

The optional example uses a real LangGraph `StateGraph` but no language model,
API key, or hosted service. It writes the same agent preference to both stores,
coordinates deletion, replays the request ID, verifies absence, and routes any
partial failure to a `human_review` outcome.

```sh
python3 -m pip install -r examples/lethe-stores/requirements-langgraph.txt
python3 examples/lethe-stores/langgraph_example.py
```

Expected ending:

```text
aggregate: complete — lethe://request/langgraph-…/…
idempotent replay: yes
graph outcome: erasure_complete
```

Stop the containers while retaining their local volumes:

```sh
python3 examples/lethe-stores/lethe_poc.py down
```

Delete the POC volumes too:

```sh
python3 examples/lethe-stores/lethe_poc.py destroy
```

## What the POC proves

- PostgreSQL uses `DELETE … RETURNING` so the receipt commits only to rows the
  database reports as deleted. A `vector(3)` column proves the same lifecycle
  applies when pgvector holds the embedding beside the memory.
- Redis assigns every memory a logical expiry and a hard native TTL. A scheduled
  Lua sweep deletes at logical expiry and can issue a receipt; native expiry is
  delayed by a 60-second safety window.
- Explicit subject erasure returns one independent receipt per store. The caller
  can require every configured store to report success before declaring the
  request complete.
- A typed `ErasureAdapter` contract re-verifies absence after every store call.
  `ErasureCoordinator` reports `complete`, `partial`, or `failed` and commits all
  store outcomes into one aggregate receipt.
- Reusing a request ID returns the original store and aggregate receipts. Reusing
  it for another subject is rejected as an idempotency conflict.
- Before either adapter is called, the coordinator atomically binds the aggregate
  request ID to its subject in PostgreSQL's shared `lethe_erasure_intents`
  ledger. Concurrent real-store POC processes therefore agree on one subject;
  the generic contract uses a process-wide locked ledger by default.
- Each store atomically reserves a new request ID for its subject before touching
  memory data. A concurrent caller that loses that reservation cannot delete a
  different subject before discovering the conflict.
- A Redis receipt proves removal from the logical keyspace. Because `UNLINK`
  reclaims allocations asynchronously, it does not prove immediate physical
  memory reclamation.

## Honest limitations

This is an executable adapter-contract experiment, not a production driver.
It shells into dedicated Docker Compose services, uses fixed local credentials,
has no authentication or retry policy, and does not cover replicas, WAL/AOF,
snapshots, object storage, or backups. Redis can hard-expire a key before the
sweeper observes it; that condition is counted as an **unreceipted expiration**
rather than presented as proof. A production adapter should use native clients,
atomic deletion-and-ledger transactions, cryptographic signing, and
store-specific backup erasure policies.

PostgreSQL is the POC's aggregate intent authority, so its availability gates
all cross-store erasure before deletion begins. A production control plane
would make that ledger durable and highly available rather than silently fall
back to independent store claims.

Successful request results are stored in PostgreSQL and Redis for replay. A
process crash after the aggregate claim or a store reservation can leave a
request visibly in progress, and a crash between deletion and result
persistence can still lose the original receipt. Production adapters must make
that state machine recoverable and the deletion-to-receipt transition atomic.
Subject commitments are plain SHA-256 in the POC; production should use a keyed
digest to resist guessing.

The selection evidence and remaining interview questions are recorded in
[`../../docs/store-selection.md`](../../docs/store-selection.md). The typed
status, idempotency, and aggregation rules are specified in the
[`erasure contract`](../../docs/erasure-contract.md).
