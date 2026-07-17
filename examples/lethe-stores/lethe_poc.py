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
import math
import subprocess
import time
import uuid
from concurrent.futures import ThreadPoolExecutor
from dataclasses import dataclass
from pathlib import Path
from typing import Sequence

from lethe_contract import (
    DeletedRecord as DeletedMemory,
    ErasureCoordinator,
    IdempotencyConflict,
    ReportStatus,
    StoreErasureResult,
    build_store_result,
    subject_commitment,
)

HERE = Path(__file__).resolve().parent
COMPOSE_FILE = HERE / "compose.yaml"
SCHEMA_FILE = HERE / "postgres.sql"
REDIS_EXPIRIES = "lethe:expiries"
REDIS_HARD_EXPIRY_GRACE_SECONDS = 60


class StoreCommandError(RuntimeError):
    """A store CLI returned a non-zero exit code."""


@dataclass(frozen=True)
class SweepOutcome:
    receipt: StoreErasureResult
    unreceipted_expirations: int = 0


def _b64(value: str) -> str:
    return base64.b64encode(value.encode("utf-8")).decode("ascii")


def _from_b64(value: str) -> str:
    return base64.b64decode(value).decode("utf-8")


def _receipt(
    store: str,
    request_id: str,
    subject: str,
    deleted: Sequence[DeletedMemory],
    *,
    verified_absent: bool,
) -> StoreErasureResult:
    return build_store_result(
        store,
        request_id,
        subject,
        deleted,
        verified_absent=verified_absent,
    )


def _validate_replay(
    result: StoreErasureResult, store: str, request_id: str, subject: str
) -> None:
    if result.store != store or result.request_id != request_id:
        raise IdempotencyConflict("stored erasure result belongs to another request")
    if result.subject_digest != subject_commitment(subject):
        raise IdempotencyConflict("request ID belongs to a different subject")


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

    @property
    def name(self) -> str:
        return self.STORE_NAME

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
        self._psql(
            "TRUNCATE TABLE lethe_memories, lethe_erasure_requests, "
            "lethe_erasure_intents;"
        )

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

    def _load_erasure(self, subject: str, request_id: str) -> StoreErasureResult | None:
        output = self._psql(
            """
            SELECT replace(
                encode(convert_to(result_json, 'UTF8'), 'base64'),
                E'\n',
                ''
            )
            FROM lethe_erasure_requests
            WHERE store = :'store'
              AND request_id = convert_from(decode(:'request_id_b64', 'base64'), 'UTF8')
              AND result_json IS NOT NULL;
            """,
            store=self.STORE_NAME,
            request_id_b64=_b64(request_id),
        ).strip()
        if not output:
            return None
        result = StoreErasureResult.from_json(_from_b64(output))
        _validate_replay(result, self.STORE_NAME, request_id, subject)
        return result

    def _reserve_erasure(
        self, subject: str, request_id: str
    ) -> tuple[StoreErasureResult | None, str]:
        subject_digest = subject_commitment(subject)
        reservation_token = str(uuid.uuid4())
        output = self._psql(
            """
            INSERT INTO lethe_erasure_requests
                (store, request_id, subject_digest, reservation_token)
            VALUES (
                :'store',
                convert_from(decode(:'request_id_b64', 'base64'), 'UTF8'),
                :'subject_digest',
                :'reservation_token'
            )
            ON CONFLICT (store, request_id) DO NOTHING;

            SELECT
                subject_digest,
                COALESCE(
                    replace(
                        encode(convert_to(result_json, 'UTF8'), 'base64'),
                        E'\n',
                        ''
                    ),
                    ''
                ),
                COALESCE(reservation_token, '')
            FROM lethe_erasure_requests
            WHERE store = :'store'
              AND request_id = convert_from(decode(:'request_id_b64', 'base64'), 'UTF8');
            """,
            store=self.STORE_NAME,
            request_id_b64=_b64(request_id),
            subject_digest=subject_digest,
            reservation_token=reservation_token,
        ).strip()
        fields = output.split("|")
        if len(fields) != 3:
            raise StoreCommandError(f"unexpected PostgreSQL reservation: {output!r}")
        stored_digest, result_b64, owner = fields
        if stored_digest != subject_digest:
            raise IdempotencyConflict("request ID belongs to a different subject")
        if result_b64:
            result = StoreErasureResult.from_json(_from_b64(result_b64))
            _validate_replay(result, self.STORE_NAME, request_id, subject)
            return result, reservation_token
        if owner != reservation_token:
            raise StoreCommandError("erasure request is already in progress")
        return None, reservation_token

    def _save_erasure(
        self, subject: str, result: StoreErasureResult, reservation_token: str
    ) -> StoreErasureResult:
        output = self._psql(
            """
            UPDATE lethe_erasure_requests
            SET result_json = convert_from(decode(:'result_b64', 'base64'), 'UTF8'),
                reservation_token = NULL
            WHERE store = :'store'
              AND request_id = convert_from(decode(:'request_id_b64', 'base64'), 'UTF8')
              AND subject_digest = :'subject_digest'
              AND reservation_token = :'reservation_token'
              AND result_json IS NULL
            RETURNING replace(
                encode(convert_to(result_json, 'UTF8'), 'base64'),
                E'\n',
                ''
            );
            """,
            store=self.STORE_NAME,
            request_id_b64=_b64(result.request_id),
            subject_digest=result.subject_digest,
            reservation_token=reservation_token,
            result_b64=_b64(result.to_json()),
        ).strip()
        if not output:
            raise StoreCommandError("PostgreSQL did not persist the erasure result")
        winner = StoreErasureResult.from_json(_from_b64(output))
        _validate_replay(winner, self.STORE_NAME, result.request_id, subject)
        return winner

    def erase_subject(self, subject: str, request_id: str) -> StoreErasureResult:
        existing, reservation_token = self._reserve_erasure(subject, request_id)
        if existing is not None:
            return existing
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
        result = _receipt(
            self.STORE_NAME,
            request_id,
            subject,
            _parse_deleted(output),
            verified_absent=self.verify_subject_absent(subject),
        )
        return self._save_erasure(subject, result, reservation_token)

    def forget_subject(
        self, subject: str, request_id: str | None = None
    ) -> StoreErasureResult:
        return self.erase_subject(subject, request_id or str(uuid.uuid4()))

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
        deleted = _parse_deleted(output)
        # Observe absence instead of trusting the deletion response: a swept
        # row that survived would still satisfy this predicate, because its
        # expiry can only recede further into the past.
        remaining = int(
            self._psql(
                """
                SELECT count(*)
                FROM lethe_memories
                WHERE expires_at <= clock_timestamp();
                """
            ).strip()
        )
        return SweepOutcome(
            _receipt(
                self.STORE_NAME,
                f"sweep:{int(time.time())}",
                "expired",
                deleted,
                verified_absent=remaining == 0,
            )
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

    def verify_subject_absent(self, subject: str) -> bool:
        return self.count_subject(subject) == 0

    def health(self) -> bool:
        return self._psql("SELECT 1;").strip() == "1"

    def claim_request(self, request_id: str, subject_digest: str) -> None:
        """Durably bind an aggregate request before either store can delete."""
        output = self._psql(
            """
            INSERT INTO lethe_erasure_intents (request_id, subject_digest)
            VALUES (
                convert_from(decode(:'request_id_b64', 'base64'), 'UTF8'),
                :'subject_digest'
            )
            ON CONFLICT (request_id) DO NOTHING;

            SELECT subject_digest
            FROM lethe_erasure_intents
            WHERE request_id = convert_from(
                decode(:'request_id_b64', 'base64'),
                'UTF8'
            );
            """,
            request_id_b64=_b64(request_id),
            subject_digest=subject_digest,
        ).strip()
        if output != subject_digest:
            raise IdempotencyConflict(
                "aggregate request ID belongs to a different subject"
            )


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

REDIS_SAVE_ERASURE_LUA = """
if redis.call('HGET', KEYS[1], 'reservation_token') == ARGV[1]
  and not redis.call('HGET', KEYS[1], 'result_b64') then
  redis.call('HSET', KEYS[1], 'result_b64', ARGV[2])
  redis.call('HDEL', KEYS[1], 'reservation_token')
end
return redis.call('HGET', KEYS[1], 'result_b64')
""".strip()

REDIS_RESERVE_ERASURE_LUA = """
local digest = redis.call('HGET', KEYS[1], 'subject_digest')
if digest then
  return {
    digest,
    redis.call('HGET', KEYS[1], 'result_b64') or '',
    redis.call('HGET', KEYS[1], 'reservation_token') or ''
  }
end
redis.call('HSET', KEYS[1],
  'subject_digest', ARGV[1],
  'reservation_token', ARGV[2])
return {ARGV[1], '', ARGV[2]}
""".strip()


class RedisStore:
    STORE_NAME = "redis"

    def __init__(self, compose: Compose):
        self.compose = compose

    @property
    def name(self) -> str:
        return self.STORE_NAME

    def _redis(self, *args: str) -> list[str]:
        output = self.compose.exec("redis", "redis-cli", "--raw", *args)
        return output.splitlines()

    @staticmethod
    def _subject_index(subject: str) -> str:
        token = hashlib.sha256(subject.encode("utf-8")).hexdigest()
        return f"lethe:subject:{token}"

    @staticmethod
    def _erasure_key(request_id: str) -> str:
        token = hashlib.sha256(request_id.encode("utf-8")).hexdigest()
        return f"lethe:erasure:{token}"

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

    def _reserve_erasure(
        self, subject: str, request_id: str
    ) -> tuple[StoreErasureResult | None, str]:
        subject_digest = subject_commitment(subject)
        reservation_token = str(uuid.uuid4())
        lines = self._redis(
            "EVAL",
            REDIS_RESERVE_ERASURE_LUA,
            "1",
            self._erasure_key(request_id),
            subject_digest,
            reservation_token,
        )
        if len(lines) != 3:
            raise StoreCommandError(f"unexpected Redis reservation: {lines!r}")
        stored_digest, result_b64, owner = lines
        if stored_digest != subject_digest:
            raise IdempotencyConflict("request ID belongs to a different subject")
        if result_b64:
            result = StoreErasureResult.from_json(_from_b64(result_b64))
            _validate_replay(result, self.STORE_NAME, request_id, subject)
            return result, reservation_token
        if owner != reservation_token:
            raise StoreCommandError("erasure request is already in progress")
        return None, reservation_token

    def _save_erasure(
        self, subject: str, result: StoreErasureResult, reservation_token: str
    ) -> StoreErasureResult:
        lines = self._redis(
            "EVAL",
            REDIS_SAVE_ERASURE_LUA,
            "1",
            self._erasure_key(result.request_id),
            reservation_token,
            _b64(result.to_json()),
        )
        if len(lines) != 1 or not lines[0]:
            raise StoreCommandError("Redis did not persist the erasure result")
        winner = StoreErasureResult.from_json(_from_b64(lines[0]))
        _validate_replay(winner, self.STORE_NAME, result.request_id, subject)
        return winner

    def erase_subject(self, subject: str, request_id: str) -> StoreErasureResult:
        existing, reservation_token = self._reserve_erasure(subject, request_id)
        if existing is not None:
            return existing
        lines = self._redis(
            "EVAL",
            REDIS_FORGET_LUA,
            "2",
            self._subject_index(subject),
            REDIS_EXPIRIES,
        )
        result = _receipt(
            self.STORE_NAME,
            request_id,
            subject,
            _parse_redis_deleted(lines),
            verified_absent=self.verify_subject_absent(subject),
        )
        return self._save_erasure(subject, result, reservation_token)

    def forget_subject(
        self, subject: str, request_id: str | None = None
    ) -> StoreErasureResult:
        return self.erase_subject(subject, request_id or str(uuid.uuid4()))

    def sweep(self, now_epoch_seconds: int | None = None) -> SweepOutcome:
        now = int(time.time()) if now_epoch_seconds is None else now_epoch_seconds
        lines = self._redis("EVAL", REDIS_SWEEP_LUA, "1", REDIS_EXPIRIES, str(now))
        if not lines:
            raise StoreCommandError("Redis sweep returned no result")
        missed = int(lines[0])
        deleted = _parse_redis_deleted(lines[1:])
        swept_keys = [line for line in lines[1:] if line][::3]
        # Observe absence instead of trusting the deletion response: the
        # swept keys must be gone and no due entry may remain in the index.
        verified = self._redis("ZCOUNT", REDIS_EXPIRIES, "-inf", str(now)) == ["0"]
        if verified and swept_keys:
            verified = self._redis("EXISTS", *swept_keys) == ["0"]
        return SweepOutcome(
            _receipt(
                self.STORE_NAME,
                f"sweep:{now}",
                "expired",
                deleted,
                verified_absent=verified,
            ),
            missed,
        )

    def count_subject(self, subject: str) -> int:
        keys = self._redis("SMEMBERS", self._subject_index(subject))
        count = 0
        for key in keys:
            if key and self._redis("EXISTS", key) == ["1"]:
                count += 1
        return count

    def verify_subject_absent(self, subject: str) -> bool:
        return self.count_subject(subject) == 0

    def health(self) -> bool:
        return self._redis("PING") == ["PONG"]


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
    if not postgres.health() or not redis.health():
        raise StoreCommandError("a configured store failed its health check")

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

    request_id = f"demo-{uuid.uuid4()}"
    coordinator = ErasureCoordinator([postgres, redis], request_ledger=postgres)
    print(f"\nErasing user:8842 from every configured store ({request_id})", flush=True)
    report = coordinator.erase_subject(subject, request_id)
    for result in report.stores:
        print(f"  {result}", flush=True)
    print(f"  aggregate: {report.status.value} — {report.receipt}", flush=True)

    replay = coordinator.erase_subject(subject, request_id)
    idempotent = replay == report
    conflict = coordinator.erase_subject("user:kept", request_id)
    conflict_protected = conflict.status is ReportStatus.FAILED
    complete = report.complete and all(
        result.erased == len(memories) for result in report.stores
    )
    complete = complete and postgres.verify_subject_absent(subject)
    complete = complete and redis.verify_subject_absent(subject)
    unrelated_survives = postgres.count_subject("user:kept") == 1
    unrelated_survives = unrelated_survives and redis.count_subject("user:kept") == 1

    race_subjects = ("user:race-a", "user:race-b")
    for race_subject in race_subjects:
        postgres.remember(
            race_subject, "race sentinel", policy, 2_592_000, [0.0, 0.0, 0.1]
        )
        redis.remember(race_subject, "race sentinel", policy, 2_592_000)
    race_request = f"race-{uuid.uuid4()}"
    with ThreadPoolExecutor(max_workers=2) as executor:
        race_reports = tuple(
            executor.map(
                lambda race_subject: coordinator.erase_subject(
                    race_subject, race_request
                ),
                race_subjects,
            )
        )
    winners = [
        race_subject
        for race_subject, race_report in zip(race_subjects, race_reports)
        if race_report.complete
    ]
    losers = [
        race_subject
        for race_subject, race_report in zip(race_subjects, race_reports)
        if race_report.status is ReportStatus.FAILED
    ]
    concurrent_conflict_protected = len(winners) == 1 and len(losers) == 1
    if concurrent_conflict_protected:
        winner, loser = winners[0], losers[0]
        concurrent_conflict_protected = postgres.count_subject(winner) == 0
        concurrent_conflict_protected &= redis.count_subject(winner) == 0
        concurrent_conflict_protected &= postgres.count_subject(loser) == 1
        concurrent_conflict_protected &= redis.count_subject(loser) == 1

    print(f"\nCross-store erasure complete: {'yes' if complete else 'NO'}")
    print(f"Idempotent request replay: {'yes' if idempotent else 'NO'}")
    print(f"Conflicting subject protected: {'yes' if conflict_protected else 'NO'}")
    print(
        "Concurrent aggregate conflict protected: "
        f"{'yes' if concurrent_conflict_protected else 'NO'}"
    )
    print(f"Unrelated memory preserved: {'yes' if unrelated_survives else 'NO'}")
    if (
        not complete
        or not idempotent
        or not conflict_protected
        or not concurrent_conflict_protected
        or not unrelated_survives
    ):
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
