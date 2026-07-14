//! One messy model output, three organs. Run with `cargo run -p seam-demo`.

use digest::{digest, Field, Outcome, Schema};
use lethe::Lethe;
use std::time::{Duration, Instant};
use strata::{dist, DriftWatch, Labor, Legislature, ProvenanceLabel};

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

    // The user answers "2". Second pass resolves.
    let answered = "{'item': 'espresso', 'qty': 2, 'gift_wrap': True}";
    let d2 = digest(answered, &schema).expect("unrecoverable text");
    if let Outcome::Resolved(v) = &d2.outcome {
        println!("   after clarification → {v}");
    }
    println!();

    println!("── strata: the sediment ─────────────────────────────────");
    let label = ProvenanceLabel {
        corpus: "prod-v4".into(),
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
    if let Some(alert) = watch.compare("family", &grown, &observed) {
        println!("   {alert}");
    }

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
    memory.remember(
        "user:8842",
        "prefers oat milk",
        Duration::from_secs(90 * 24 * 3600),
        t0,
    );
    memory.remember(
        "user:8842",
        "orders espresso, usually 2",
        Duration::from_secs(90 * 24 * 3600),
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

    let swept = memory.sweep(t0 + Duration::from_secs(120));
    println!(
        "   sweep → {} expired, {} faded (the garden, pruned)",
        swept.expired, swept.faded
    );

    let receipt = memory.forget(|m| m.subject == "user:8842");
    println!("   forget(user:8842) → {receipt}");
    println!("   remaining memories: {}", memory.len());
    println!();
    println!("── digest the contradiction · own the sediment · bless the delete key ──");
}
