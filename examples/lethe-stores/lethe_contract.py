"""Formal cross-store erasure contract for the Lethe proof of concept."""

from __future__ import annotations

import hashlib
import json
import threading
from dataclasses import dataclass, replace
from enum import Enum
from typing import Protocol, Sequence, runtime_checkable


class StoreErasureStatus(str, Enum):
    ERASED = "erased"
    ALREADY_ABSENT = "already_absent"
    FAILED = "failed"
    VERIFICATION_FAILED = "verification_failed"


class ReportStatus(str, Enum):
    COMPLETE = "complete"
    PARTIAL = "partial"
    FAILED = "failed"


class IdempotencyConflict(ValueError):
    """A request ID was reused for a different subject or store."""


@dataclass(frozen=True)
class DeletedRecord:
    memory_id: str
    content_b64: str
    retention_policy_b64: str


@dataclass(frozen=True)
class StoreErasureResult:
    store: str
    request_id: str
    subject_digest: str
    status: StoreErasureStatus
    erased: int
    verified_absent: bool
    receipt: str
    error: str | None = None

    @property
    def complete(self) -> bool:
        return self.verified_absent and self.status in {
            StoreErasureStatus.ERASED,
            StoreErasureStatus.ALREADY_ABSENT,
        }

    def __str__(self) -> str:
        verification = "verified absent" if self.verified_absent else "NOT verified"
        return (
            f"{self.store}: {self.status.value}, {self.erased} erased, "
            f"{verification} — {self.receipt}"
        )

    def to_json(self) -> str:
        return json.dumps(
            {
                "store": self.store,
                "request_id": self.request_id,
                "subject_digest": self.subject_digest,
                "status": self.status.value,
                "erased": self.erased,
                "verified_absent": self.verified_absent,
                "receipt": self.receipt,
                "error": self.error,
            },
            ensure_ascii=True,
            separators=(",", ":"),
            sort_keys=True,
        )

    @classmethod
    def from_json(cls, payload: str) -> StoreErasureResult:
        value = json.loads(payload)
        return cls(
            store=value["store"],
            request_id=value["request_id"],
            subject_digest=value["subject_digest"],
            status=StoreErasureStatus(value["status"]),
            erased=int(value["erased"]),
            verified_absent=bool(value["verified_absent"]),
            receipt=value["receipt"],
            error=value.get("error"),
        )


@dataclass(frozen=True)
class ErasureReport:
    request_id: str
    subject_digest: str
    status: ReportStatus
    stores: tuple[StoreErasureResult, ...]
    receipt: str

    @property
    def complete(self) -> bool:
        return self.status is ReportStatus.COMPLETE


def subject_commitment(subject: str) -> str:
    """Return a POC commitment; production should use a keyed digest."""
    return hashlib.sha256(subject.encode("utf-8")).hexdigest()


def build_store_result(
    store: str,
    request_id: str,
    subject: str,
    deleted: Sequence[DeletedRecord],
    *,
    verified_absent: bool,
) -> StoreErasureResult:
    if not request_id.strip():
        raise ValueError("request_id must not be empty")
    canonical = [
        [item.memory_id, item.content_b64, item.retention_policy_b64]
        for item in sorted(deleted, key=lambda item: item.memory_id)
    ]
    subject_digest = subject_commitment(subject)
    payload = json.dumps(
        {
            "store": store,
            "request_id": request_id,
            "subject_digest": subject_digest,
            "deleted": canonical,
        },
        ensure_ascii=True,
        separators=(",", ":"),
        sort_keys=True,
    ).encode("utf-8")
    digest = hashlib.sha256(payload).hexdigest()
    if verified_absent:
        status = (
            StoreErasureStatus.ERASED if deleted else StoreErasureStatus.ALREADY_ABSENT
        )
    else:
        status = StoreErasureStatus.VERIFICATION_FAILED
    return StoreErasureResult(
        store=store,
        request_id=request_id,
        subject_digest=subject_digest,
        status=status,
        erased=len(deleted),
        verified_absent=verified_absent,
        receipt=f"lethe://store/{store}/{digest}",
    )


def _failure_result(
    store: str, request_id: str, subject_digest: str, error: BaseException
) -> StoreErasureResult:
    error_name = type(error).__name__
    payload = f"{store}\0{request_id}\0{subject_digest}\0{error_name}".encode()
    digest = hashlib.sha256(payload).hexdigest()
    return StoreErasureResult(
        store=store,
        request_id=request_id,
        subject_digest=subject_digest,
        status=StoreErasureStatus.FAILED,
        erased=0,
        verified_absent=False,
        receipt=f"lethe://store/{store}/failed/{digest}",
        error=error_name,
    )


@runtime_checkable
class ErasureAdapter(Protocol):
    """Minimum contract for a store participating in an erasure request."""

    @property
    def name(self) -> str: ...

    def erase_subject(self, subject: str, request_id: str) -> StoreErasureResult: ...

    def verify_subject_absent(self, subject: str) -> bool: ...

    def health(self) -> bool: ...


@runtime_checkable
class ErasureRequestLedger(Protocol):
    """Atomically bind an aggregate request ID before any store is touched."""

    def claim_request(self, request_id: str, subject_digest: str) -> None: ...


class InMemoryRequestLedger:
    """Thread-safe POC ledger for one coordinator process."""

    def __init__(self) -> None:
        self._claims: dict[str, str] = {}
        self._lock = threading.Lock()

    def claim_request(self, request_id: str, subject_digest: str) -> None:
        with self._lock:
            existing = self._claims.setdefault(request_id, subject_digest)
            if existing != subject_digest:
                raise IdempotencyConflict(
                    "aggregate request ID belongs to a different subject"
                )


_PROCESS_REQUEST_LEDGER = InMemoryRequestLedger()


class ErasureCoordinator:
    def __init__(
        self,
        adapters: Sequence[ErasureAdapter],
        request_ledger: ErasureRequestLedger | None = None,
    ):
        if not adapters:
            raise ValueError("at least one erasure adapter is required")
        names = [adapter.name for adapter in adapters]
        if len(names) != len(set(names)):
            raise ValueError("erasure adapter names must be unique")
        self.adapters = tuple(adapters)
        self.request_ledger = request_ledger or _PROCESS_REQUEST_LEDGER

    def erase_subject(self, subject: str, request_id: str) -> ErasureReport:
        if not request_id.strip():
            raise ValueError("request_id must not be empty")
        subject_digest = subject_commitment(subject)
        try:
            self.request_ledger.claim_request(request_id, subject_digest)
        except Exception as error:
            results = [
                _failure_result(adapter.name, request_id, subject_digest, error)
                for adapter in self.adapters
            ]
            return self._report(request_id, subject_digest, results)
        results = []
        for adapter in self.adapters:
            try:
                result = adapter.erase_subject(subject, request_id)
                self._validate_result(adapter, result, request_id, subject_digest)
                verified = adapter.verify_subject_absent(subject)
                if not verified:
                    result = replace(
                        result,
                        status=StoreErasureStatus.VERIFICATION_FAILED,
                        verified_absent=False,
                        error="verification_failed",
                    )
                elif not result.verified_absent:
                    result = replace(result, verified_absent=True)
                results.append(result)
            except Exception as error:  # one store must not hide every other result
                results.append(
                    _failure_result(adapter.name, request_id, subject_digest, error)
                )
        return self._report(request_id, subject_digest, results)

    @staticmethod
    def _validate_result(
        adapter: ErasureAdapter,
        result: StoreErasureResult,
        request_id: str,
        subject_digest: str,
    ) -> None:
        if result.store != adapter.name:
            raise ValueError("adapter returned a result for a different store")
        if result.request_id != request_id:
            raise IdempotencyConflict("adapter returned a different request ID")
        if result.subject_digest != subject_digest:
            raise IdempotencyConflict("request ID belongs to a different subject")

    @staticmethod
    def _report(
        request_id: str,
        subject_digest: str,
        results: Sequence[StoreErasureResult],
    ) -> ErasureReport:
        completed = sum(result.complete for result in results)
        if completed == len(results):
            status = ReportStatus.COMPLETE
        elif completed == 0:
            status = ReportStatus.FAILED
        else:
            status = ReportStatus.PARTIAL
        canonical = [
            {
                "store": result.store,
                "status": result.status.value,
                "erased": result.erased,
                "verified_absent": result.verified_absent,
                "receipt": result.receipt,
                "error": result.error,
            }
            for result in sorted(results, key=lambda result: result.store)
        ]
        payload = json.dumps(
            {
                "request_id": request_id,
                "subject_digest": subject_digest,
                "status": status.value,
                "stores": canonical,
            },
            ensure_ascii=True,
            separators=(",", ":"),
            sort_keys=True,
        ).encode("utf-8")
        digest = hashlib.sha256(payload).hexdigest()
        return ErasureReport(
            request_id=request_id,
            subject_digest=subject_digest,
            status=status,
            stores=tuple(results),
            receipt=f"lethe://request/{request_id}/{digest}",
        )
