//! A product-shaped Strata proof: two provenance-carrying ontology snapshots
//! in, and one machine-readable semantic-drift report out.

mod sha256;

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

use sha256::hex_digest;

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
    let mut baseline = match parse_envelope(baseline_raw) {
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
    let mut observed = match parse_envelope(observed_raw) {
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

    let baseline_router = match verify_envelope(&mut baseline, "baseline") {
        Ok(router) => router,
        Err(error) => {
            return (
                rejection(
                    "baseline",
                    "verification_failed",
                    &error.message,
                    Some(&error.path),
                ),
                1,
            );
        }
    };
    let observed_router = match verify_envelope(&mut observed, "observed") {
        Ok(router) => router,
        Err(error) => {
            return (
                rejection(
                    "observed",
                    "verification_failed",
                    &error.message,
                    Some(&error.path),
                ),
                1,
            );
        }
    };
    materialize_behavior(&mut baseline, &baseline_router, case_id);
    materialize_behavior(&mut observed, &observed_router, case_id);

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

#[derive(Debug, Clone, PartialEq, Eq)]
struct BehaviorDecision {
    classification: String,
    route: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RouterRule {
    contains: String,
    decision: BehaviorDecision,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RouterArtifact {
    artifact_id: String,
    artifact_version: String,
    rules: Vec<RouterRule>,
    default: BehaviorDecision,
}

impl RouterArtifact {
    fn evaluate(&self, input: &str) -> BehaviorDecision {
        self.rules
            .iter()
            .find(|rule| input.contains(&rule.contains))
            .map_or_else(|| self.default.clone(), |rule| rule.decision.clone())
    }
}

fn verify_envelope(
    envelope: &mut ProvenanceEnvelope,
    stage: &str,
) -> Result<RouterArtifact, InputError> {
    let artifact = read_committed(
        &envelope.artifact_uri,
        &envelope.artifact_sha256,
        &format!("$.{stage}.artifact"),
    )?;
    read_committed(
        &envelope.provenance.source_uri,
        &envelope.provenance.corpus_sha256,
        &format!("$.{stage}.provenance"),
    )?;
    let behavior = read_committed(
        &envelope.behavior_uri,
        &envelope.behavior_sha256,
        &format!("$.{stage}.behavior"),
    )?;

    let router = parse_router(&artifact)?;
    if router.artifact_id != envelope.artifact_id
        || router.artifact_version != envelope.artifact_version
    {
        return Err(InputError::new(
            format!("$.{stage}.artifact"),
            "artifact bytes do not identify the envelope's artifact ID and version",
        ));
    }
    envelope.behavior_cases = parse_behavior_cases(&behavior)?.into_iter().collect();
    Ok(router)
}

fn read_committed(source_uri: &str, expected: &str, path: &str) -> Result<String, InputError> {
    let source_path = PathBuf::from(source_uri);
    let source_path = if source_path.is_absolute() {
        source_path
    } else {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(source_path)
    };
    let bytes = fs::read(&source_path).map_err(|error| {
        InputError::new(
            format!("{path}.source_uri"),
            format!("cannot read committed source {source_uri:?}: {error}"),
        )
    })?;
    let observed = hex_digest(&bytes);
    if observed != expected {
        return Err(InputError::new(
            format!("{path}.sha256"),
            format!("commitment mismatch: observed {observed}, expected {expected}"),
        ));
    }
    String::from_utf8(bytes).map_err(|_| {
        InputError::new(
            format!("{path}.source_uri"),
            "committed source must be UTF-8",
        )
    })
}

fn parse_router(raw: &str) -> Result<RouterArtifact, InputError> {
    let value = parse(raw).map_err(|error| {
        InputError::new(
            format!("byte {}", error.at),
            format!("invalid router JSON: {}", error.msg),
        )
    })?;
    let object = expect_object(&value, "$.artifact_source")?;
    reject_unknown_fields(
        object,
        &[
            "format",
            "artifact_id",
            "artifact_version",
            "rules",
            "default",
        ],
        "$.artifact_source",
    )?;
    let format = required_string(object, "format", "$.artifact_source")?;
    if format != "strata-router/v1" {
        return Err(InputError::new(
            "$.artifact_source.format",
            "expected `strata-router/v1`",
        ));
    }
    let rules_value = required(object, "rules", "$.artifact_source")?;
    let rules_array = expect_array(rules_value, "$.artifact_source.rules")?;
    let mut rules = Vec::with_capacity(rules_array.len());
    for (index, value) in rules_array.iter().enumerate() {
        let path = format!("$.artifact_source.rules[{index}]");
        let rule = expect_object(value, &path)?;
        reject_unknown_fields(rule, &["contains", "classification", "route"], &path)?;
        rules.push(RouterRule {
            contains: required_string(rule, "contains", &path)?,
            decision: parse_decision(rule, &path)?,
        });
    }
    let default_path = "$.artifact_source.default";
    let default = expect_object(
        required(object, "default", "$.artifact_source")?,
        default_path,
    )?;
    reject_unknown_fields(default, &["classification", "route"], default_path)?;

    Ok(RouterArtifact {
        artifact_id: required_string(object, "artifact_id", "$.artifact_source")?,
        artifact_version: required_string(object, "artifact_version", "$.artifact_source")?,
        rules,
        default: parse_decision(default, default_path)?,
    })
}

fn parse_decision(
    object: &BTreeMap<String, Value>,
    path: &str,
) -> Result<BehaviorDecision, InputError> {
    Ok(BehaviorDecision {
        classification: required_string(object, "classification", path)?,
        route: required_string(object, "route", path)?,
    })
}

fn materialize_behavior(envelope: &mut ProvenanceEnvelope, router: &RouterArtifact, case_id: &str) {
    if let Some(reading) = envelope.behavior_cases.get_mut(case_id) {
        let decision = router.evaluate(&reading.input);
        reading.classification = decision.classification;
        reading.route = decision.route;
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
    let (behavior_uri, behavior_sha256) = parse_behavior_source(behavior_value)?;

    Ok(ProvenanceEnvelope::new(
        &artifact_id,
        &artifact_version,
        &artifact_uri,
        &artifact_sha256,
        provenance,
        ontology,
    )
    .with_behavior_source(&behavior_uri, &behavior_sha256))
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

fn parse_behavior_source(value: &Value) -> Result<(String, String), InputError> {
    let object = expect_object(value, "$.behavior")?;
    reject_unknown_fields(object, &["source_uri", "sha256"], "$.behavior")?;
    Ok((
        required_string(object, "source_uri", "$.behavior")?,
        required_sha256(object, "sha256", "$.behavior")?,
    ))
}

fn parse_behavior_cases(raw: &str) -> Result<Vec<(String, BehaviorReading)>, InputError> {
    let value = parse(raw).map_err(|error| {
        InputError::new(
            format!("byte {}", error.at),
            format!("invalid behavior-case JSON: {}", error.msg),
        )
    })?;
    let object = expect_object(&value, "$.behavior_source")?;
    reject_unknown_fields(object, &["format", "cases"], "$.behavior_source")?;
    let format = required_string(object, "format", "$.behavior_source")?;
    if format != "strata-cases/v1" {
        return Err(InputError::new(
            "$.behavior_source.format",
            "expected `strata-cases/v1`",
        ));
    }
    let cases = expect_object(
        required(object, "cases", "$.behavior_source")?,
        "$.behavior_source.cases",
    )?;
    if cases.is_empty() {
        return Err(InputError::new(
            "$.behavior_source.cases",
            "at least one behavior case is required",
        ));
    }
    let mut parsed = Vec::with_capacity(cases.len());
    for (case_id, value) in cases {
        if case_id.trim().is_empty() {
            return Err(InputError::new(
                "$.behavior_source.cases",
                "behavior case IDs must not be empty",
            ));
        }
        let path = format!("$.behavior_source.cases.{case_id}");
        let reading = expect_object(value, &path)?;
        reject_unknown_fields(reading, &["input"], &path)?;
        parsed.push((
            case_id.clone(),
            BehaviorReading {
                input: required_string(reading, "input", &path)?,
                classification: String::new(),
                route: String::new(),
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
        (
            "verification".into(),
            Value::Obj(BTreeMap::from([
                (
                    "behavior_engine".into(),
                    Value::Str("strata-router/v1".into()),
                ),
                ("commitments_verified".into(), Value::Bool(true)),
            ])),
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
            Value::Obj(BTreeMap::from([
                (
                    "cases".into(),
                    Value::Obj(
                        envelope
                            .behavior_cases
                            .iter()
                            .map(|(case_id, reading)| {
                                (
                                    case_id.clone(),
                                    Value::Obj(BTreeMap::from([(
                                        "input".into(),
                                        Value::Str(reading.input.clone()),
                                    )])),
                                )
                            })
                            .collect(),
                    ),
                ),
                (
                    "sha256".into(),
                    Value::Str(envelope.behavior_sha256.clone()),
                ),
                (
                    "source_uri".into(),
                    Value::Str(envelope.behavior_uri.clone()),
                ),
            ])),
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
        "1. Verify two provenance envelopes, probe `nuclear`, and execute `chosen-caregiver`\n{report}\n"
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
        assert!(report
            .to_string()
            .contains("artifact bytes do not identify"));
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

    #[test]
    fn a_declared_hash_that_does_not_match_bytes_is_rejected() {
        let tampered = DEMO_BASELINE.replace(
            "c46701df82ea273c52dfbaabb32a6d0ebed76a7b1bb283a28270529da7e0f208",
            "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc",
        );

        let (report, code) = analyze(&tampered, DEMO_OBSERVED, "nuclear", "chosen-caregiver");

        assert_eq!(code, 1);
        assert_eq!(status(&report), Some("rejected"));
        assert!(report.to_string().contains("commitment mismatch"));
    }

    #[test]
    fn router_executes_rules_and_default_instead_of_accepting_recorded_outputs() {
        let router = parse_router(include_str!(
            "../../../examples/strata-poc/candidate-v2.artifact.json"
        ))
        .unwrap();

        assert_eq!(
            router
                .evaluate("Alex names a chosen-family caregiver")
                .classification,
            "chosen"
        );
        assert_eq!(
            router.evaluate("Alex names an aunt as caregiver").route,
            "manual-review"
        );
    }
}
