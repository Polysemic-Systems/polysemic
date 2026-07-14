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

Expected shape:

```text
Erasing user:8842 from every configured store
  postgres-pgvector: 2 erased — lethe://store/postgres-pgvector/…
  redis: 2 erased — lethe://store/redis/…

Cross-store erasure complete: yes
Unrelated memory preserved: yes
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
transactions, cryptographic signing, idempotency keys, and store-specific backup
erasure policies.

The selection evidence and remaining interview questions are recorded in
[`../../docs/store-selection.md`](../../docs/store-selection.md).
