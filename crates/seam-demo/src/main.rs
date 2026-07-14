//! One messy model output, three organs. Run with `cargo run -p seam-demo`.

use digest::{digest, digest_with_answers, Answer, Field, Outcome, Schema};
use lethe::{Lethe, RetentionPolicy};
use polysemic_core::Value;
use std::time::{Duration, Instant};
use strata::{dist, DriftWatch, Labor, Legislature, OntologySnapshot, ProvenanceLabel};

fn main() {
    println!("── the seam, before ─────────────────────────────────────");
    println!("   JSON.parse(model.output)  // ✕ SyntaxError, 3:12 AM, pager");
    println!();

    // What the model actually said. Fenced, single-quoted, Pythonic,
    // trailing comma, and a hedged quantity. A normal Tuesday.
    let raw = "Sure — here's the order you asked for:\n\
               ```json\n\
               {'item': 'espresso', 'qty': '2 or 3', 'gift_wrap': True,}\n\
               ```\n\
               Let me know if you need anything else!";

    let schema = Schema::obj([
        Field::req("item", Schema::Str),
        Field::req("qty", Schema::num_range(1.0, 99.0)),
        Field::opt("gift_wrap", Schema::Bool),
        Field::opt("size", Schema::choice(["small", "double", "triple"])),
    ]);

    println!("── digest: the metabolism ───────────────────────────────");
    let d = digest(raw, &schema).expect("unrecoverable text");
    for repair in &d.repairs {
        println!("   → repaired: {repair}");
    }
    match &d.outcome {
        Outcome::Resolved(v) => println!("   ✓ resolved: {v}"),
        Outcome::Clarify(questions) => {
            for q in questions {
                println!("   ? clarify:  {q}");
            }
            println!(
                "   ✓ 0 exceptions, {} clarifying question(s), no coin flips",
                questions.len()
            );
        }
    }
    println!();

    // The user answers "2". Digest applies only the path it asked about,
    // preserves the original output, and records the answer separately from
    // parser repairs.
    let d2 = digest_with_answers(raw, &schema, [Answer::new("$.qty", Value::Num(2.0))])
        .expect("valid clarification");
    if let Outcome::Resolved(v) = &d2.outcome {
        println!("   after clarification → {v}");
        println!(
            "   answer ledger       → {} = {}",
            d2.answers[0].path, d2.answers[0].value
        );
    }
    println!();

    println!("── strata: the sediment ─────────────────────────────────");
    let label = ProvenanceLabel {
        corpus: "prod-v4".into(),
        source_uri: "urn:polysemic:corpus:prod-v4".into(),
        corpus_sha256: "a".repeat(64),
        tokens: 2_400_000_000_000,
        vintage: "mostly post-1996".into(),
        languages: vec![("en".into(), 0.87), ("other".into(), 0.13)],
        annotator_labor: Labor::Credited,
        notes: vec!["categories settled during training; check before shipping".into()],
    };
    println!("{label}");
    println!();

    let watch = DriftWatch::new(0.2);
    let grown = dist([("nuclear", 0.8), ("extended", 0.2)]);
    let observed = dist([("nuclear", 0.4), ("extended", 0.3), ("chosen", 0.3)]);
    if let Some(alert) = watch.compare("family", &grown, &observed).unwrap() {
        println!("   {alert}");
    }

    let grown_ontology = OntologySnapshot::new(
        "family",
        "model-v1",
        [
            ("nuclear", "parents and dependent children"),
            ("extended", "relatives beyond the household"),
        ],
    );
    let observed_ontology = OntologySnapshot::new(
        "family",
        "users-2026-07",
        [
            ("nuclear", "a household's primary care network"),
            ("extended", "relatives beyond the household"),
            ("chosen", "people intentionally recognized as family"),
        ],
    );
    println!("   {}", grown_ontology.compare(&observed_ontology));

    let mut law = Legislature::new();
    law.enact("units", "metric", "product ships in the EU");
    println!(
        "   resolve(\"units\") → {}",
        law.resolve("units", "imperial")
    );
    println!("   resolve(\"size\")  → {}", law.resolve("size", "double"));
    println!();

    println!("── lethe: the excretion ─────────────────────────────────");
    let t0 = Instant::now();
    let mut memory = Lethe::new(Duration::from_secs(3600), 0.05);
    let preference_policy = RetentionPolicy::new(
        "customer-preference-v1",
        Duration::from_secs(90 * 24 * 3600),
    );
    memory.remember_with_policy("user:8842", "prefers oat milk", &preference_policy, t0);
    memory.remember_with_policy(
        "user:8842",
        "orders espresso, usually 2",
        &preference_policy,
        t0,
    );
    memory.remember(
        "scratch",
        "cart state: mid-checkout",
        Duration::from_secs(60),
        t0,
    );

    let recalled = memory.recall("espresso", t0 + Duration::from_secs(5));
    println!("   recalled {} memory(ies) about espresso", recalled.len());

    let sweep_receipt = memory.sweep_with_receipt(t0 + Duration::from_secs(120));
    println!("   scheduled sweep → {sweep_receipt}");

    let receipt = memory.forget(|m| m.subject == "user:8842");
    println!("   forget(user:8842) → {receipt}");
    println!("   remaining memories: {}", memory.len());
    println!();
    println!("── digest the contradiction · own the sediment · bless the delete key ──");
}
