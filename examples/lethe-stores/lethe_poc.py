#!/usr/bin/env python3
"""Runnable PostgreSQL+pgvector and Redis adapters for the Lethe POC.

The adapters deliberately use the database CLIs inside Docker Compose. This
keeps the quickstart dependency-free while exercising real store semantics.
They are evidence for an adapter contract, not production database drivers.
"""

from __future__ import annotations

import argparse
import base64
import hashlib
import json
import math
import subprocess
import time
import uuid
from dataclasses import dataclass
from pathlib import Path
from typing import Sequence

HERE = Path(__file__).resolve().parent
COMPOSE_FILE = HERE / "compose.yaml"
SCHEMA_FILE = HERE / "postgres.sql"
REDIS_EXPIRIES = "lethe:expiries"
REDIS_HARD_EXPIRY_GRACE_SECONDS = 60


class StoreCommandError(RuntimeError):
    """A store CLI returned a non-zero exit code."""


@dataclass(frozen=True)
class DeletedMemory:
    memory_id: str
    content_b64: str
    retention_policy_b64: str


@dataclass(frozen=True)
class StoreReceipt:
    store: str
    subject: str
    erased: int
    receipt: str

    def __str__(self) -> str:
        return f"{self.store}: {self.erased} erased — {self.receipt}"


@dataclass(frozen=True)
class SweepOutcome:
    receipt: StoreReceipt
    unreceipted_expirations: int = 0


def _b64(value: str) -> str:
    return base64.b64encode(value.encode("utf-8")).decode("ascii")


def _receipt(
    store: str, subject: str, deleted: Sequence[DeletedMemory]
) -> StoreReceipt:
    canonical = [
        [item.memory_id, item.content_b64, item.retention_policy_b64]
        for item in sorted(deleted, key=lambda item: item.memory_id)
    ]
    payload = json.dumps(
        {"store": store, "subject": subject, "deleted": canonical},
        ensure_ascii=True,
        separators=(",", ":"),
        sort_keys=True,
    ).encode("utf-8")
    digest = hashlib.sha256(payload).hexdigest()
    return StoreReceipt(store, subject, len(deleted), f"lethe://store/{store}/{digest}")


def _parse_deleted(output: str) -> list[DeletedMemory]:
    deleted = []
    for line in output.splitlines():
        if not line.strip():
            continue
        fields = line.split("|")
        if len(fields) != 3:
            raise StoreCommandError(f"unexpected deletion row: {line!r}")
        deleted.append(DeletedMemory(*fields))
    return deleted


def _parse_redis_deleted(lines: Sequence[str]) -> list[DeletedMemory]:
    values = [line for line in lines if line]
    if len(values) % 3:
        raise StoreCommandError(f"unexpected Redis deletion response: {values!r}")
    return [
        DeletedMemory(
            values[index].rsplit(":", 1)[-1], values[index + 1], values[index + 2]
        )
        for index in range(0, len(values), 3)
    ]


class Compose:
    def control(self, *args: str) -> str:
        return self._run(
            "docker", "compose", "-f", str(COMPOSE_FILE), *args, stream=True
        )

    def exec(self, service: str, *args: str, input_text: str | None = None) -> str:
        return self._run(
            "docker",
            "compose",
            "-f",
            str(COMPOSE_FILE),
            "exec",
            "-T",
            service,
            *args,
            input_text=input_text,
            timeout_seconds=30,
        )

    @staticmethod
    def _run(
        *args: str,
        input_text: str | None = None,
        stream: bool = False,
        timeout_seconds: int | None = None,
    ) -> str:
        try:
            completed = subprocess.run(
                args,
                cwd=HERE,
                input=input_text,
                text=True,
                capture_output=not stream,
                check=False,
                timeout=timeout_seconds,
            )
        except FileNotFoundError as error:
            raise StoreCommandError(
                f"required command is missing: {args[0]}"
            ) from error
        except subprocess.TimeoutExpired as error:
            raise StoreCommandError(
                f"{' '.join(args[:8])} … exceeded {timeout_seconds}s"
            ) from error
        if completed.returncode:
            if stream:
                detail = f"exit status {completed.returncode}; see Docker output above"
            else:
                detail = completed.stderr.strip() or completed.stdout.strip()
            raise StoreCommandError(f"{' '.join(args)} failed: {detail}")
        return completed.stdout or ""


class PostgresStore:
    STORE_NAME = "postgres-pgvector"

    def __init__(self, compose: Compose):
        self.compose = compose

    def _psql(self, sql: str, **variables: str) -> str:
        variable_args = [f"--set={name}={value}" for name, value in variables.items()]
        return self.compose.exec(
            "postgres",
            "psql",
            "--username=lethe",
            "--dbname=lethe",
            "--no-psqlrc",
            "--quiet",
            "--tuples-only",
            "--no-align",
            "--field-separator=|",
            "--set=ON_ERROR_STOP=1",
            *variable_args,
            input_text=sql,
        )

    def setup(self) -> None:
        self.compose.exec(
            "postgres",
            "psql",
            "--username=lethe",
            "--dbname=lethe",
            "--no-psqlrc",
            "--set=ON_ERROR_STOP=1",
            input_text=SCHEMA_FILE.read_text(encoding="utf-8"),
        )

    def reset_poc_data(self) -> None:
        self._psql("TRUNCATE TABLE lethe_memories;")

    def remember(
        self,
        subject: str,
        content: str,
        retention_policy: str,
        ttl_seconds: int,
        embedding: Sequence[float],
    ) -> str:
        if ttl_seconds <= 0:
            raise ValueError("ttl_seconds must be positive")
        if len(embedding) != 3 or not all(math.isfinite(value) for value in embedding):
            raise ValueError("the POC expects exactly three finite embedding values")
        memory_id = str(uuid.uuid4())
        vector = "[" + ",".join(str(value) for value in embedding) + "]"
        output = self._psql(
            """
            INSERT INTO lethe_memories
                (id, subject, content, embedding, retention_policy, expires_at)
            VALUES (
                :'memory_id',
                convert_from(decode(:'subject_b64', 'base64'), 'UTF8'),
                convert_from(decode(:'content_b64', 'base64'), 'UTF8'),
                :'embedding'::vector,
                convert_from(decode(:'policy_b64', 'base64'), 'UTF8'),
                clock_timestamp() + make_interval(secs => :'ttl_seconds'::double precision)
            )
            RETURNING id;
            """,
            memory_id=memory_id,
            subject_b64=_b64(subject),
            content_b64=_b64(content),
            embedding=vector,
            policy_b64=_b64(retention_policy),
            ttl_seconds=str(ttl_seconds),
        )
        returned_id = output.strip()
        if returned_id != memory_id:
            raise StoreCommandError(
                f"PostgreSQL returned unexpected id: {returned_id!r}"
            )
        return memory_id

    def forget_subject(self, subject: str) -> StoreReceipt:
        output = self._psql(
            """
            WITH deleted AS (
                DELETE FROM lethe_memories
                WHERE subject = convert_from(decode(:'subject_b64', 'base64'), 'UTF8')
                RETURNING id, content, retention_policy
            )
            SELECT
                id,
                replace(encode(convert_to(content, 'UTF8'), 'base64'), E'\n', ''),
                replace(encode(convert_to(retention_policy, 'UTF8'), 'base64'), E'\n', '')
            FROM deleted
            ORDER BY id;
            """,
            subject_b64=_b64(subject),
        )
        return _receipt(self.STORE_NAME, subject, _parse_deleted(output))

    def sweep(self) -> SweepOutcome:
        output = self._psql("""
            WITH deleted AS (
                DELETE FROM lethe_memories
                WHERE expires_at <= clock_timestamp()
                RETURNING id, content, retention_policy
            )
            SELECT
                id,
                replace(encode(convert_to(content, 'UTF8'), 'base64'), E'\n', ''),
                replace(encode(convert_to(retention_policy, 'UTF8'), 'base64'), E'\n', '')
            FROM deleted
            ORDER BY id;
            """)
        return SweepOutcome(
            _receipt(self.STORE_NAME, "expired", _parse_deleted(output))
        )

    def count_subject(self, subject: str) -> int:
        output = self._psql(
            """
            SELECT count(*)
            FROM lethe_memories
            WHERE subject = convert_from(decode(:'subject_b64', 'base64'), 'UTF8');
            """,
            subject_b64=_b64(subject),
        )
        return int(output.strip())


REDIS_REMEMBER_LUA = """
redis.call('HSET', KEYS[1],
  'subject_b64', ARGV[1],
  'content_b64', ARGV[2],
  'policy_b64', ARGV[3],
  'subject_index', KEYS[2])
redis.call('EXPIRE', KEYS[1], tonumber(ARGV[4]) + tonumber(ARGV[5]))
redis.call('SADD', KEYS[2], KEYS[1])
redis.call('ZADD', KEYS[3], ARGV[6], KEYS[1])
return KEYS[1]
""".strip()

REDIS_FORGET_LUA = """
local members = redis.call('SMEMBERS', KEYS[1])
local result = {}
for _, key in ipairs(members) do
  if redis.call('EXISTS', key) == 1 then
    table.insert(result, key)
    table.insert(result, redis.call('HGET', key, 'content_b64') or '')
    table.insert(result, redis.call('HGET', key, 'policy_b64') or '')
    redis.call('UNLINK', key)
  end
  redis.call('ZREM', KEYS[2], key)
end
redis.call('DEL', KEYS[1])
return result
""".strip()

REDIS_SWEEP_LUA = """
local members = redis.call('ZRANGEBYSCORE', KEYS[1], '-inf', ARGV[1])
local result = {0}
for _, key in ipairs(members) do
  if redis.call('EXISTS', key) == 1 then
    local subject_index = redis.call('HGET', key, 'subject_index')
    table.insert(result, key)
    table.insert(result, redis.call('HGET', key, 'content_b64') or '')
    table.insert(result, redis.call('HGET', key, 'policy_b64') or '')
    redis.call('UNLINK', key)
    if subject_index then
      redis.call('SREM', subject_index, key)
      if redis.call('SCARD', subject_index) == 0 then
        redis.call('DEL', subject_index)
      end
    end
  else
    result[1] = result[1] + 1
  end
  redis.call('ZREM', KEYS[1], key)
end
return result
""".strip()


class RedisStore:
    STORE_NAME = "redis"

    def __init__(self, compose: Compose):
        self.compose = compose

    def _redis(self, *args: str) -> list[str]:
        output = self.compose.exec("redis", "redis-cli", "--raw", *args)
        return output.splitlines()

    @staticmethod
    def _subject_index(subject: str) -> str:
        token = hashlib.sha256(subject.encode("utf-8")).hexdigest()
        return f"lethe:subject:{token}"

    def reset_poc_data(self) -> None:
        keys = [key for key in self._redis("--scan", "--pattern", "lethe:*") if key]
        if keys:
            self._redis("UNLINK", *keys)

    def remember(
        self,
        subject: str,
        content: str,
        retention_policy: str,
        ttl_seconds: int,
    ) -> str:
        if ttl_seconds <= 0:
            raise ValueError("ttl_seconds must be positive")
        memory_id = str(uuid.uuid4())
        key = f"lethe:memory:{memory_id}"
        logical_expiry = int(time.time()) + ttl_seconds
        lines = self._redis(
            "EVAL",
            REDIS_REMEMBER_LUA,
            "3",
            key,
            self._subject_index(subject),
            REDIS_EXPIRIES,
            _b64(subject),
            _b64(content),
            _b64(retention_policy),
            str(ttl_seconds),
            str(REDIS_HARD_EXPIRY_GRACE_SECONDS),
            str(logical_expiry),
        )
        if lines != [key]:
            raise StoreCommandError(f"Redis returned unexpected key: {lines!r}")
        return memory_id

    def forget_subject(self, subject: str) -> StoreReceipt:
        lines = self._redis(
            "EVAL",
            REDIS_FORGET_LUA,
            "2",
            self._subject_index(subject),
            REDIS_EXPIRIES,
        )
        return _receipt(self.STORE_NAME, subject, _parse_redis_deleted(lines))

    def sweep(self, now_epoch_seconds: int | None = None) -> SweepOutcome:
        now = int(time.time()) if now_epoch_seconds is None else now_epoch_seconds
        lines = self._redis("EVAL", REDIS_SWEEP_LUA, "1", REDIS_EXPIRIES, str(now))
        if not lines:
            raise StoreCommandError("Redis sweep returned no result")
        missed = int(lines[0])
        deleted = _parse_redis_deleted(lines[1:])
        return SweepOutcome(_receipt(self.STORE_NAME, "expired", deleted), missed)

    def count_subject(self, subject: str) -> int:
        keys = self._redis("SMEMBERS", self._subject_index(subject))
        count = 0
        for key in keys:
            if key and self._redis("EXISTS", key) == ["1"]:
                count += 1
        return count


def run_demo(compose: Compose) -> None:
    print(
        "Starting PostgreSQL+pgvector and Redis (the first run pulls images) …",
        flush=True,
    )
    compose.control("up", "--detach", "--wait", "--wait-timeout", "120")
    postgres = PostgresStore(compose)
    redis = RedisStore(compose)
    postgres.setup()
    postgres.reset_poc_data()
    redis.reset_poc_data()

    subject = "user:8842"
    policy = "customer-memory-30d-v1"
    memories = [
        ("prefers oat milk", [0.1, 0.2, 0.3]),
        ("delivery address: example street", [0.3, 0.2, 0.1]),
    ]
    for content, embedding in memories:
        postgres.remember(subject, content, policy, 2_592_000, embedding)
        redis.remember(subject, content, policy, 2_592_000)
    postgres.remember(
        "user:kept", "unrelated memory", policy, 2_592_000, [0.0, 0.1, 0.0]
    )
    redis.remember("user:kept", "unrelated memory", policy, 2_592_000)

    print("\nErasing user:8842 from every configured store", flush=True)
    print("  PostgreSQL+pgvector …", flush=True)
    postgres_receipt = postgres.forget_subject(subject)
    print(f"  {postgres_receipt}", flush=True)
    print("  Redis …", flush=True)
    redis_receipt = redis.forget_subject(subject)
    print(f"  {redis_receipt}", flush=True)
    receipts = [postgres_receipt, redis_receipt]

    complete = all(receipt.erased == len(memories) for receipt in receipts)
    complete = complete and postgres.count_subject(subject) == 0
    complete = complete and redis.count_subject(subject) == 0
    unrelated_survives = postgres.count_subject("user:kept") == 1
    unrelated_survives = unrelated_survives and redis.count_subject("user:kept") == 1
    print(f"\nCross-store erasure complete: {'yes' if complete else 'NO'}")
    print(f"Unrelated memory preserved: {'yes' if unrelated_survives else 'NO'}")
    if not complete or not unrelated_survives:
        raise StoreCommandError("the cross-store deletion invariant failed")


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    subparsers = parser.add_subparsers(dest="command", required=True)
    subparsers.add_parser("demo", help="start both stores and run the erasure proof")
    subparsers.add_parser("up", help="start both stores")
    subparsers.add_parser("down", help="stop both stores and preserve their volumes")
    subparsers.add_parser("destroy", help="stop both stores and delete POC volumes")
    return parser


def main() -> int:
    args = build_parser().parse_args()
    compose = Compose()
    try:
        if args.command == "demo":
            run_demo(compose)
        elif args.command == "up":
            compose.control("up", "--detach", "--wait")
        elif args.command == "down":
            compose.control("down")
        elif args.command == "destroy":
            compose.control("down", "--volumes")
    except StoreCommandError as error:
        print(f"error: {error}")
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
