//! Store-independent erasure contracts.
//!
//! The in-memory [`crate::Lethe`] type demonstrates lifecycle semantics. This
//! module defines the boundary implemented by durable stores and the coordinator
//! that refuses to call a multi-store request complete until every adapter has
//! verified absence.

use std::error::Error;
use std::fmt;

use super::{fnv1a_extend, FNV_OFFSET};

/// Store behaviors that affect retention and erasure policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StoreCapabilities {
    pub native_ttl: bool,
    pub vector_values: bool,
    pub auditable_sweeps: bool,
}

impl StoreCapabilities {
    pub const fn new(native_ttl: bool, vector_values: bool, auditable_sweeps: bool) -> Self {
        Self {
            native_ttl,
            vector_values,
            auditable_sweeps,
        }
    }
}

/// One idempotent erasure intent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ErasureRequest {
    pub request_id: String,
    pub subject: String,
}

impl ErasureRequest {
    pub fn new(request_id: &str, subject: &str) -> Result<Self, ContractError> {
        if request_id.trim().is_empty() {
            return Err(ContractError::EmptyRequestId);
        }
        if subject.trim().is_empty() {
            return Err(ContractError::EmptySubject);
        }
        Ok(Self {
            request_id: request_id.to_string(),
            subject: subject.to_string(),
        })
    }

    /// POC-only unkeyed commitment. Production control planes should replace
    /// this with a keyed digest so low-entropy subjects cannot be guessed.
    pub fn subject_digest(&self) -> String {
        subject_commitment(&self.subject)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContractError {
    EmptyRequestId,
    EmptySubject,
    NoAdapters,
    EmptyAdapterName,
    DuplicateAdapter(String),
}

impl fmt::Display for ContractError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyRequestId => f.write_str("request_id must not be empty"),
            Self::EmptySubject => f.write_str("subject must not be empty"),
            Self::NoAdapters => f.write_str("at least one erasure adapter is required"),
            Self::EmptyAdapterName => f.write_str("erasure adapter names must not be empty"),
            Self::DuplicateAdapter(name) => write!(f, "duplicate erasure adapter: {name}"),
        }
    }
}

impl Error for ContractError {}

/// A deliberately non-sensitive adapter failure classification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdapterError {
    pub kind: String,
}

impl AdapterError {
    pub fn new(kind: &str) -> Self {
        Self {
            kind: if kind.trim().is_empty() {
                "adapter_error".to_string()
            } else {
                kind.to_string()
            },
        }
    }
}

impl fmt::Display for AdapterError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.kind)
    }
}

impl Error for AdapterError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoreErasureStatus {
    Erased,
    AlreadyAbsent,
    Failed,
    VerificationFailed,
}

impl StoreErasureStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::Erased => "erased",
            Self::AlreadyAbsent => "already_absent",
            Self::Failed => "failed",
            Self::VerificationFailed => "verification_failed",
        }
    }
}

/// One adapter's result. The receipt must commit to the store's deleted records;
/// the coordinator separately re-queries the store and sets `verified_absent`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreErasureResult {
    pub store: String,
    pub request_id: String,
    pub subject_digest: String,
    pub status: StoreErasureStatus,
    pub erased: usize,
    pub verified_absent: bool,
    pub receipt: String,
    pub error: Option<String>,
}

impl StoreErasureResult {
    pub fn successful(store: &str, request: &ErasureRequest, erased: usize, receipt: &str) -> Self {
        Self {
            store: store.to_string(),
            request_id: request.request_id.clone(),
            subject_digest: request.subject_digest(),
            status: if erased == 0 {
                StoreErasureStatus::AlreadyAbsent
            } else {
                StoreErasureStatus::Erased
            },
            erased,
            verified_absent: false,
            receipt: receipt.to_string(),
            error: None,
        }
    }

    pub fn complete(&self) -> bool {
        self.verified_absent
            && matches!(
                self.status,
                StoreErasureStatus::Erased | StoreErasureStatus::AlreadyAbsent
            )
    }
}

/// Minimum durable-store boundary. Implementations own their idempotency ledger:
/// replaying a request ID for the same subject must return the original result,
/// while reuse for another subject must return an error.
pub trait ErasureAdapter {
    fn name(&self) -> &str;
    fn capabilities(&self) -> StoreCapabilities;
    fn health(&mut self) -> Result<(), AdapterError>;
    fn erase_subject(
        &mut self,
        request: &ErasureRequest,
    ) -> Result<StoreErasureResult, AdapterError>;
    fn verify_subject_absent(&mut self, subject: &str) -> Result<bool, AdapterError>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErasureReportStatus {
    Complete,
    Partial,
    Failed,
}

impl ErasureReportStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::Complete => "complete",
            Self::Partial => "partial",
            Self::Failed => "failed",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreErasureReport {
    pub request_id: String,
    pub subject_digest: String,
    pub status: ErasureReportStatus,
    pub stores: Vec<StoreErasureResult>,
    pub receipt: String,
}

impl StoreErasureReport {
    pub fn complete(&self) -> bool {
        self.status == ErasureReportStatus::Complete
    }
}

/// Fans one request out to every configured store and keeps failures local to
/// their adapter. One unavailable store cannot suppress evidence from the rest.
pub struct ErasureCoordinator {
    adapters: Vec<Box<dyn ErasureAdapter>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreDescriptor {
    pub name: String,
    pub capabilities: StoreCapabilities,
}

impl ErasureCoordinator {
    pub fn new(adapters: Vec<Box<dyn ErasureAdapter>>) -> Result<Self, ContractError> {
        if adapters.is_empty() {
            return Err(ContractError::NoAdapters);
        }
        let mut names = Vec::with_capacity(adapters.len());
        for adapter in &adapters {
            let name = adapter.name();
            if name.trim().is_empty() {
                return Err(ContractError::EmptyAdapterName);
            }
            if names.iter().any(|seen| seen == name) {
                return Err(ContractError::DuplicateAdapter(name.to_string()));
            }
            names.push(name.to_string());
        }
        Ok(Self { adapters })
    }

    pub fn erase_subject(&mut self, request: &ErasureRequest) -> StoreErasureReport {
        let expected_subject = request.subject_digest();
        let mut results = Vec::with_capacity(self.adapters.len());
        for adapter in &mut self.adapters {
            let store = adapter.name().to_string();
            let result = match adapter.erase_subject(request) {
                Ok(mut result) => {
                    if let Some(kind) = validate_result(&store, request, &expected_subject, &result)
                    {
                        failed_result(&store, request, &expected_subject, &kind)
                    } else {
                        match adapter.verify_subject_absent(&request.subject) {
                            Ok(true) => {
                                result.verified_absent = true;
                                result
                            }
                            Ok(false) => {
                                result.status = StoreErasureStatus::VerificationFailed;
                                result.verified_absent = false;
                                result.error = Some("verification_failed".to_string());
                                result
                            }
                            Err(error) => {
                                result.status = StoreErasureStatus::VerificationFailed;
                                result.verified_absent = false;
                                result.error = Some(error.kind);
                                result
                            }
                        }
                    }
                }
                Err(error) => failed_result(&store, request, &expected_subject, &error.kind),
            };
            results.push(result);
        }
        build_report(request, expected_subject, results)
    }

    /// Snapshot the configured store names and lifecycle capabilities for
    /// policy checks, diagnostics, and control-plane presentation.
    pub fn stores(&self) -> Vec<StoreDescriptor> {
        self.adapters
            .iter()
            .map(|adapter| StoreDescriptor {
                name: adapter.name().to_string(),
                capabilities: adapter.capabilities(),
            })
            .collect()
    }
}

fn validate_result(
    store: &str,
    request: &ErasureRequest,
    expected_subject: &str,
    result: &StoreErasureResult,
) -> Option<String> {
    if result.store != store {
        return Some("store_mismatch".to_string());
    }
    if result.request_id != request.request_id {
        return Some("request_id_mismatch".to_string());
    }
    if result.subject_digest != expected_subject {
        return Some("subject_mismatch".to_string());
    }
    if result.receipt.trim().is_empty() {
        return Some("missing_receipt".to_string());
    }
    None
}

fn failed_result(
    store: &str,
    request: &ErasureRequest,
    subject_digest: &str,
    kind: &str,
) -> StoreErasureResult {
    let mut hash = FNV_OFFSET;
    for value in [store, &request.request_id, subject_digest, kind] {
        hash = hash_field(hash, value);
    }
    StoreErasureResult {
        store: store.to_string(),
        request_id: request.request_id.clone(),
        subject_digest: subject_digest.to_string(),
        status: StoreErasureStatus::Failed,
        erased: 0,
        verified_absent: false,
        receipt: format!("lethe://store/{store}/failed/{hash:016x}"),
        error: Some(kind.to_string()),
    }
}

fn build_report(
    request: &ErasureRequest,
    subject_digest: String,
    results: Vec<StoreErasureResult>,
) -> StoreErasureReport {
    let completed = results.iter().filter(|result| result.complete()).count();
    let status = if completed == results.len() {
        ErasureReportStatus::Complete
    } else if completed == 0 {
        ErasureReportStatus::Failed
    } else {
        ErasureReportStatus::Partial
    };
    let mut canonical: Vec<&StoreErasureResult> = results.iter().collect();
    canonical.sort_by(|left, right| left.store.cmp(&right.store));
    let mut hash = FNV_OFFSET;
    hash = hash_field(hash, &request.request_id);
    hash = hash_field(hash, &subject_digest);
    hash = hash_field(hash, status.as_str());
    for result in canonical {
        hash = hash_field(hash, &result.store);
        hash = hash_field(hash, result.status.as_str());
        hash = fnv1a_extend(hash, &(result.erased as u64).to_le_bytes());
        hash = fnv1a_extend(hash, &[u8::from(result.verified_absent)]);
        hash = hash_field(hash, &result.receipt);
        hash = hash_field(hash, result.error.as_deref().unwrap_or(""));
    }
    StoreErasureReport {
        request_id: request.request_id.clone(),
        subject_digest,
        status,
        stores: results,
        receipt: format!("lethe://request/{hash:016x}"),
    }
}

fn subject_commitment(subject: &str) -> String {
    format!("fnv1a64:{:016x}", hash_field(FNV_OFFSET, subject))
}

fn hash_field(mut hash: u64, value: &str) -> u64 {
    hash = fnv1a_extend(hash, &(value.len() as u64).to_le_bytes());
    fnv1a_extend(hash, value.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct StubAdapter {
        name: String,
        fail: bool,
        verified: bool,
    }

    impl StubAdapter {
        fn complete(name: &str) -> Self {
            Self {
                name: name.to_string(),
                fail: false,
                verified: true,
            }
        }

        fn failing(name: &str) -> Self {
            Self {
                name: name.to_string(),
                fail: true,
                verified: false,
            }
        }
    }

    impl ErasureAdapter for StubAdapter {
        fn name(&self) -> &str {
            &self.name
        }

        fn capabilities(&self) -> StoreCapabilities {
            StoreCapabilities::new(true, false, true)
        }

        fn health(&mut self) -> Result<(), AdapterError> {
            Ok(())
        }

        fn erase_subject(
            &mut self,
            request: &ErasureRequest,
        ) -> Result<StoreErasureResult, AdapterError> {
            if self.fail {
                Err(AdapterError::new("offline"))
            } else {
                Ok(StoreErasureResult::successful(
                    &self.name,
                    request,
                    2,
                    &format!("lethe://store/{}/proof", self.name),
                ))
            }
        }

        fn verify_subject_absent(&mut self, _subject: &str) -> Result<bool, AdapterError> {
            Ok(self.verified)
        }
    }

    fn request() -> ErasureRequest {
        ErasureRequest::new("request-1", "user:1").unwrap()
    }

    #[test]
    fn every_verified_store_completes_the_request() {
        let mut coordinator = ErasureCoordinator::new(vec![
            Box::new(StubAdapter::complete("postgres")),
            Box::new(StubAdapter::complete("redis")),
        ])
        .unwrap();

        let report = coordinator.erase_subject(&request());

        assert_eq!(report.status, ErasureReportStatus::Complete);
        assert!(report.complete());
        assert!(report.stores.iter().all(StoreErasureResult::complete));
    }

    #[test]
    fn one_failed_store_is_a_partial_request() {
        let mut coordinator = ErasureCoordinator::new(vec![
            Box::new(StubAdapter::complete("postgres")),
            Box::new(StubAdapter::failing("redis")),
        ])
        .unwrap();

        let report = coordinator.erase_subject(&request());

        assert_eq!(report.status, ErasureReportStatus::Partial);
        assert_eq!(report.stores[1].status, StoreErasureStatus::Failed);
        assert_eq!(report.stores[1].error.as_deref(), Some("offline"));
    }

    #[test]
    fn aggregate_receipt_is_independent_of_adapter_order() {
        let mut left = ErasureCoordinator::new(vec![
            Box::new(StubAdapter::complete("postgres")),
            Box::new(StubAdapter::complete("redis")),
        ])
        .unwrap();
        let mut right = ErasureCoordinator::new(vec![
            Box::new(StubAdapter::complete("redis")),
            Box::new(StubAdapter::complete("postgres")),
        ])
        .unwrap();

        assert_eq!(
            left.erase_subject(&request()).receipt,
            right.erase_subject(&request()).receipt
        );
    }

    #[test]
    fn duplicate_adapter_names_are_rejected() {
        let result = ErasureCoordinator::new(vec![
            Box::new(StubAdapter::complete("same")),
            Box::new(StubAdapter::complete("same")),
        ]);

        assert!(matches!(result, Err(ContractError::DuplicateAdapter(_))));
    }

    #[test]
    fn descriptors_expose_capabilities_without_store_access() {
        let coordinator =
            ErasureCoordinator::new(vec![Box::new(StubAdapter::complete("redis"))]).unwrap();

        assert_eq!(
            coordinator.stores(),
            vec![StoreDescriptor {
                name: "redis".to_string(),
                capabilities: StoreCapabilities::new(true, false, true),
            }]
        );
    }

    #[test]
    fn empty_requests_and_coordinators_are_rejected() {
        assert_eq!(
            ErasureRequest::new("", "user:1"),
            Err(ContractError::EmptyRequestId)
        );
        assert_eq!(
            ErasureRequest::new("request-1", ""),
            Err(ContractError::EmptySubject)
        );
        assert!(matches!(
            ErasureCoordinator::new(Vec::new()),
            Err(ContractError::NoAdapters)
        ));
    }

    #[test]
    fn invalid_adapter_result_becomes_a_named_failure() {
        struct WrongStore;
        impl ErasureAdapter for WrongStore {
            fn name(&self) -> &str {
                "postgres"
            }
            fn capabilities(&self) -> StoreCapabilities {
                StoreCapabilities::new(false, false, false)
            }
            fn health(&mut self) -> Result<(), AdapterError> {
                Ok(())
            }
            fn erase_subject(
                &mut self,
                request: &ErasureRequest,
            ) -> Result<StoreErasureResult, AdapterError> {
                Ok(StoreErasureResult::successful("redis", request, 1, "proof"))
            }
            fn verify_subject_absent(&mut self, _subject: &str) -> Result<bool, AdapterError> {
                Ok(true)
            }
        }
        let mut coordinator = ErasureCoordinator::new(vec![Box::new(WrongStore)]).unwrap();

        let report = coordinator.erase_subject(&request());

        assert_eq!(report.status, ErasureReportStatus::Failed);
        assert_eq!(report.stores[0].error.as_deref(), Some("store_mismatch"));
    }
}
