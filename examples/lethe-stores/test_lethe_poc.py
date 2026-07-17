import importlib.util
import sys
import unittest
from pathlib import Path
from unittest import mock

MODULE_PATH = Path(__file__).with_name("lethe_poc.py")
sys.path.insert(0, str(MODULE_PATH.parent))
SPEC = importlib.util.spec_from_file_location("lethe_poc", MODULE_PATH)
assert SPEC and SPEC.loader
lethe_poc = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = lethe_poc
SPEC.loader.exec_module(lethe_poc)


class FakeCompose:
    def __init__(self, responses=None):
        self.responses = list(responses or [])
        self.calls = []

    def exec(self, service, *args, input_text=None):
        self.calls.append((service, args, input_text))
        if not self.responses:
            return ""
        response = self.responses.pop(0)
        if callable(response):
            return response(self, service, args, input_text)
        return response


def echo_psql_saved_result(compose, _service, _args, _input_text):
    for _service, args, _input_text in reversed(compose.calls):
        for argument in args:
            if argument.startswith("--set=result_b64="):
                return f"{argument.removeprefix('--set=result_b64=')}\n"
    raise AssertionError("no saved PostgreSQL result found")


def echo_psql_reservation(_compose, _service, args, _input_text):
    variables = {
        argument.removeprefix("--set=").split("=", 1)[0]: argument.split("=", 2)[-1]
        for argument in args
        if argument.startswith("--set=")
    }
    return f"{variables['subject_digest']}||{variables['reservation_token']}\n"


def echo_redis_saved_result(_compose, _service, args, _input_text):
    return f"{args[-1]}\n"


def echo_redis_reservation(_compose, _service, args, _input_text):
    return f"{args[-2]}\n\n{args[-1]}\n"


class ReceiptTests(unittest.TestCase):
    def test_receipt_is_stable_across_store_return_order(self):
        first = lethe_poc.DeletedMemory("b", "Yg==", "cDE=")
        second = lethe_poc.DeletedMemory("a", "YQ==", "cDE=")

        left = lethe_poc._receipt(
            "postgres",
            "request-1",
            "user:1",
            [first, second],
            verified_absent=True,
        )
        right = lethe_poc._receipt(
            "postgres",
            "request-1",
            "user:1",
            [second, first],
            verified_absent=True,
        )

        self.assertEqual(left, right)
        self.assertEqual(left.erased, 2)
        self.assertTrue(left.receipt.startswith("lethe://store/postgres/"))

    def test_request_id_cannot_be_replayed_for_another_subject(self):
        receipt = lethe_poc._receipt(
            "postgres",
            "request-1",
            "user:1",
            [],
            verified_absent=True,
        )

        with self.assertRaises(lethe_poc.IdempotencyConflict):
            lethe_poc._validate_replay(receipt, "postgres", "request-1", "user:other")

    def test_postgres_delete_rows_become_a_receipt(self):
        compose = FakeCompose(
            [
                echo_psql_reservation,
                "id-1|Y29udGVudA==|cG9saWN5\n",
                "0\n",
                echo_psql_saved_result,
            ]
        )
        store = lethe_poc.PostgresStore(compose)

        receipt = store.erase_subject("user:1", "request-1")

        self.assertEqual(receipt.erased, 1)
        self.assertEqual(receipt.store, "postgres-pgvector")
        self.assertTrue(receipt.verified_absent)
        command = compose.calls[1][1]
        self.assertIn("--set=subject_b64=dXNlcjox", command)
        self.assertNotIn("--command", command)
        self.assertIn(":'subject_b64'", compose.calls[1][2])

        compose.responses.append(
            f"{receipt.subject_digest}|{lethe_poc._b64(receipt.to_json())}|\n"
        )
        self.assertEqual(store.erase_subject("user:1", "request-1"), receipt)

    def test_postgres_sweep_receipt_verifies_no_expired_rows_remain(self):
        compose = FakeCompose(["id-1|YQ==|cDE=\n", "0\n"])
        store = lethe_poc.PostgresStore(compose)

        outcome = store.sweep()

        self.assertEqual(outcome.receipt.erased, 1)
        self.assertTrue(outcome.receipt.verified_absent)
        self.assertIn("expires_at <= clock_timestamp()", compose.calls[-1][2])

    def test_postgres_sweep_receipt_reports_surviving_expired_rows_as_unverified(self):
        compose = FakeCompose(["id-1|YQ==|cDE=\n", "1\n"])
        store = lethe_poc.PostgresStore(compose)

        outcome = store.sweep()

        self.assertFalse(outcome.receipt.verified_absent)

    def test_postgres_aggregate_claim_precedes_store_deletion(self):
        digest = lethe_poc.subject_commitment("user:1")
        compose = FakeCompose([f"{digest}\n"])
        store = lethe_poc.PostgresStore(compose)

        store.claim_request("request-1", digest)

        self.assertEqual(len(compose.calls), 1)
        self.assertIn("lethe_erasure_intents", compose.calls[0][2])
        self.assertNotIn("DELETE FROM lethe_memories", compose.calls[0][2])

    def test_postgres_aggregate_claim_rejects_another_subject(self):
        other_digest = lethe_poc.subject_commitment("user:other")
        compose = FakeCompose([f"{other_digest}\n"])
        store = lethe_poc.PostgresStore(compose)

        with self.assertRaises(lethe_poc.IdempotencyConflict):
            store.claim_request("request-1", lethe_poc.subject_commitment("user:1"))

    def test_postgres_conflict_is_rejected_before_delete(self):
        other_digest = lethe_poc.subject_commitment("user:other")
        compose = FakeCompose([f"{other_digest}||another-owner\n"])
        store = lethe_poc.PostgresStore(compose)

        with self.assertRaises(lethe_poc.IdempotencyConflict):
            store.erase_subject("user:1", "request-1")

        self.assertEqual(len(compose.calls), 1)
        self.assertNotIn("DELETE FROM lethe_memories", compose.calls[0][2])

    def test_redis_subject_index_does_not_expose_the_subject(self):
        index = lethe_poc.RedisStore._subject_index("user:private")

        self.assertTrue(index.startswith("lethe:subject:"))
        self.assertNotIn("private", index)

    def test_redis_delete_response_becomes_a_receipt(self):
        compose = FakeCompose(
            [
                echo_redis_reservation,
                "lethe:memory:id-1\nY29udGVudA==\ncG9saWN5\n",
                "",
                echo_redis_saved_result,
            ]
        )
        store = lethe_poc.RedisStore(compose)

        receipt = store.erase_subject("user:1", "request-1")

        self.assertEqual(receipt.erased, 1)
        self.assertEqual(receipt.store, "redis")
        self.assertTrue(receipt.verified_absent)
        self.assertTrue(
            any("UNLINK" in call[1][3] for call in compose.calls if len(call[1]) > 3)
        )

        compose.responses.append(
            f"{receipt.subject_digest}\n{lethe_poc._b64(receipt.to_json())}\n\n"
        )
        self.assertEqual(store.erase_subject("user:1", "request-1"), receipt)

    def test_redis_conflict_is_rejected_before_delete(self):
        other_digest = lethe_poc.subject_commitment("user:other")
        compose = FakeCompose([f"{other_digest}\n\nanother-owner\n"])
        store = lethe_poc.RedisStore(compose)

        with self.assertRaises(lethe_poc.IdempotencyConflict):
            store.erase_subject("user:1", "request-1")

        self.assertEqual(len(compose.calls), 1)
        self.assertIn(lethe_poc.REDIS_RESERVE_ERASURE_LUA, compose.calls[0][1])
        self.assertNotIn(lethe_poc.REDIS_FORGET_LUA, compose.calls[0][1])

    def test_redis_sweep_reports_memories_lost_to_hard_expiry(self):
        compose = FakeCompose(["2\nlethe:memory:id-1\nYQ==\ncDE=\n", "0", "0"])
        store = lethe_poc.RedisStore(compose)

        outcome = store.sweep(now_epoch_seconds=100)

        self.assertEqual(outcome.receipt.erased, 1)
        self.assertEqual(outcome.unreceipted_expirations, 2)
        self.assertTrue(outcome.receipt.verified_absent)
        exists_call = compose.calls[-1][1]
        self.assertIn("EXISTS", exists_call)
        self.assertIn("lethe:memory:id-1", exists_call)

    def test_redis_sweep_receipt_reports_a_surviving_key_as_unverified(self):
        compose = FakeCompose(["0\nlethe:memory:id-1\nYQ==\ncDE=\n", "0", "1"])
        store = lethe_poc.RedisStore(compose)

        outcome = store.sweep(now_epoch_seconds=100)

        self.assertFalse(outcome.receipt.verified_absent)

    def test_redis_sweep_receipt_reports_a_still_due_index_as_unverified(self):
        compose = FakeCompose(["0\nlethe:memory:id-1\nYQ==\ncDE=\n", "1"])
        store = lethe_poc.RedisStore(compose)

        outcome = store.sweep(now_epoch_seconds=100)

        self.assertFalse(outcome.receipt.verified_absent)

    def test_malformed_store_output_is_rejected(self):
        with self.assertRaises(lethe_poc.StoreCommandError):
            lethe_poc._parse_deleted("missing|field")

    def test_store_command_timeout_names_the_boundary(self):
        expired = lethe_poc.subprocess.TimeoutExpired(["docker", "compose"], 30)

        with mock.patch.object(lethe_poc.subprocess, "run", side_effect=expired):
            with self.assertRaisesRegex(lethe_poc.StoreCommandError, "exceeded 30s"):
                lethe_poc.Compose._run(
                    "docker", "compose", input_text="PING", timeout_seconds=30
                )


if __name__ == "__main__":
    unittest.main()
