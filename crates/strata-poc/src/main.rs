//! A product-shaped Strata proof: two provenance-carrying ontology snapshots
//! in, and one machine-readable semantic-drift report out.

use polysemic_core::{parse, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fmt;
use std::fs;
use std::path::PathBuf;
use strata::{
    BehaviorProbe, BehaviorReading, ComparisonError, EnvelopeComparison, Labor, OntologySnapshot,
    ProvenanceEnvelope, ProvenanceLabel,
};

const DEMO_BASELINE: &str = include_str!("../../../examples/strata-poc/model-v1.envelope.json");
const DEMO_OBSERVED: &str = include_str!("../../../examples/strata-poc/candidate-v2.envelope.json");

const HELP: &str = r#"strata-poc — provenance travels with meaning

USAGE
  strata-poc demo
  strata-poc compare --baseline FILE --observed FILE --probe CONCEPT --case CASE

EXIT STATUS
  0  stable: no structural ontology drift detected
  2  drift_detected: route the comparison to an accountable owner
  1  rejected: an envelope or comparison contract is invalid
"#;

fn main() {
    std::process::exit(run(env::args().skip(1).collect()));
}

fn run(args: Vec<String>) -> i32 {
    let Some(command) = args.first().map(String::as_str) else {
        print!("{HELP}");
        return 1;
    };
    match command {
        "demo" if args.len() == 1 => run_demo(),
        "compare" => run_compare(&args[1..]),
        "help" | "--help" | "-h" if args.len() == 1 => {
            print!("{HELP}");
            0
        }
        _ => {
            println!(
                "{}",
                rejection(
                    "cli",
                    "invalid_command",
                    "unknown command or arguments",
                    None,
                )
            );
            1
        }
    }
}

#[derive(Default)]
struct Options {
    baseline: Option<PathBuf>,
    observed: Option<PathBuf>,
    probe: Option<String>,
    case_id: Option<String>,
}

fn run_compare(args: &[String]) -> i32 {
    let options = match parse_options(args) {
        Ok(options) => options,
        Err(message) => {
            println!("{}", rejection("cli", "invalid_arguments", &message, None));
            return 1;
        }
    };
    let Some(baseline_path) = options.baseline else {
        println!(
            "{}",
            rejection(
                "cli",
                "missing_baseline",
                "--baseline FILE is required",
                None,
            )
        );
        return 1;
    };
    let Some(observed_path) = options.observed else {
        println!(
            "{}",
            rejection(
                "cli",
                "missing_observed",
                "--observed FILE is required",
                None,
            )
        );
        return 1;
    };
    let Some(probe) = options.probe else {
        println!(
            "{}",
            rejection("cli", "missing_probe", "--probe CONCEPT is required", None,)
        );
        return 1;
    };
    let Some(case_id) = options.case_id else {
        println!(
            "{}",
            rejection("cli", "missing_case", "--case CASE is required", None,)
        );
        return 1;
    };

    let baseline_raw = match read_envelope(&baseline_path, "baseline") {
        Ok(raw) => raw,
        Err(envelope) => {
            println!("{envelope}");
            return 1;
        }
    };
    let observed_raw = match read_envelope(&observed_path, "observed") {
        Ok(raw) => raw,
        Err(envelope) => {
            println!("{envelope}");
            return 1;
        }
    };

    let (report, code) = analyze(&baseline_raw, &observed_raw, &probe, &case_id);
    println!("{report}");
    code
}

fn parse_options(args: &[String]) -> Result<Options, String> {
    let mut options = Options::default();
    let mut index = 0;
    while index < args.len() {
        let flag = args[index].as_str();
        let value = args
            .get(index + 1)
            .ok_or_else(|| format!("{flag} requires a value"))?;
        match flag {
            "--baseline" if options.baseline.is_none() => options.baseline = Some(value.into()),
            "--observed" if options.observed.is_none() => options.observed = Some(value.into()),
            "--probe" if options.probe.is_none() => options.probe = Some(value.clone()),
            "--case" if options.case_id.is_none() => options.case_id = Some(value.clone()),
            "--baseline" | "--observed" | "--probe" | "--case" => {
                return Err(format!("{flag} may be provided only once"));
            }
            _ => return Err(format!("unknown argument {flag:?}")),
        }
        index += 2;
    }
    Ok(options)
}

fn read_envelope(path: &PathBuf, stage: &str) -> Result<String, Value> {
    fs::read_to_string(path).map_err(|error| {
        rejection(
            stage,
            "read_failed",
            &format!("{}: {error}", path.display()),
            None,
        )
    })
}

fn analyze(baseline_raw: &str, observed_raw: &str, probe: &str, case_id: &str) -> (Value, i32) {
    let baseline = match parse_envelope(baseline_raw) {
        Ok(envelope) => envelope,
        Err(error) => {
            return (
                rejection(
                    "baseline",
                    "invalid_envelope",
                    &error.message,
                    Some(&error.path),
                ),
                1,
            );
        }
    };
    let observed = match parse_envelope(observed_raw) {
        Ok(envelope) => envelope,
        Err(error) => {
            return (
                rejection(
                    "observed",
                    "invalid_envelope",
                    &error.message,
                    Some(&error.path),
                ),
                1,
            );
        }
    };
    if probe.trim().is_empty() {
        return (
            rejection(
                "comparison",
                "invalid_probe",
                "probe concept must not be empty",
                Some("$.probe"),
            ),
            1,
        );
    }
    if case_id.trim().is_empty() {
        return (
            rejection(
                "comparison",
                "invalid_case",
                "behavior case must not be empty",
                Some("$.case"),
            ),
            1,
        );
    }

    match baseline.compare(&observed, probe) {
        Ok(comparison) => {
            let behavior = match baseline.probe_behavior(&observed, case_id) {
                Ok(behavior) => behavior,
                Err(error) => {
                    return (
                        rejection(
                            "comparison",
                            "invalid_behavior_probe",
                            &error.to_string(),
                            Some("$.case"),
                        ),
                        1,
                    );
                }
            };
            let drift_detected = comparison.drift_detected() || behavior.changed();
            let code = if drift_detected { 2 } else { 0 };
            (report_value(&comparison, &behavior, drift_detected), code)
        }
        Err(ComparisonError::UnknownProbe { concept }) => (
            rejection(
                "comparison",
                "unknown_probe",
                &format!("probe concept {concept:?} is absent from both snapshots"),
                Some("$.probe"),
            ),
            1,
        ),
        Err(error) => (
            rejection(
                "comparison",
                "incomparable_envelopes",
                &error.to_string(),
                None,
            ),
            1,
        ),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct InputError {
    path: String,
    message: String,
}

impl InputError {
    fn new(path: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            message: message.into(),
        }
    }
}

impl fmt::Display for InputError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.path, self.message)
    }
}

fn parse_envelope(raw: &str) -> Result<ProvenanceEnvelope, InputError> {
    let value = parse(raw).map_err(|error| {
        InputError::new(
            format!("byte {}", error.at),
            format!("invalid JSON: {}", error.msg),
        )
    })?;
    let root = expect_object(&value, "$")?;
    reject_unknown_fields(
        root,
        &["artifact", "provenance", "ontology", "behavior"],
        "$",
    )?;

    let artifact_value = required(root, "artifact", "$")?;
    let artifact = expect_object(artifact_value, "$.artifact")?;
    reject_unknown_fields(
        artifact,
        &["id", "version", "source_uri", "sha256"],
        "$.artifact",
    )?;
    let artifact_id = required_string(artifact, "id", "$.artifact")?;
    let artifact_version = required_string(artifact, "version", "$.artifact")?;
    let artifact_uri = required_string(artifact, "source_uri", "$.artifact")?;
    let artifact_sha256 = required_sha256(artifact, "sha256", "$.artifact")?;

    let provenance_value = required(root, "provenance", "$")?;
    let provenance = parse_provenance(provenance_value)?;

    let ontology_value = required(root, "ontology", "$")?;
    let ontology = parse_ontology(ontology_value)?;

    let behavior_value = required(root, "behavior", "$")?;
    let behavior_cases = parse_behavior(behavior_value)?;

    Ok(ProvenanceEnvelope::new(
        &artifact_id,
        &artifact_version,
        &artifact_uri,
        &artifact_sha256,
        provenance,
        ontology,
    )
    .with_behavior_cases(behavior_cases))
}

fn parse_provenance(value: &Value) -> Result<ProvenanceLabel, InputError> {
    let object = expect_object(value, "$.provenance")?;
    reject_unknown_fields(
        object,
        &[
            "corpus",
            "source_uri",
            "sha256",
            "tokens",
            "vintage",
            "languages",
            "annotator_labor",
            "notes",
        ],
        "$.provenance",
    )?;
    let corpus = required_string(object, "corpus", "$.provenance")?;
    let source_uri = required_string(object, "source_uri", "$.provenance")?;
    let corpus_sha256 = required_sha256(object, "sha256", "$.provenance")?;
    let vintage = required_string(object, "vintage", "$.provenance")?;
    let tokens = required_u64(object, "tokens", "$.provenance")?;

    let labor = match required_string(object, "annotator_labor", "$.provenance")?.as_str() {
        "credited" => Labor::Credited,
        "uncredited" => Labor::Uncredited,
        _ => {
            return Err(InputError::new(
                "$.provenance.annotator_labor",
                "expected `credited` or `uncredited`",
            ));
        }
    };

    let languages_value = required(object, "languages", "$.provenance")?;
    let languages_array = expect_array(languages_value, "$.provenance.languages")?;
    if languages_array.is_empty() {
        return Err(InputError::new(
            "$.provenance.languages",
            "at least one language share is required",
        ));
    }
    let mut languages = Vec::with_capacity(languages_array.len());
    let mut language_codes = BTreeSet::new();
    let mut total_share = 0.0;
    for (index, item) in languages_array.iter().enumerate() {
        let path = format!("$.provenance.languages[{index}]");
        let item = expect_object(item, &path)?;
        reject_unknown_fields(item, &["code", "share"], &path)?;
        let code = required_string(item, "code", &path)?;
        if !language_codes.insert(code.clone()) {
            return Err(InputError::new(
                format!("{path}.code"),
                format!("duplicate language code {code:?}"),
            ));
        }
        let share = required_number(item, "share", &path)?;
        if !share.is_finite() || !(0.0..=1.0).contains(&share) {
            return Err(InputError::new(
                format!("{path}.share"),
                "share must be a finite number between 0 and 1",
            ));
        }
        total_share += share;
        languages.push((code, share));
    }
    if (total_share - 1.0_f64).abs() > 1e-6 {
        return Err(InputError::new(
            "$.provenance.languages",
            format!("language shares must total 1.0; got {total_share}"),
        ));
    }

    let notes_value = required(object, "notes", "$.provenance")?;
    let notes_array = expect_array(notes_value, "$.provenance.notes")?;
    let mut notes = Vec::with_capacity(notes_array.len());
    for (index, note) in notes_array.iter().enumerate() {
        notes.push(expect_string(
            note,
            &format!("$.provenance.notes[{index}]"),
        )?);
    }

    Ok(ProvenanceLabel {
        corpus,
        source_uri,
        corpus_sha256,
        tokens,
        vintage,
        languages,
        annotator_labor: labor,
        notes,
    })
}

fn parse_ontology(value: &Value) -> Result<OntologySnapshot, InputError> {
    let object = expect_object(value, "$.ontology")?;
    reject_unknown_fields(object, &["name", "version", "concepts"], "$.ontology")?;
    let name = required_string(object, "name", "$.ontology")?;
    let version = required_string(object, "version", "$.ontology")?;
    let concepts_value = required(object, "concepts", "$.ontology")?;
    let concepts_object = expect_object(concepts_value, "$.ontology.concepts")?;
    if concepts_object.is_empty() {
        return Err(InputError::new(
            "$.ontology.concepts",
            "at least one concept is required",
        ));
    }
    let mut concepts = Vec::with_capacity(concepts_object.len());
    for (concept, definition) in concepts_object {
        if concept.trim().is_empty() {
            return Err(InputError::new(
                "$.ontology.concepts",
                "concept names must not be empty",
            ));
        }
        concepts.push((
            concept.clone(),
            expect_string(definition, &format!("$.ontology.concepts.{concept}"))?,
        ));
    }
    Ok(OntologySnapshot::new(&name, &version, concepts))
}

fn parse_behavior(value: &Value) -> Result<Vec<(String, BehaviorReading)>, InputError> {
    let object = expect_object(value, "$.behavior")?;
    reject_unknown_fields(object, &["cases"], "$.behavior")?;
    let cases = expect_object(required(object, "cases", "$.behavior")?, "$.behavior.cases")?;
    if cases.is_empty() {
        return Err(InputError::new(
            "$.behavior.cases",
            "at least one behavior case is required",
        ));
    }
    let mut parsed = Vec::with_capacity(cases.len());
    for (case_id, value) in cases {
        if case_id.trim().is_empty() {
            return Err(InputError::new(
                "$.behavior.cases",
                "behavior case IDs must not be empty",
            ));
        }
        let path = format!("$.behavior.cases.{case_id}");
        let reading = expect_object(value, &path)?;
        reject_unknown_fields(reading, &["input", "classification", "route"], &path)?;
        parsed.push((
            case_id.clone(),
            BehaviorReading {
                input: required_string(reading, "input", &path)?,
                classification: required_string(reading, "classification", &path)?,
                route: required_string(reading, "route", &path)?,
            },
        ));
    }
    Ok(parsed)
}

fn required<'a>(
    object: &'a BTreeMap<String, Value>,
    key: &str,
    path: &str,
) -> Result<&'a Value, InputError> {
    object
        .get(key)
        .ok_or_else(|| InputError::new(format!("{path}.{key}"), "required field is missing"))
}

fn reject_unknown_fields(
    object: &BTreeMap<String, Value>,
    allowed: &[&str],
    path: &str,
) -> Result<(), InputError> {
    if let Some(field) = object
        .keys()
        .find(|field| !allowed.contains(&field.as_str()))
    {
        return Err(InputError::new(
            format!("{path}.{field}"),
            "unknown field; the strict envelope contract rejects data it cannot preserve",
        ));
    }
    Ok(())
}

fn required_string(
    object: &BTreeMap<String, Value>,
    key: &str,
    path: &str,
) -> Result<String, InputError> {
    expect_string(required(object, key, path)?, &format!("{path}.{key}"))
}

fn required_sha256(
    object: &BTreeMap<String, Value>,
    key: &str,
    path: &str,
) -> Result<String, InputError> {
    let value = required_string(object, key, path)?;
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(InputError::new(
            format!("{path}.{key}"),
            "expected a 64-character SHA-256 hex digest",
        ));
    }
    Ok(value.to_ascii_lowercase())
}

fn required_number(
    object: &BTreeMap<String, Value>,
    key: &str,
    path: &str,
) -> Result<f64, InputError> {
    let field_path = format!("{path}.{key}");
    match required(object, key, path)? {
        Value::Num(number) => Ok(*number),
        value => Err(InputError::new(
            field_path,
            format!("expected number, got {}", value.type_name()),
        )),
    }
}

fn required_u64(
    object: &BTreeMap<String, Value>,
    key: &str,
    path: &str,
) -> Result<u64, InputError> {
    let number = required_number(object, key, path)?;
    if !number.is_finite()
        || number < 0.0
        || number.fract() != 0.0
        || number > 9_007_199_254_740_991.0
    {
        return Err(InputError::new(
            format!("{path}.{key}"),
            "expected a non-negative integer no larger than JSON's exact-integer limit",
        ));
    }
    Ok(number as u64)
}

fn expect_object<'a>(
    value: &'a Value,
    path: &str,
) -> Result<&'a BTreeMap<String, Value>, InputError> {
    match value {
        Value::Obj(object) => Ok(object),
        value => Err(InputError::new(
            path,
            format!("expected object, got {}", value.type_name()),
        )),
    }
}

fn expect_array<'a>(value: &'a Value, path: &str) -> Result<&'a [Value], InputError> {
    match value {
        Value::Arr(array) => Ok(array),
        value => Err(InputError::new(
            path,
            format!("expected array, got {}", value.type_name()),
        )),
    }
}

fn expect_string(value: &Value, path: &str) -> Result<String, InputError> {
    match value {
        Value::Str(string) if !string.trim().is_empty() => Ok(string.clone()),
        Value::Str(_) => Err(InputError::new(path, "string must not be empty")),
        value => Err(InputError::new(
            path,
            format!("expected string, got {}", value.type_name()),
        )),
    }
}

fn report_value(
    comparison: &EnvelopeComparison,
    behavior: &BehaviorProbe,
    drift_detected: bool,
) -> Value {
    Value::Obj(BTreeMap::from([
        (
            "comparison".into(),
            Value::Obj(BTreeMap::from([
                (
                    "behavior_probe".into(),
                    Value::Obj(BTreeMap::from([
                        (
                            "baseline".into(),
                            behavior_reading_value(&behavior.baseline),
                        ),
                        ("case_id".into(), Value::Str(behavior.case_id.clone())),
                        ("changed".into(), Value::Bool(behavior.changed())),
                        ("input".into(), Value::Str(behavior.input.clone())),
                        (
                            "observed".into(),
                            behavior_reading_value(&behavior.observed),
                        ),
                    ])),
                ),
                (
                    "ontology".into(),
                    Value::Obj(BTreeMap::from([
                        (
                            "added".into(),
                            string_array(&comparison.ontology_drift.added),
                        ),
                        (
                            "from_version".into(),
                            Value::Str(comparison.ontology_drift.from_version.clone()),
                        ),
                        (
                            "name".into(),
                            Value::Str(comparison.ontology_drift.ontology.clone()),
                        ),
                        (
                            "redefined".into(),
                            string_array(&comparison.ontology_drift.redefined),
                        ),
                        (
                            "removed".into(),
                            string_array(&comparison.ontology_drift.removed),
                        ),
                        (
                            "to_version".into(),
                            Value::Str(comparison.ontology_drift.to_version.clone()),
                        ),
                    ])),
                ),
                (
                    "semantic_probe".into(),
                    Value::Obj(BTreeMap::from([
                        (
                            "baseline".into(),
                            reading_value(&comparison.probe.baseline_definition),
                        ),
                        ("changed".into(), Value::Bool(comparison.probe.changed())),
                        (
                            "concept".into(),
                            Value::Str(comparison.probe.concept.clone()),
                        ),
                        (
                            "observed".into(),
                            reading_value(&comparison.probe.observed_definition),
                        ),
                    ])),
                ),
            ])),
        ),
        (
            "envelopes".into(),
            Value::Obj(BTreeMap::from([
                ("baseline".into(), envelope_value(&comparison.baseline)),
                ("observed".into(), envelope_value(&comparison.observed)),
            ])),
        ),
        (
            "status".into(),
            Value::Str(
                if drift_detected {
                    "drift_detected"
                } else {
                    "stable"
                }
                .into(),
            ),
        ),
    ]))
}

fn envelope_value(envelope: &ProvenanceEnvelope) -> Value {
    Value::Obj(BTreeMap::from([
        (
            "artifact".into(),
            Value::Obj(BTreeMap::from([
                ("id".into(), Value::Str(envelope.artifact_id.clone())),
                (
                    "source_uri".into(),
                    Value::Str(envelope.artifact_uri.clone()),
                ),
                (
                    "sha256".into(),
                    Value::Str(envelope.artifact_sha256.clone()),
                ),
                (
                    "version".into(),
                    Value::Str(envelope.artifact_version.clone()),
                ),
            ])),
        ),
        (
            "behavior".into(),
            Value::Obj(BTreeMap::from([(
                "cases".into(),
                Value::Obj(
                    envelope
                        .behavior_cases
                        .iter()
                        .map(|(case_id, reading)| {
                            (case_id.clone(), behavior_reading_value(reading))
                        })
                        .collect(),
                ),
            )])),
        ),
        ("ontology".into(), ontology_value(&envelope.ontology)),
        ("provenance".into(), provenance_value(&envelope.provenance)),
    ]))
}

fn provenance_value(label: &ProvenanceLabel) -> Value {
    Value::Obj(BTreeMap::from([
        (
            "annotator_labor".into(),
            Value::Str(
                match label.annotator_labor {
                    Labor::Credited => "credited",
                    Labor::Uncredited => "uncredited",
                }
                .into(),
            ),
        ),
        ("corpus".into(), Value::Str(label.corpus.clone())),
        ("sha256".into(), Value::Str(label.corpus_sha256.clone())),
        ("source_uri".into(), Value::Str(label.source_uri.clone())),
        (
            "languages".into(),
            Value::Arr(
                label
                    .languages
                    .iter()
                    .map(|(code, share)| {
                        Value::Obj(BTreeMap::from([
                            ("code".into(), Value::Str(code.clone())),
                            ("share".into(), Value::Num(*share)),
                        ]))
                    })
                    .collect(),
            ),
        ),
        (
            "notes".into(),
            Value::Arr(label.notes.iter().cloned().map(Value::Str).collect()),
        ),
        ("tokens".into(), Value::Num(label.tokens as f64)),
        ("vintage".into(), Value::Str(label.vintage.clone())),
    ]))
}

fn behavior_reading_value(reading: &BehaviorReading) -> Value {
    Value::Obj(BTreeMap::from([
        (
            "classification".into(),
            Value::Str(reading.classification.clone()),
        ),
        ("input".into(), Value::Str(reading.input.clone())),
        ("route".into(), Value::Str(reading.route.clone())),
    ]))
}

fn ontology_value(snapshot: &OntologySnapshot) -> Value {
    Value::Obj(BTreeMap::from([
        (
            "concepts".into(),
            Value::Obj(
                snapshot
                    .concepts
                    .iter()
                    .map(|(concept, definition)| (concept.clone(), Value::Str(definition.clone())))
                    .collect(),
            ),
        ),
        ("name".into(), Value::Str(snapshot.name.clone())),
        ("version".into(), Value::Str(snapshot.version.clone())),
    ]))
}

fn reading_value(definition: &Option<String>) -> Value {
    Value::Obj(BTreeMap::from([
        (
            "definition".into(),
            definition
                .as_ref()
                .map_or(Value::Null, |definition| Value::Str(definition.clone())),
        ),
        ("recognized".into(), Value::Bool(definition.is_some())),
    ]))
}

fn string_array(values: &[String]) -> Value {
    Value::Arr(values.iter().cloned().map(Value::Str).collect())
}

fn rejection(stage: &str, code: &str, message: &str, path: Option<&str>) -> Value {
    let mut error = BTreeMap::from([
        ("code".into(), Value::Str(code.into())),
        ("message".into(), Value::Str(message.into())),
        ("stage".into(), Value::Str(stage.into())),
    ]);
    if let Some(path) = path {
        error.insert("path".into(), Value::Str(path.into()));
    }
    Value::Obj(BTreeMap::from([
        ("error".into(), Value::Obj(error)),
        ("status".into(), Value::Str("rejected".into())),
    ]))
}

fn status(value: &Value) -> Option<&str> {
    match value {
        Value::Obj(object) => match object.get("status") {
            Some(Value::Str(status)) => Some(status),
            _ => None,
        },
        _ => None,
    }
}

fn semantic_probe_changed(value: &Value) -> Option<bool> {
    let report = value.as_obj()?;
    let comparison = report.get("comparison")?.as_obj()?;
    let probe = comparison.get("semantic_probe")?.as_obj()?;
    match probe.get("changed") {
        Some(Value::Bool(changed)) => Some(*changed),
        _ => None,
    }
}

fn behavior_probe_changed(value: &Value) -> Option<bool> {
    let report = value.as_obj()?;
    let comparison = report.get("comparison")?.as_obj()?;
    let probe = comparison.get("behavior_probe")?.as_obj()?;
    match probe.get("changed") {
        Some(Value::Bool(changed)) => Some(*changed),
        _ => None,
    }
}

fn run_demo() -> i32 {
    println!("Strata POC — the sediment carries its history\n");
    let (report, code) = analyze(DEMO_BASELINE, DEMO_OBSERVED, "nuclear", "chosen-caregiver");
    println!(
        "1. Compare two provenance envelopes, probe `nuclear`, and replay `chosen-caregiver`\n{report}\n"
    );
    if code == 2
        && status(&report) == Some("drift_detected")
        && semantic_probe_changed(&report) == Some(true)
        && behavior_probe_changed(&report) == Some(true)
    {
        println!(
            "Proof: the same concept names a different social boundary, and both histories remain attached."
        );
        0
    } else {
        println!("Proof failed: expected a routed drift report.");
        1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn demo_detects_and_serializes_semantic_drift() {
        let (report, code) = analyze(DEMO_BASELINE, DEMO_OBSERVED, "nuclear", "chosen-caregiver");
        assert_eq!(code, 2);
        assert_eq!(status(&report), Some("drift_detected"));
        assert!(parse(&report.to_string()).is_ok());

        let object = report.as_obj().unwrap();
        let comparison = object.get("comparison").unwrap().as_obj().unwrap();
        let probe = comparison.get("semantic_probe").unwrap().as_obj().unwrap();
        assert_eq!(probe.get("changed"), Some(&Value::Bool(true)));
        assert_eq!(semantic_probe_changed(&report), Some(true));
        assert_eq!(behavior_probe_changed(&report), Some(true));

        let behavior = comparison.get("behavior_probe").unwrap().as_obj().unwrap();
        let baseline = behavior.get("baseline").unwrap().as_obj().unwrap();
        let observed = behavior.get("observed").unwrap().as_obj().unwrap();
        assert_eq!(
            baseline.get("classification"),
            Some(&Value::Str("other".into()))
        );
        assert_eq!(
            observed.get("classification"),
            Some(&Value::Str("chosen".into()))
        );
    }

    #[test]
    fn identical_envelopes_are_stable() {
        let (report, code) = analyze(DEMO_BASELINE, DEMO_BASELINE, "nuclear", "chosen-caregiver");
        assert_eq!(code, 0);
        assert_eq!(status(&report), Some("stable"));
    }

    #[test]
    fn provenance_is_preserved_on_both_sides() {
        let (report, _) = analyze(DEMO_BASELINE, DEMO_OBSERVED, "chosen", "chosen-caregiver");
        let envelopes = report
            .as_obj()
            .unwrap()
            .get("envelopes")
            .unwrap()
            .as_obj()
            .unwrap();
        for side in ["baseline", "observed"] {
            let provenance = envelopes
                .get(side)
                .unwrap()
                .as_obj()
                .unwrap()
                .get("provenance")
                .unwrap()
                .as_obj()
                .unwrap();
            assert!(provenance.contains_key("corpus"));
            assert!(provenance.contains_key("annotator_labor"));
            assert!(provenance.contains_key("source_uri"));
            assert!(provenance.contains_key("sha256"));
        }
    }

    #[test]
    fn invalid_language_shares_are_rejected() {
        let invalid = DEMO_BASELINE.replace(
            "{ \"code\": \"other\", \"share\": 0.06 }",
            "{ \"code\": \"other\", \"share\": 0.60 }",
        );
        let (report, code) = analyze(&invalid, DEMO_OBSERVED, "nuclear", "chosen-caregiver");
        assert_eq!(code, 1);
        assert_eq!(status(&report), Some("rejected"));
        assert!(report.to_string().contains("language shares must total"));
    }

    #[test]
    fn unrelated_artifacts_are_rejected() {
        let unrelated = DEMO_OBSERVED.replace("family-support-router", "risk-router");
        let (report, code) = analyze(DEMO_BASELINE, &unrelated, "nuclear", "chosen-caregiver");
        assert_eq!(code, 1);
        assert_eq!(status(&report), Some("rejected"));
        assert!(report.to_string().contains("artifact mismatch"));
    }

    #[test]
    fn unknown_envelope_fields_are_rejected_instead_of_dropped() {
        let extended = DEMO_BASELINE.replace(
            "\"corpus\": \"archived-support-tickets\"",
            "\"license\": \"consent-required\",\n    \"corpus\": \"archived-support-tickets\"",
        );
        let (report, code) = analyze(&extended, DEMO_OBSERVED, "nuclear", "chosen-caregiver");
        assert_eq!(code, 1);
        assert_eq!(status(&report), Some("rejected"));
        assert!(report.to_string().contains("$.provenance.license"));
        assert!(report.to_string().contains("unknown field"));
    }

    #[test]
    fn unknown_probe_is_rejected_instead_of_reported_stable() {
        let (report, code) = analyze(DEMO_BASELINE, DEMO_BASELINE, "nucelar", "chosen-caregiver");
        assert_eq!(code, 1);
        assert_eq!(status(&report), Some("rejected"));
        assert!(report.to_string().contains("unknown_probe"));
    }

    #[test]
    fn unknown_behavior_case_is_rejected() {
        let (report, code) = analyze(DEMO_BASELINE, DEMO_OBSERVED, "nuclear", "missing-case");
        assert_eq!(code, 1);
        assert_eq!(status(&report), Some("rejected"));
        assert!(report.to_string().contains("invalid_behavior_probe"));
    }
}
