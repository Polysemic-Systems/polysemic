//! strata — the sediment, made visible.
//!
//! A model's categories aren't designed; they're sedimented from a
//! particular corpus, written by particular people, at a particular time.
//! This crate makes the sediment legible and governable:
//!
//! - [`ProvenanceLabel`]: a shipping manifest for training data. Wrapped in a
//!   [`ProvenanceEnvelope`], an ontology snapshot and every comparison retain
//!   the histories that produced them.
//! - [`DriftWatch`]: total-variation distance between the distribution the
//!   model's ontology assumes and the distribution your users actually
//!   exhibit. When the weights stop matching the world, you hear about it.
//! - [`OntologySnapshot`]: versioned category definitions. Comparing two
//!   snapshots reveals added, removed, and redefined concepts even when no
//!   numeric distribution is available.
//! - [`Legislature`]: the explicit constraint layer over the grown one.
//!   Grown meaning answers by default; legislated rules overrule it where
//!   someone has taken responsibility for doing so — and every resolution
//!   says which layer answered.

use std::collections::BTreeMap;
use std::fmt;

// ---------------------------------------------------------------------------
// Provenance
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Labor {
    /// Annotator labor acknowledged and traceable.
    Credited,
    /// Present in the pipeline, absent from the record. The default of the
    /// industry; never the default here — you must say it out loud.
    Uncredited,
}

impl fmt::Display for Labor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Labor::Credited => write!(f, "present, credited"),
            Labor::Uncredited => write!(f, "present, uncredited"),
        }
    }
}

/// A nutrition label for a corpus. Ship it with every model artifact.
#[derive(Debug, Clone, PartialEq)]
pub struct ProvenanceLabel {
    pub corpus: String,
    /// Stable location or registry identifier for the corpus manifest.
    pub source_uri: String,
    /// SHA-256 commitment to that manifest or corpus export.
    pub corpus_sha256: String,
    pub tokens: u64,
    pub vintage: String,
    /// Language shares, e.g. `[("en", 0.87), ("other", 0.13)]`.
    pub languages: Vec<(String, f64)>,
    pub annotator_labor: Labor,
    pub notes: Vec<String>,
}

impl fmt::Display for ProvenanceLabel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "┌─ PROVENANCE FACTS ─────────────────────────")?;
        writeln!(f, "│ corpus          {}", self.corpus)?;
        writeln!(f, "│ source          {}", self.source_uri)?;
        writeln!(f, "│ corpus sha256   {}", self.corpus_sha256)?;
        writeln!(f, "│ serving size    {} tokens", humanize(self.tokens))?;
        writeln!(f, "│ vintage         {}", self.vintage)?;
        for (lang, share) in &self.languages {
            writeln!(f, "│ language        {lang}: {:.0}%", share * 100.0)?;
        }
        writeln!(f, "│ annotator labor {}", self.annotator_labor)?;
        for note in &self.notes {
            writeln!(f, "│ ⚠ {note}")?;
        }
        write!(f, "└────────────────────────────────────────────")
    }
}

fn humanize(n: u64) -> String {
    match n {
        n if n >= 1_000_000_000_000 => format!("{:.1}T", n as f64 / 1e12),
        n if n >= 1_000_000_000 => format!("{:.1}B", n as f64 / 1e9),
        n if n >= 1_000_000 => format!("{:.1}M", n as f64 / 1e6),
        n => n.to_string(),
    }
}

// ---------------------------------------------------------------------------
// Drift
// ---------------------------------------------------------------------------

/// A distribution over category values, e.g. what "family" looks like in
/// the corpus vs. among your users. Values need not be normalized; they
/// will be.
pub type Dist = BTreeMap<String, f64>;

pub fn dist<S: Into<String>>(pairs: impl IntoIterator<Item = (S, f64)>) -> Dist {
    pairs.into_iter().map(|(k, v)| (k.into(), v)).collect()
}

/// Total variation distance between two (auto-normalized) distributions:
/// half the L1 distance, in `[0, 1]`. Invalid weights are rejected rather than
/// turned into a plausible drift score.
pub fn total_variation(p: &Dist, q: &Dist) -> Result<f64, DistributionError> {
    validate_distribution("baseline", p)?;
    validate_distribution("observed", q)?;
    let sum_p: f64 = p.values().sum();
    let sum_q: f64 = q.values().sum();
    let np = |k: &str| p.get(k).copied().unwrap_or(0.0) / sum_p;
    let nq = |k: &str| q.get(k).copied().unwrap_or(0.0) / sum_q;

    let mut keys: Vec<&String> = p.keys().chain(q.keys()).collect();
    keys.sort();
    keys.dedup();

    Ok(0.5 * keys.iter().map(|k| (np(k) - nq(k)).abs()).sum::<f64>())
}

#[derive(Debug, Clone, PartialEq)]
pub enum DistributionError {
    InvalidThreshold(f64),
    EmptyMass {
        side: &'static str,
    },
    InvalidWeight {
        side: &'static str,
        category: String,
        weight: f64,
    },
}

impl fmt::Display for DistributionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DistributionError::InvalidThreshold(threshold) => {
                write!(
                    f,
                    "drift threshold must be finite and in [0, 1], got {threshold}"
                )
            }
            DistributionError::EmptyMass { side } => {
                write!(f, "{side} distribution must have positive mass")
            }
            DistributionError::InvalidWeight {
                side,
                category,
                weight,
            } => write!(
                f,
                "{side} distribution has invalid weight {weight} for {category:?}"
            ),
        }
    }
}

impl std::error::Error for DistributionError {}

fn validate_distribution(side: &'static str, distribution: &Dist) -> Result<(), DistributionError> {
    for (category, weight) in distribution {
        if !weight.is_finite() || *weight < 0.0 {
            return Err(DistributionError::InvalidWeight {
                side,
                category: category.clone(),
                weight: *weight,
            });
        }
    }
    if distribution.values().sum::<f64>() <= 0.0 {
        return Err(DistributionError::EmptyMass { side });
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq)]
pub struct DriftAlert {
    pub category: String,
    pub distance: f64,
    pub suggestion: String,
}

impl fmt::Display for DriftAlert {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "⚠ drift on {:?}: TV distance {:.2} — {}",
            self.category, self.distance, self.suggestion
        )
    }
}

pub struct DriftWatch {
    pub threshold: f64,
}

impl DriftWatch {
    pub fn new(threshold: f64) -> Self {
        DriftWatch { threshold }
    }

    /// Compare the distribution the model's ontology assumes (`grown`)
    /// against what your users actually exhibit (`observed`).
    pub fn compare(
        &self,
        category: &str,
        grown: &Dist,
        observed: &Dist,
    ) -> Result<Option<DriftAlert>, DistributionError> {
        if !self.threshold.is_finite() || !(0.0..=1.0).contains(&self.threshold) {
            return Err(DistributionError::InvalidThreshold(self.threshold));
        }
        let d = total_variation(grown, observed)?;
        if d > self.threshold {
            Ok(Some(DriftAlert {
                category: category.to_string(),
                distance: d,
                suggestion: "activate constraint layer; schedule sediment refresh".into(),
            }))
        } else {
            Ok(None)
        }
    }
}

// ---------------------------------------------------------------------------
// Ontology versions
// ---------------------------------------------------------------------------

/// A versioned, inspectable vocabulary. Definitions are intentionally plain
/// text in this POC: a production Strata can replace comparison with semantic
/// embeddings while keeping the version and diff contract.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OntologySnapshot {
    pub name: String,
    pub version: String,
    pub concepts: BTreeMap<String, String>,
}

impl OntologySnapshot {
    pub fn new<K: Into<String>, V: Into<String>>(
        name: &str,
        version: &str,
        concepts: impl IntoIterator<Item = (K, V)>,
    ) -> Self {
        Self {
            name: name.to_string(),
            version: version.to_string(),
            concepts: concepts
                .into_iter()
                .map(|(concept, definition)| (concept.into(), definition.into()))
                .collect(),
        }
    }

    /// Compare this sedimented ontology with a newer observed vocabulary.
    pub fn compare(&self, observed: &OntologySnapshot) -> OntologyDrift {
        let added = observed
            .concepts
            .keys()
            .filter(|concept| !self.concepts.contains_key(*concept))
            .cloned()
            .collect();
        let removed = self
            .concepts
            .keys()
            .filter(|concept| !observed.concepts.contains_key(*concept))
            .cloned()
            .collect();
        let redefined = self
            .concepts
            .iter()
            .filter_map(|(concept, grown_definition)| {
                let observed_definition = observed.concepts.get(concept)?;
                (canonical_definition(grown_definition)
                    != canonical_definition(observed_definition))
                .then(|| concept.clone())
            })
            .collect();

        OntologyDrift {
            ontology: self.name.clone(),
            from_version: self.version.clone(),
            to_version: observed.version.clone(),
            added,
            removed,
            redefined,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OntologyDrift {
    pub ontology: String,
    pub from_version: String,
    pub to_version: String,
    pub added: Vec<String>,
    pub removed: Vec<String>,
    pub redefined: Vec<String>,
}

impl OntologyDrift {
    pub fn is_empty(&self) -> bool {
        self.added.is_empty() && self.removed.is_empty() && self.redefined.is_empty()
    }
}

impl fmt::Display for OntologyDrift {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "ontology {:?} {} → {}: +{} -{} ~{}",
            self.ontology,
            self.from_version,
            self.to_version,
            self.added.len(),
            self.removed.len(),
            self.redefined.len()
        )?;
        if !self.added.is_empty() {
            write!(f, " · added [{}]", self.added.join(", "))?;
        }
        if !self.removed.is_empty() {
            write!(f, " · removed [{}]", self.removed.join(", "))?;
        }
        if !self.redefined.is_empty() {
            write!(f, " · redefined [{}]", self.redefined.join(", "))?;
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Provenance-carrying ontology artifacts
// ---------------------------------------------------------------------------

/// An ontology snapshot that cannot travel without its corpus provenance.
///
/// The artifact and ontology versions are intentionally separate: a model
/// release can reuse an ontology snapshot, and an ontology can evolve between
/// model releases. Keeping both makes that relationship inspectable.
#[derive(Debug, Clone, PartialEq)]
pub struct ProvenanceEnvelope {
    pub artifact_id: String,
    pub artifact_version: String,
    /// Stable location or registry identifier for the artifact bytes.
    pub artifact_uri: String,
    /// SHA-256 commitment to the exact artifact revision under review.
    pub artifact_sha256: String,
    pub provenance: ProvenanceLabel,
    pub ontology: OntologySnapshot,
    /// Stable location of the behavior-case corpus replayed by a runner.
    pub behavior_uri: String,
    /// SHA-256 commitment to the exact behavior-case corpus.
    pub behavior_sha256: String,
    /// Runner observations materialized from the committed artifact and cases.
    pub behavior_cases: BTreeMap<String, BehaviorReading>,
}

impl ProvenanceEnvelope {
    pub fn new(
        artifact_id: &str,
        artifact_version: &str,
        artifact_uri: &str,
        artifact_sha256: &str,
        provenance: ProvenanceLabel,
        ontology: OntologySnapshot,
    ) -> Self {
        Self {
            artifact_id: artifact_id.to_string(),
            artifact_version: artifact_version.to_string(),
            artifact_uri: artifact_uri.to_string(),
            artifact_sha256: artifact_sha256.to_string(),
            provenance,
            ontology,
            behavior_uri: String::new(),
            behavior_sha256: String::new(),
            behavior_cases: BTreeMap::new(),
        }
    }

    pub fn with_behavior_source(mut self, source_uri: &str, sha256: &str) -> Self {
        self.behavior_uri = source_uri.to_string();
        self.behavior_sha256 = sha256.to_string();
        self
    }

    pub fn with_behavior_cases(
        mut self,
        cases: impl IntoIterator<Item = (String, BehaviorReading)>,
    ) -> Self {
        self.behavior_cases = cases.into_iter().collect();
        self
    }

    /// Compare two revisions of the same artifact and probe one concept's
    /// meaning on both sides of the version boundary.
    pub fn compare(
        &self,
        observed: &ProvenanceEnvelope,
        concept: &str,
    ) -> Result<EnvelopeComparison, ComparisonError> {
        if self.artifact_id != observed.artifact_id {
            return Err(ComparisonError::ArtifactMismatch {
                baseline: self.artifact_id.clone(),
                observed: observed.artifact_id.clone(),
            });
        }
        if self.ontology.name != observed.ontology.name {
            return Err(ComparisonError::OntologyMismatch {
                baseline: self.ontology.name.clone(),
                observed: observed.ontology.name.clone(),
            });
        }

        let baseline_definition = self.ontology.concepts.get(concept).cloned();
        let observed_definition = observed.ontology.concepts.get(concept).cloned();
        if baseline_definition.is_none() && observed_definition.is_none() {
            return Err(ComparisonError::UnknownProbe {
                concept: concept.to_string(),
            });
        }
        Ok(EnvelopeComparison {
            baseline: self.clone(),
            observed: observed.clone(),
            ontology_drift: self.ontology.compare(&observed.ontology),
            probe: ConceptProbe {
                concept: concept.to_string(),
                baseline_definition,
                observed_definition,
            },
        })
    }

    /// Compare runner observations for one committed case across two artifact
    /// snapshots. The case input must be identical, and each classification
    /// must exist in its ontology.
    pub fn probe_behavior(
        &self,
        observed: &ProvenanceEnvelope,
        case_id: &str,
    ) -> Result<BehaviorProbe, ComparisonError> {
        if self.behavior_sha256 != observed.behavior_sha256 {
            return Err(ComparisonError::BehaviorCorpusMismatch {
                baseline: self.behavior_sha256.clone(),
                observed: observed.behavior_sha256.clone(),
            });
        }
        let baseline = self.behavior_cases.get(case_id).cloned();
        let observed_reading = observed.behavior_cases.get(case_id).cloned();
        let (Some(baseline), Some(observed_reading)) = (baseline, observed_reading) else {
            return Err(ComparisonError::UnknownBehaviorCase {
                case_id: case_id.to_string(),
            });
        };
        if baseline.input != observed_reading.input {
            return Err(ComparisonError::BehaviorInputMismatch {
                case_id: case_id.to_string(),
            });
        }
        for (side, reading, ontology) in [
            ("baseline", &baseline, &self.ontology),
            ("observed", &observed_reading, &observed.ontology),
        ] {
            if !ontology.concepts.contains_key(&reading.classification) {
                return Err(ComparisonError::InvalidBehaviorClassification {
                    side,
                    case_id: case_id.to_string(),
                    classification: reading.classification.clone(),
                });
            }
        }
        Ok(BehaviorProbe {
            case_id: case_id.to_string(),
            input: baseline.input.clone(),
            baseline,
            observed: observed_reading,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BehaviorReading {
    pub input: String,
    pub classification: String,
    pub route: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BehaviorProbe {
    pub case_id: String,
    pub input: String,
    pub baseline: BehaviorReading,
    pub observed: BehaviorReading,
}

impl BehaviorProbe {
    pub fn changed(&self) -> bool {
        self.baseline.classification != self.observed.classification
            || self.baseline.route != self.observed.route
    }
}

/// A comparison was requested between envelopes that do not describe the
/// same artifact or ontology. Refusing the comparison avoids a plausible but
/// meaningless drift score.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ComparisonError {
    ArtifactMismatch {
        baseline: String,
        observed: String,
    },
    OntologyMismatch {
        baseline: String,
        observed: String,
    },
    UnknownProbe {
        concept: String,
    },
    UnknownBehaviorCase {
        case_id: String,
    },
    BehaviorCorpusMismatch {
        baseline: String,
        observed: String,
    },
    BehaviorInputMismatch {
        case_id: String,
    },
    InvalidBehaviorClassification {
        side: &'static str,
        case_id: String,
        classification: String,
    },
}

impl fmt::Display for ComparisonError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ComparisonError::ArtifactMismatch { baseline, observed } => write!(
                f,
                "artifact mismatch: baseline {baseline:?}, observed {observed:?}"
            ),
            ComparisonError::OntologyMismatch { baseline, observed } => write!(
                f,
                "ontology mismatch: baseline {baseline:?}, observed {observed:?}"
            ),
            ComparisonError::UnknownProbe { concept } => {
                write!(f, "probe concept {concept:?} is absent from both snapshots")
            }
            ComparisonError::UnknownBehaviorCase { case_id } => {
                write!(
                    f,
                    "behavior case {case_id:?} is not recorded in both snapshots"
                )
            }
            ComparisonError::BehaviorCorpusMismatch { baseline, observed } => write!(
                f,
                "behavior corpus mismatch: baseline {baseline:?}, observed {observed:?}"
            ),
            ComparisonError::BehaviorInputMismatch { case_id } => write!(
                f,
                "behavior case {case_id:?} does not contain the same input on both sides"
            ),
            ComparisonError::InvalidBehaviorClassification {
                side,
                case_id,
                classification,
            } => write!(
                f,
                "{side} behavior case {case_id:?} references unknown concept {classification:?}"
            ),
        }
    }
}

impl std::error::Error for ComparisonError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConceptProbe {
    pub concept: String,
    pub baseline_definition: Option<String>,
    pub observed_definition: Option<String>,
}

impl ConceptProbe {
    /// Whether the probed concept was added, removed, or materially
    /// redefined. Case and whitespace-only edits are ignored just as they are
    /// in [`OntologySnapshot::compare`].
    pub fn changed(&self) -> bool {
        match (&self.baseline_definition, &self.observed_definition) {
            (Some(baseline), Some(observed)) => {
                canonical_definition(baseline) != canonical_definition(observed)
            }
            (None, None) => false,
            _ => true,
        }
    }
}

/// A drift result with both source envelopes attached. The definitions in the
/// diff and probe therefore cannot be transported without the corpus and
/// labor histories that produced them.
#[derive(Debug, Clone, PartialEq)]
pub struct EnvelopeComparison {
    pub baseline: ProvenanceEnvelope,
    pub observed: ProvenanceEnvelope,
    pub ontology_drift: OntologyDrift,
    pub probe: ConceptProbe,
}

impl EnvelopeComparison {
    pub fn drift_detected(&self) -> bool {
        !self.ontology_drift.is_empty()
    }
}

fn canonical_definition(definition: &str) -> String {
    definition
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

// ---------------------------------------------------------------------------
// The legislated layer
// ---------------------------------------------------------------------------

/// Where a resolution came from. Every answer names its layer: no more
/// pretending the grown ontology's output is neutral fact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    /// The sedimented default — what the model grew.
    Grown,
    /// An explicit human rule — what someone legislated, with a stated
    /// reason attached.
    Legislated { rule: String, reason: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolution {
    pub value: String,
    pub source: Source,
}

impl fmt::Display for Resolution {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.source {
            Source::Grown => write!(f, "{} (grown)", self.value),
            Source::Legislated { reason, .. } => {
                write!(f, "{} (legislated: {})", self.value, reason)
            }
        }
    }
}

/// The contestable constraint layer. Rules override the grown default per
/// category — and every rule carries a reason, because an override without
/// a reason is just a different unaccountable ontology.
///
/// Honest scope: this is a per-category override map with mandatory
/// reasons, not a governance registry. It keeps no history, no owners,
/// no timestamps, and no audit log; enacting a category replaces the
/// previous rule. Those are the seams a real registry would fill.
#[derive(Default)]
pub struct Legislature {
    rules: BTreeMap<String, (String, String)>, // category → (value, reason)
}

/// Refusal to enact an unaccountable rule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EnactError {
    /// The rule gave no reason. "Every rule carries a stated reason" is a
    /// contract, not a suggestion — an empty reason is rejected, not stored.
    EmptyReason,
    /// The category is empty; a rule must govern something nameable.
    EmptyCategory,
}

impl fmt::Display for EnactError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EnactError::EmptyReason => write!(f, "a rule without a reason cannot be enacted"),
            EnactError::EmptyCategory => write!(f, "a rule must name a non-empty category"),
        }
    }
}

impl std::error::Error for EnactError {}

impl Legislature {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn enact(&mut self, category: &str, value: &str, reason: &str) -> Result<(), EnactError> {
        if category.trim().is_empty() {
            return Err(EnactError::EmptyCategory);
        }
        if reason.trim().is_empty() {
            return Err(EnactError::EmptyReason);
        }
        self.rules.insert(
            category.to_string(),
            (value.to_string(), reason.to_string()),
        );
        Ok(())
    }

    /// Repealing a rule is as first-class as enacting one. (The forgetting
    /// crate would be disappointed otherwise.)
    pub fn repeal(&mut self, category: &str) -> bool {
        self.rules.remove(category).is_some()
    }

    /// Resolve a category: legislated rule if one exists, otherwise the
    /// grown default. Either way, the source is named.
    pub fn resolve(&self, category: &str, grown_default: &str) -> Resolution {
        match self.rules.get(category) {
            Some((value, reason)) => Resolution {
                value: value.clone(),
                source: Source::Legislated {
                    rule: category.to_string(),
                    reason: reason.clone(),
                },
            },
            None => Resolution {
                value: grown_default.to_string(),
                source: Source::Grown,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tvd_is_zero_for_identical_and_one_for_disjoint() {
        let p = dist([("a", 0.5), ("b", 0.5)]);
        assert!(total_variation(&p, &p).unwrap() < 1e-12);

        let q = dist([("c", 1.0)]);
        assert!((total_variation(&p, &q).unwrap() - 1.0).abs() < 1e-12);
    }

    #[test]
    fn drift_watch_alerts_over_threshold_only() {
        let watch = DriftWatch::new(0.2);
        let grown = dist([("nuclear", 0.8), ("extended", 0.2)]);
        let same = dist([("nuclear", 0.78), ("extended", 0.22)]);
        let moved = dist([("nuclear", 0.4), ("extended", 0.3), ("chosen", 0.3)]);

        assert!(watch.compare("family", &grown, &same).unwrap().is_none());
        let alert = watch.compare("family", &grown, &moved).unwrap().unwrap();
        assert!(alert.distance > 0.2);
    }

    #[test]
    fn drift_watch_rejects_invalid_distributions() {
        let watch = DriftWatch::new(0.2);
        let invalid = dist([("family", -1.0), ("other", 2.0)]);
        let observed = dist([("family", 1.0)]);

        assert!(matches!(
            watch.compare("family", &invalid, &observed),
            Err(DistributionError::InvalidWeight { .. })
        ));
        assert!(total_variation(&Dist::new(), &observed).is_err());
    }

    #[test]
    fn ontology_diff_names_added_removed_and_redefined_concepts() {
        let grown = OntologySnapshot::new(
            "family",
            "model-v1",
            [
                ("nuclear", "parents and dependent children"),
                ("extended", "relatives beyond the household"),
                ("legacy", "a retired historical bucket"),
            ],
        );
        let observed = OntologySnapshot::new(
            "family",
            "users-2026-07",
            [
                ("nuclear", "a household's primary care network"),
                ("extended", "relatives beyond the household"),
                ("chosen", "people intentionally recognized as family"),
            ],
        );

        let drift = grown.compare(&observed);
        assert_eq!(drift.added, vec!["chosen"]);
        assert_eq!(drift.removed, vec!["legacy"]);
        assert_eq!(drift.redefined, vec!["nuclear"]);
        assert!(!drift.is_empty());
        assert!(drift.to_string().contains("added [chosen]"));
    }

    #[test]
    fn ontology_diff_ignores_case_and_whitespace_only_changes() {
        let grown = OntologySnapshot::new("units", "v1", [("metric", "SI  units")]);
        let observed = OntologySnapshot::new("units", "v2", [("metric", "si units")]);

        assert!(grown.compare(&observed).is_empty());
    }

    #[test]
    fn provenance_envelopes_compare_only_like_artifacts() {
        let baseline = envelope(
            "family-router",
            "model-v1",
            "family",
            "parents and children",
        );
        let observed = envelope(
            "family-router",
            "candidate-v2",
            "family",
            "a household's primary care network",
        );

        let comparison = baseline.compare(&observed, "nuclear").unwrap();
        assert!(comparison.drift_detected());
        assert!(comparison.probe.changed());
        assert_eq!(comparison.baseline.artifact_id, "family-router");
        assert_eq!(comparison.baseline.provenance.corpus, "support-archive");
        assert_eq!(comparison.observed.provenance.corpus, "support-archive");

        let unrelated = envelope("risk-router", "v2", "family", "parents and children");
        assert!(matches!(
            baseline.compare(&unrelated, "nuclear"),
            Err(ComparisonError::ArtifactMismatch { .. })
        ));
    }

    #[test]
    fn concept_probe_exposes_added_meaning() {
        let baseline = ProvenanceEnvelope::new(
            "family-router",
            "model-v1",
            "urn:test:artifact:model-v1",
            &"1".repeat(64),
            label("archive-2018"),
            OntologySnapshot::new("family", "v1", [("nuclear", "parents and children")]),
        );
        let observed = ProvenanceEnvelope::new(
            "family-router",
            "candidate-v2",
            "urn:test:artifact:candidate-v2",
            &"2".repeat(64),
            label("support-2026"),
            OntologySnapshot::new(
                "family",
                "v2",
                [
                    ("nuclear", "parents and children"),
                    ("chosen", "people intentionally recognized as family"),
                ],
            ),
        );

        let probe = baseline.compare(&observed, "chosen").unwrap().probe;
        assert!(probe.baseline_definition.is_none());
        assert!(probe.observed_definition.is_some());
        assert!(probe.changed());
    }

    #[test]
    fn unknown_concept_cannot_masquerade_as_a_stable_probe() {
        let baseline = envelope(
            "family-router",
            "model-v1",
            "family",
            "parents and children",
        );
        let observed = envelope(
            "family-router",
            "candidate-v2",
            "family",
            "a household's primary care network",
        );

        assert_eq!(
            baseline.compare(&observed, "nucelar"),
            Err(ComparisonError::UnknownProbe {
                concept: "nucelar".into(),
            })
        );
    }

    #[test]
    fn behavior_probe_compares_the_same_case_and_exposes_a_route_change() {
        let baseline = envelope(
            "family-router",
            "model-v1",
            "family",
            "parents and children",
        )
        .with_behavior_cases([(
            "chosen-caregiver".into(),
            BehaviorReading {
                input: "Alex names a chosen-family caregiver".into(),
                classification: "nuclear".into(),
                route: "manual-review".into(),
            },
        )])
        .with_behavior_source("urn:test:cases", &"3".repeat(64));
        let observed = envelope(
            "family-router",
            "candidate-v2",
            "family",
            "a household's primary care network",
        )
        .with_behavior_cases([(
            "chosen-caregiver".into(),
            BehaviorReading {
                input: "Alex names a chosen-family caregiver".into(),
                classification: "nuclear".into(),
                route: "family-support".into(),
            },
        )])
        .with_behavior_source("urn:test:cases", &"3".repeat(64));

        let probe = baseline
            .probe_behavior(&observed, "chosen-caregiver")
            .unwrap();
        assert!(probe.changed());
        assert_eq!(probe.baseline.route, "manual-review");
        assert_eq!(probe.observed.route, "family-support");
    }

    #[test]
    fn legislature_overrules_grown_and_names_its_source() {
        let mut law = Legislature::new();
        let before = law.resolve("units", "imperial");
        assert_eq!(before.source, Source::Grown);

        law.enact("units", "metric", "product ships in the EU")
            .expect("a reasoned rule enacts");
        let after = law.resolve("units", "imperial");
        assert_eq!(after.value, "metric");
        assert!(matches!(after.source, Source::Legislated { .. }));

        // An override without a reason is refused, not stored.
        assert_eq!(
            law.enact("units", "imperial", "  "),
            Err(EnactError::EmptyReason)
        );
        assert_eq!(
            law.enact("", "metric", "why"),
            Err(EnactError::EmptyCategory)
        );
        assert_eq!(law.resolve("units", "imperial").value, "metric");

        assert!(law.repeal("units"));
        assert_eq!(law.resolve("units", "imperial").value, "imperial");
    }

    fn label(corpus: &str) -> ProvenanceLabel {
        ProvenanceLabel {
            corpus: corpus.into(),
            source_uri: format!("urn:test:{corpus}"),
            corpus_sha256: "a".repeat(64),
            tokens: 1_000,
            vintage: "2026-07".into(),
            languages: vec![("en".into(), 1.0)],
            annotator_labor: Labor::Credited,
            notes: Vec::new(),
        }
    }

    fn envelope(
        artifact_id: &str,
        artifact_version: &str,
        ontology: &str,
        definition: &str,
    ) -> ProvenanceEnvelope {
        ProvenanceEnvelope::new(
            artifact_id,
            artifact_version,
            &format!("urn:test:artifact:{artifact_version}"),
            &"1".repeat(64),
            label("support-archive"),
            OntologySnapshot::new(ontology, artifact_version, [("nuclear", definition)]),
        )
    }
}
