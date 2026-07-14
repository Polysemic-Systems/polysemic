//! A product-shaped Digest proof: strict schema in, unreliable model text in,
//! and one machine-readable envelope out.

use digest::{
    digest, digest_with_answers, Answer, AnswerError, Digestion, Outcome, Repair, Schema,
};
use polysemic_core::{parse, ParseError, Value};
use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::io::{self, Read};
use std::path::PathBuf;

const DEMO_SCHEMA: &str = include_str!("../../../examples/digest-poc/order.schema.json");
const DEMO_OUTPUT: &str = include_str!("../../../examples/digest-poc/model-output.txt");

const HELP: &str = r#"digest-poc — ambiguity becomes a question, never a guess

USAGE
  digest-poc demo
  digest-poc check  --schema FILE [--input FILE]
  digest-poc answer --schema FILE [--input FILE] --answer '$.path=<json>' [...]

If --input is omitted, model output is read from stdin.

EXIT STATUS
  0  resolved: safe to hand to deterministic infrastructure
  2  clarify: route the emitted questions to a human or policy layer
  1  rejected: schema, input, or answer contract is invalid
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
        "check" => run_command(&args[1..], false),
        "answer" => run_command(&args[1..], true),
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
                    None
                )
            );
            1
        }
    }
}

#[derive(Default)]
struct Options {
    schema: Option<PathBuf>,
    input: Option<PathBuf>,
    answers: Vec<String>,
}

fn run_command(args: &[String], allow_answers: bool) -> i32 {
    let options = match parse_options(args, allow_answers) {
        Ok(options) => options,
        Err(message) => {
            println!("{}", rejection("cli", "invalid_arguments", &message, None));
            return 1;
        }
    };
    let schema_path = match options.schema {
        Some(path) => path,
        None => {
            println!(
                "{}",
                rejection("cli", "missing_schema", "--schema FILE is required", None)
            );
            return 1;
        }
    };
    if allow_answers && options.answers.is_empty() {
        println!(
            "{}",
            rejection(
                "cli",
                "missing_answer",
                "answer requires at least one --answer '$.path=<json>'",
                None,
            )
        );
        return 1;
    }

    let schema = match fs::read_to_string(&schema_path) {
        Ok(schema) => schema,
        Err(error) => {
            println!(
                "{}",
                rejection(
                    "schema",
                    "read_failed",
                    &format!("{}: {error}", schema_path.display()),
                    None,
                )
            );
            return 1;
        }
    };
    let raw = match read_input(options.input) {
        Ok(raw) => raw,
        Err(message) => {
            println!("{}", rejection("output", "read_failed", &message, None));
            return 1;
        }
    };
    let answers = if allow_answers {
        match options
            .answers
            .iter()
            .map(|answer| parse_answer(answer))
            .collect::<Result<Vec<_>, _>>()
        {
            Ok(answers) => Some(answers),
            Err(message) => {
                println!("{}", rejection("answer", "invalid_answer", &message, None));
                return 1;
            }
        }
    } else {
        None
    };

    let (envelope, code) = metabolize(&schema, &raw, answers);
    println!("{envelope}");
    code
}

fn parse_options(args: &[String], allow_answers: bool) -> Result<Options, String> {
    let mut options = Options::default();
    let mut index = 0;
    while index < args.len() {
        let flag = args[index].as_str();
        let value = args
            .get(index + 1)
            .ok_or_else(|| format!("{flag} requires a value"))?;
        match flag {
            "--schema" if options.schema.is_none() => options.schema = Some(value.into()),
            "--input" if options.input.is_none() => options.input = Some(value.into()),
            "--answer" if allow_answers => options.answers.push(value.clone()),
            "--answer" => return Err("check does not accept --answer; use `answer`".into()),
            "--schema" | "--input" => return Err(format!("{flag} may be provided only once")),
            _ => return Err(format!("unknown argument {flag:?}")),
        }
        index += 2;
    }
    Ok(options)
}

fn read_input(path: Option<PathBuf>) -> Result<String, String> {
    match path {
        Some(path) => {
            fs::read_to_string(&path).map_err(|error| format!("{}: {error}", path.display()))
        }
        None => {
            let mut raw = String::new();
            io::stdin()
                .read_to_string(&mut raw)
                .map_err(|error| error.to_string())?;
            Ok(raw)
        }
    }
}

fn parse_answer(raw: &str) -> Result<Answer, String> {
    let (path, value) = raw
        .split_once('=')
        .ok_or_else(|| "expected '$.path=<json>'".to_string())?;
    if !path.starts_with('$') {
        return Err(format!("answer path {path:?} must start with `$`"));
    }
    let value = parse(value).map_err(|error| format!("invalid JSON value for {path}: {error}"))?;
    Ok(Answer::new(path, value))
}

fn metabolize(schema_raw: &str, raw: &str, answers: Option<Vec<Answer>>) -> (Value, i32) {
    let schema = match Schema::from_json_schema(schema_raw) {
        Ok(schema) => schema,
        Err(error) => {
            return (
                rejection(
                    "schema",
                    "unsupported_schema",
                    &error.message,
                    Some(&error.path),
                ),
                1,
            )
        }
    };

    let result = match answers {
        Some(answers) => digest_with_answers(raw, &schema, answers).map_err(DigestFailure::Answer),
        None => digest(raw, &schema).map_err(DigestFailure::Parse),
    };
    match result {
        Ok(digestion) => {
            let code = if digestion.is_resolved() { 0 } else { 2 };
            (digestion_envelope(&digestion), code)
        }
        Err(error) => (failure_envelope(error), 1),
    }
}

enum DigestFailure {
    Parse(ParseError),
    Answer(AnswerError),
}

fn failure_envelope(error: DigestFailure) -> Value {
    match error {
        DigestFailure::Parse(error) | DigestFailure::Answer(AnswerError::Parse(error)) => {
            rejection(
                "output",
                "unrecoverable_output",
                &error.msg,
                Some(&format!("byte {}", error.at)),
            )
        }
        DigestFailure::Answer(AnswerError::NotRequested { path }) => rejection(
            "answer",
            "not_requested",
            "an answer may change only a path Digest questioned",
            Some(&path),
        ),
        DigestFailure::Answer(AnswerError::Duplicate { path }) => rejection(
            "answer",
            "duplicate_answer",
            "the same path was answered more than once",
            Some(&path),
        ),
        DigestFailure::Answer(AnswerError::InvalidPath { path }) => rejection(
            "answer",
            "invalid_path",
            "the questioned path cannot be updated in this value",
            Some(&path),
        ),
    }
}

fn digestion_envelope(digestion: &Digestion) -> Value {
    let mut envelope = BTreeMap::new();
    envelope.insert(
        "answers".into(),
        Value::Arr(digestion.answers.iter().map(answer_value).collect()),
    );
    envelope.insert(
        "repairs".into(),
        Value::Arr(digestion.repairs.iter().map(repair_value).collect()),
    );
    match &digestion.outcome {
        Outcome::Resolved(value) => {
            envelope.insert("status".into(), Value::Str("resolved".into()));
            envelope.insert("value".into(), value.clone());
        }
        Outcome::Clarify(questions) => {
            envelope.insert(
                "questions".into(),
                Value::Arr(questions.iter().map(question_value).collect()),
            );
            envelope.insert("status".into(), Value::Str("clarify".into()));
        }
    }
    Value::Obj(envelope)
}

fn repair_value(repair: &Repair) -> Value {
    let mut fields = BTreeMap::from([
        ("code".into(), Value::Str(repair.code().into())),
        ("message".into(), Value::Str(repair.to_string())),
    ]);
    match repair {
        Repair::Coerced { path, from, to } => {
            fields.insert("path".into(), Value::Str(path.clone()));
            fields.insert("from".into(), Value::Str(from.clone()));
            fields.insert("to".into(), Value::Str(to.clone()));
        }
        Repair::HedgeResolved { path, original } | Repair::CaseFolded { path, original } => {
            fields.insert("path".into(), Value::Str(path.clone()));
            fields.insert("original".into(), Value::Str(original.clone()));
        }
        _ => {}
    }
    Value::Obj(fields)
}

fn question_value(question: &digest::Question) -> Value {
    Value::Obj(BTreeMap::from([
        (
            "candidates".into(),
            Value::Arr(
                question
                    .candidates
                    .iter()
                    .map(|candidate| Value::Str(candidate.clone()))
                    .collect(),
            ),
        ),
        ("path".into(), Value::Str(question.path.clone())),
        ("prompt".into(), Value::Str(question.prompt.clone())),
    ]))
}

fn answer_value(answer: &Answer) -> Value {
    Value::Obj(BTreeMap::from([
        ("path".into(), Value::Str(answer.path.clone())),
        ("value".into(), answer.value.clone()),
    ]))
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

fn run_demo() -> i32 {
    println!("Digest POC — the metabolism at the probabilistic seam\n");
    let (first, first_code) = metabolize(DEMO_SCHEMA, DEMO_OUTPUT, None);
    println!("1. Check unreliable model output\n{first}\n");
    let (second, second_code) = metabolize(
        DEMO_SCHEMA,
        DEMO_OUTPUT,
        Some(vec![Answer::new("$.qty", Value::Num(2.0))]),
    );
    println!("2. Apply only the answer Digest requested\n{second}\n");
    if first_code == 2 && second_code == 0 {
        println!("Proof: ambiguity was routed as data; the resolved value is safe to commit.");
        0
    } else {
        println!("Proof failed: expected clarify, then resolved.");
        1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn status(envelope: &Value) -> Option<&str> {
        match envelope {
            Value::Obj(object) => match object.get("status") {
                Some(Value::Str(status)) => Some(status),
                _ => None,
            },
            _ => None,
        }
    }

    #[test]
    fn ambiguous_output_has_a_distinct_exit_status() {
        let (envelope, code) = metabolize(DEMO_SCHEMA, DEMO_OUTPUT, None);
        assert_eq!(code, 2);
        assert_eq!(status(&envelope), Some("clarify"));
    }

    #[test]
    fn requested_answer_resolves_the_output() {
        let (envelope, code) = metabolize(
            DEMO_SCHEMA,
            DEMO_OUTPUT,
            Some(vec![Answer::new("$.qty", Value::Num(2.0))]),
        );
        assert_eq!(code, 0);
        assert_eq!(status(&envelope), Some("resolved"));
        assert!(parse(&envelope.to_string()).is_ok());
    }

    #[test]
    fn unrelated_answer_is_rejected() {
        let (envelope, code) = metabolize(
            DEMO_SCHEMA,
            DEMO_OUTPUT,
            Some(vec![Answer::new("$.item", Value::Str("tea".into()))]),
        );
        assert_eq!(code, 1);
        assert_eq!(status(&envelope), Some("rejected"));
    }
}
