//! strata — the sediment, made visible.
//!
//! A model's categories aren't designed; they're sedimented from a
//! particular corpus, written by particular people, at a particular time.
//! This crate makes the sediment legible and governable:
//!
//! - [`ProvenanceLabel`]: a shipping manifest for training data. The model
//!   does not simply "know" — it remembers what a particular crowd once
//!   wrote. The label says which crowd.
//! - [`DriftWatch`]: total-variation distance between the distribution the
//!   model's ontology assumes and the distribution your users actually
//!   exhibit. When the weights stop matching the world, you hear about it.
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
#[derive(Debug, Clone)]
pub struct ProvenanceLabel {
    pub corpus: String,
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
/// half the L1 distance, in `[0, 1]`. The honest scalar for "how far has
/// the sediment drifted from the world".
pub fn total_variation(p: &Dist, q: &Dist) -> f64 {
    let sum_p: f64 = p.values().sum();
    let sum_q: f64 = q.values().sum();
    let np = |k: &str| p.get(k).copied().unwrap_or(0.0) / sum_p.max(f64::EPSILON);
    let nq = |k: &str| q.get(k).copied().unwrap_or(0.0) / sum_q.max(f64::EPSILON);

    let mut keys: Vec<&String> = p.keys().chain(q.keys()).collect();
    keys.sort();
    keys.dedup();

    0.5 * keys.iter().map(|k| (np(k) - nq(k)).abs()).sum::<f64>()
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
    pub fn compare(&self, category: &str, grown: &Dist, observed: &Dist) -> Option<DriftAlert> {
        let d = total_variation(grown, observed);
        if d > self.threshold {
            Some(DriftAlert {
                category: category.to_string(),
                distance: d,
                suggestion: "activate constraint layer; schedule sediment refresh".into(),
            })
        } else {
            None
        }
    }
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
#[derive(Default)]
pub struct Legislature {
    rules: BTreeMap<String, (String, String)>, // category → (value, reason)
}

impl Legislature {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn enact(&mut self, category: &str, value: &str, reason: &str) {
        self.rules.insert(
            category.to_string(),
            (value.to_string(), reason.to_string()),
        );
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
        assert!(total_variation(&p, &p) < 1e-12);

        let q = dist([("c", 1.0)]);
        assert!((total_variation(&p, &q) - 1.0).abs() < 1e-12);
    }

    #[test]
    fn drift_watch_alerts_over_threshold_only() {
        let watch = DriftWatch::new(0.2);
        let grown = dist([("nuclear", 0.8), ("extended", 0.2)]);
        let same = dist([("nuclear", 0.78), ("extended", 0.22)]);
        let moved = dist([("nuclear", 0.4), ("extended", 0.3), ("chosen", 0.3)]);

        assert!(watch.compare("family", &grown, &same).is_none());
        let alert = watch.compare("family", &grown, &moved).unwrap();
        assert!(alert.distance > 0.2);
    }

    #[test]
    fn legislature_overrules_grown_and_names_its_source() {
        let mut law = Legislature::new();
        let before = law.resolve("units", "imperial");
        assert_eq!(before.source, Source::Grown);

        law.enact("units", "metric", "product ships in the EU");
        let after = law.resolve("units", "imperial");
        assert_eq!(after.value, "metric");
        assert!(matches!(after.source, Source::Legislated { .. }));

        assert!(law.repeal("units"));
        assert_eq!(law.resolve("units", "imperial").value, "imperial");
    }
}
