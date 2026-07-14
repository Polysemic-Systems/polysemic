import unittest

from lethe_contract import (
    DeletedRecord,
    ErasureCoordinator,
    ReportStatus,
    StoreErasureResult,
    StoreErasureStatus,
    build_store_result,
)


class StubAdapter:
    def __init__(self, name, result=None, error=None, verified=True):
        self._name = name
        self.result = result
        self.error = error
        self.verified = verified

    @property
    def name(self):
        return self._name

    def erase_subject(self, _subject, _request_id):
        if self.error:
            raise self.error
        return self.result

    def verify_subject_absent(self, _subject):
        return self.verified

    def health(self):
        return True


def completed(store="postgres", request_id="request-1", subject="user:1"):
    return build_store_result(
        store,
        request_id,
        subject,
        [DeletedRecord("id-1", "Y29udGVudA==", "cG9saWN5")],
        verified_absent=True,
    )


class ContractTests(unittest.TestCase):
    def test_result_serialization_round_trips(self):
        result = completed()

        self.assertEqual(StoreErasureResult.from_json(result.to_json()), result)

    def test_all_verified_stores_produce_a_complete_aggregate(self):
        postgres = StubAdapter("postgres", completed("postgres"))
        redis = StubAdapter("redis", completed("redis"))

        report = ErasureCoordinator([postgres, redis]).erase_subject(
            "user:1", "request-1"
        )
        reordered = ErasureCoordinator([redis, postgres]).erase_subject(
            "user:1", "request-1"
        )

        self.assertEqual(report.status, ReportStatus.COMPLETE)
        self.assertTrue(report.complete)
        self.assertEqual(report.receipt, reordered.receipt)

    def test_one_failed_store_produces_a_partial_report(self):
        postgres = StubAdapter("postgres", completed("postgres"))
        redis = StubAdapter("redis", error=TimeoutError("offline"))

        report = ErasureCoordinator([postgres, redis]).erase_subject(
            "user:1", "request-1"
        )

        self.assertEqual(report.status, ReportStatus.PARTIAL)
        self.assertFalse(report.complete)
        self.assertEqual(report.stores[1].status, StoreErasureStatus.FAILED)
        self.assertEqual(report.stores[1].error, "TimeoutError")

    def test_no_verified_store_produces_a_failed_report(self):
        report = ErasureCoordinator(
            [StubAdapter("postgres", error=RuntimeError("offline"))]
        ).erase_subject("user:1", "request-1")

        self.assertEqual(report.status, ReportStatus.FAILED)

    def test_post_delete_verification_can_overrule_a_success_receipt(self):
        adapter = StubAdapter("postgres", completed("postgres"), verified=False)

        report = ErasureCoordinator([adapter]).erase_subject("user:1", "request-1")

        self.assertEqual(report.status, ReportStatus.FAILED)
        self.assertEqual(
            report.stores[0].status, StoreErasureStatus.VERIFICATION_FAILED
        )
        self.assertFalse(report.stores[0].verified_absent)

    def test_duplicate_adapter_names_are_rejected(self):
        with self.assertRaisesRegex(ValueError, "unique"):
            ErasureCoordinator(
                [
                    StubAdapter("same", completed("same")),
                    StubAdapter("same", completed("same")),
                ]
            )


if __name__ == "__main__":
    unittest.main()
