import importlib.util
import sys
import unittest
from pathlib import Path

MODULE_PATH = Path(__file__).with_name("lethe_poc.py")
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
        return self.responses.pop(0) if self.responses else ""


class ReceiptTests(unittest.TestCase):
    def test_receipt_is_stable_across_store_return_order(self):
        first = lethe_poc.DeletedMemory("b", "Yg==", "cDE=")
        second = lethe_poc.DeletedMemory("a", "YQ==", "cDE=")

        left = lethe_poc._receipt("postgres", "user:1", [first, second])
        right = lethe_poc._receipt("postgres", "user:1", [second, first])

        self.assertEqual(left, right)
        self.assertEqual(left.erased, 2)
        self.assertTrue(left.receipt.startswith("lethe://store/postgres/"))

    def test_postgres_delete_rows_become_a_receipt(self):
        compose = FakeCompose(["id-1|Y29udGVudA==|cG9saWN5\n"])
        store = lethe_poc.PostgresStore(compose)

        receipt = store.forget_subject("user:1")

        self.assertEqual(receipt.erased, 1)
        self.assertEqual(receipt.store, "postgres-pgvector")
        command = compose.calls[0][1]
        self.assertIn("--set=subject_b64=dXNlcjox", command)
        self.assertNotIn("--command", command)
        self.assertIn(":'subject_b64'", compose.calls[0][2])

    def test_redis_subject_index_does_not_expose_the_subject(self):
        index = lethe_poc.RedisStore._subject_index("user:private")

        self.assertTrue(index.startswith("lethe:subject:"))
        self.assertNotIn("private", index)

    def test_redis_delete_response_becomes_a_receipt(self):
        compose = FakeCompose(["lethe:memory:id-1\nY29udGVudA==\ncG9saWN5\n"])
        store = lethe_poc.RedisStore(compose)

        receipt = store.forget_subject("user:1")

        self.assertEqual(receipt.erased, 1)
        self.assertEqual(receipt.store, "redis")
        self.assertIn("UNLINK", compose.calls[0][1][3])

    def test_redis_sweep_reports_memories_lost_to_hard_expiry(self):
        compose = FakeCompose(["2\nlethe:memory:id-1\nYQ==\ncDE=\n"])
        store = lethe_poc.RedisStore(compose)

        outcome = store.sweep(now_epoch_seconds=100)

        self.assertEqual(outcome.receipt.erased, 1)
        self.assertEqual(outcome.unreceipted_expirations, 2)

    def test_malformed_store_output_is_rejected(self):
        with self.assertRaises(lethe_poc.StoreCommandError):
            lethe_poc._parse_deleted("missing|field")


if __name__ == "__main__":
    unittest.main()
