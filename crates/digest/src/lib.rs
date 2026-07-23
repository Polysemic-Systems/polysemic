//! digest — the metabolism.
//!
//! Sits on the seam where probabilistic output meets deterministic
//! infrastructure. Three commitments, straight from the manifesto:
//!
//! 1. **Repair, don't throw.** Fences, prose, trailing commas, single
//!    quotes, Python literals — fixed, and every fix is *named* in the
//!    [`Repair`] log. Silent repair is just a different kind of lying.
//! 2. **Ambiguity becomes a question, not a coin flip.** `"2 or 3"` in a
//!    quantity field is not an error and not a guess — it is a
//!    [`Question`] escalated to whoever can answer it.
//! 3. **Extra meaning is kept, not refused.** Unknown object keys pass
//!    through untouched. The schema legislates what it names; it does not
//!    outlaw what it doesn't.

use polysemic_core::{parse, ParseError, Value};
use std::collections::BTreeMap;
use std::fmt;

// ---------------------------------------------------------------------------
// The ledger of named acts
// ---------------------------------------------------------------------------

/// Every transformation digest performs is recorded. Nothing is silent.
#[derive(Debug, Clone, PartialEq)]
pub enum Repair {
    /// Removed a markdown code fence (``` … ```).
    StrippedFence,
    /// Extracted the JSON payload from surrounding prose.
    StrippedProse,
    /// Rewrote Python literals: True/False/None → true/false/null.
    PythonLiterals,
    /// Rewrote single-quoted strings as double-quoted.
    RequotedStrings,
    /// Quoted bare object keys.
    QuotedKeys,
    /// Removed trailing commas before `}` / `]`.
    RemovedTrailingCommas,
    /// Coerced a value's type at `path` (e.g. `"42"` → `42`).
    Coerced {
        path: String,
        from: String,
        to: String,
    },
    /// Resolved a hedge (`"~5"`, `"about 5"`) to a number at `path`.
    HedgeResolved { path: String, original: String },
    /// Matched an enum variant case-insensitively at `path`.
    CaseFolded { path: String, original: String },
    /// Dropped an explicit null at `path`, an optional field whose declared
    /// schema cannot hold null. There null and omission assert the same
    /// thing — no value here — so the drop applies the declared schema
    /// semantics instead of asking about a stated absence. Fields typed
    /// [`Schema::Any`] admit null as a value, so null passes through there.
    DroppedNullOptional { path: String },
}

impl fmt::Display for Repair {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Repair::StrippedFence => write!(f, "stripped markdown fence"),
            Repair::StrippedProse => write!(f, "extracted JSON from surrounding prose"),
            Repair::PythonLiterals => write!(f, "rewrote Python literals (True/False/None)"),
            Repair::RequotedStrings => write!(f, "rewrote single-quoted strings"),
            Repair::QuotedKeys => write!(f, "quoted bare object keys"),
            Repair::RemovedTrailingCommas => write!(f, "removed trailing commas"),
            Repair::Coerced { path, from, to } => {
                write!(f, "coerced {path}: {from} → {to}")
            }
            Repair::HedgeResolved { path, original } => {
                write!(f, "resolved hedge at {path}: {original:?}")
            }
            Repair::CaseFolded { path, original } => {
                write!(f, "case-folded {path}: {original:?}")
            }
            Repair::DroppedNullOptional { path } => {
                write!(f, "dropped null at {path}: optional field left unset")
            }
        }
    }
}

impl Repair {
    /// Stable machine-readable name for this repair. Human-readable wording
    /// may evolve; this code is suitable for logs, policies, and metrics.
    pub fn code(&self) -> &'static str {
        match self {
            Repair::StrippedFence => "stripped_fence",
            Repair::StrippedProse => "stripped_prose",
            Repair::PythonLiterals => "python_literals",
            Repair::RequotedStrings => "requoted_strings",
            Repair::QuotedKeys => "quoted_keys",
            Repair::RemovedTrailingCommas => "removed_trailing_commas",
            Repair::Coerced { .. } => "coerced",
            Repair::HedgeResolved { .. } => "hedge_resolved",
            Repair::CaseFolded { .. } => "case_folded",
            Repair::DroppedNullOptional { .. } => "dropped_null_optional",
        }
    }
}

/// An ambiguity the system could not — and should not — resolve alone.
#[derive(Debug, Clone, PartialEq)]
pub struct Question {
    /// JSON path of the ambiguous value, e.g. `$.qty`.
    pub path: String,
    /// A human-answerable prompt.
    pub prompt: String,
    /// Candidate readings, when the polysemy is enumerable.
    pub candidates: Vec<String>,
}

/// A human or policy-layer answer to one explicit [`Question`]. Answers are
/// keyed by the same JSON path carried by the question, so the handoff can be
/// transported without rewriting the model's original output.
#[derive(Debug, Clone, PartialEq)]
pub struct Answer {
    pub path: String,
    pub value: Value,
}

impl Answer {
    pub fn new(path: &str, value: Value) -> Self {
        Self {
            path: path.to_string(),
            value,
        }
    }
}

/// A failed attempt to apply a clarification answer. Digest refuses answers
/// to paths it did not ask about: human-in-the-loop must not become a silent
/// mutation backdoor.
#[derive(Debug, Clone, PartialEq)]
pub enum AnswerError {
    Parse(ParseError),
    NotRequested { path: String },
    Duplicate { path: String },
    InvalidPath { path: String },
}

impl fmt::Display for AnswerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AnswerError::Parse(error) => write!(f, "{error}"),
            AnswerError::NotRequested { path } => {
                write!(f, "no clarification was requested at {path}")
            }
            AnswerError::Duplicate { path } => write!(f, "duplicate answer for {path}"),
            AnswerError::InvalidPath { path } => write!(f, "invalid answer path {path}"),
        }
    }
}

impl std::error::Error for AnswerError {}

impl From<ParseError> for AnswerError {
    fn from(value: ParseError) -> Self {
        AnswerError::Parse(value)
    }
}

impl fmt::Display for Question {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} — {}", self.path, self.prompt)?;
        if !self.candidates.is_empty() {
            write!(f, " [{}]", self.candidates.join(" | "))?;
        }
        Ok(())
    }
}

/// The result of digestion: either a value you can trust, or the questions
/// you must answer first. There is no third state where you got a value you
/// *can't* trust — that state is the one this crate exists to delete.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    Resolved(Value),
    Clarify(Vec<Question>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Digestion {
    pub outcome: Outcome,
    pub repairs: Vec<Repair>,
    /// Human or policy-layer answers applied during this digestion. This is
    /// separate from repairs: an answer is an accountable decision, not a
    /// parser transformation.
    pub answers: Vec<Answer>,
}

impl Digestion {
    pub fn is_resolved(&self) -> bool {
        matches!(self.outcome, Outcome::Resolved(_))
    }
}

// ---------------------------------------------------------------------------
// Schema — the legislated layer
// ---------------------------------------------------------------------------

/// A deliberately small schema language. It legislates structure on top of
/// grown meaning; it does not pretend to exhaust it (see `Schema::Any`, and
/// the fact that unknown keys survive).
#[derive(Debug, Clone)]
pub enum Schema {
    Any,
    Bool,
    Num {
        min: Option<f64>,
        max: Option<f64>,
    },
    Str,
    /// One of a fixed set of strings.
    Choice(Vec<String>),
    Arr(Box<Schema>),
    Obj(Vec<Field>),
}

/// A JSON Schema document asked Digest to enforce something outside its
/// deliberately small, explicit subset. Rejecting unsupported constraints is
/// safer than silently pretending they were applied.
#[derive(Debug, Clone, PartialEq)]
pub struct SchemaError {
    pub path: String,
    pub message: String,
}

impl fmt::Display for SchemaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "schema error at {}: {}", self.path, self.message)
    }
}

impl std::error::Error for SchemaError {}

#[derive(Debug, Clone)]
pub struct Field {
    pub name: String,
    pub schema: Schema,
    pub required: bool,
}

impl Schema {
    pub fn num() -> Schema {
        Schema::Num {
            min: None,
            max: None,
        }
    }
    pub fn num_range(min: f64, max: f64) -> Schema {
        Schema::Num {
            min: Some(min),
            max: Some(max),
        }
    }
    pub fn choice<S: Into<String>>(opts: impl IntoIterator<Item = S>) -> Schema {
        Schema::Choice(opts.into_iter().map(Into::into).collect())
    }
    pub fn obj(fields: impl IntoIterator<Item = Field>) -> Schema {
        Schema::Obj(fields.into_iter().collect())
    }

    /// Parse the JSON Schema subset supported by the Digest POC.
    ///
    /// Supported forms are `string`, `number` with `minimum`/`maximum`,
    /// `boolean`, arrays with `items`, objects with `properties`/`required`,
    /// and string `enum`s. `{}` means [`Schema::Any`]. Unsupported keywords
    /// are rejected so callers never receive a false validation guarantee.
    pub fn from_json_schema(raw: &str) -> Result<Schema, SchemaError> {
        let value = parse(raw).map_err(|error| SchemaError {
            path: "$".into(),
            message: error.to_string(),
        })?;
        schema_from_value(&value, "$schema")
    }
}

impl Field {
    pub fn req(name: &str, schema: Schema) -> Field {
        Field {
            name: name.to_string(),
            schema,
            required: true,
        }
    }
    pub fn opt(name: &str, schema: Schema) -> Field {
        Field {
            name: name.to_string(),
            schema,
            required: false,
        }
    }
}

fn schema_from_value(value: &Value, path: &str) -> Result<Schema, SchemaError> {
    let Value::Obj(object) = value else {
        return Err(schema_error(path, "expected a JSON object"));
    };

    let constraint_keys: Vec<&str> = object
        .keys()
        .map(String::as_str)
        .filter(|key| !matches!(*key, "$schema" | "$id" | "title" | "description"))
        .collect();
    if constraint_keys.is_empty() {
        return Ok(Schema::Any);
    }

    if let Some(choices) = object.get("enum") {
        reject_unknown_schema_keys(object, &["enum", "type"], path)?;
        match object.get("type") {
            Some(Value::Str(schema_type)) if schema_type == "string" => {}
            Some(Value::Str(schema_type)) => {
                return Err(schema_error(
                    &format!("{path}.type"),
                    &format!("string enum cannot declare type {schema_type:?}"),
                ))
            }
            Some(_) => return Err(schema_error(&format!("{path}.type"), "expected a string")),
            None => {}
        }
        let Value::Arr(choices) = choices else {
            return Err(schema_error(&format!("{path}.enum"), "expected an array"));
        };
        if choices.is_empty() {
            return Err(schema_error(
                &format!("{path}.enum"),
                "expected at least one choice",
            ));
        }
        let mut strings = Vec::with_capacity(choices.len());
        for (index, choice) in choices.iter().enumerate() {
            let Value::Str(choice) = choice else {
                return Err(schema_error(
                    &format!("{path}.enum[{index}]"),
                    "Digest currently supports string enum values only",
                ));
            };
            strings.push(choice.clone());
        }
        return Ok(Schema::Choice(strings));
    }

    let schema_type = match object.get("type") {
        Some(Value::Str(schema_type)) => schema_type.as_str(),
        Some(_) => return Err(schema_error(&format!("{path}.type"), "expected a string")),
        None => return Err(schema_error(path, "missing `type` or `enum`")),
    };

    match schema_type {
        "string" => {
            reject_unknown_schema_keys(object, &["type"], path)?;
            Ok(Schema::Str)
        }
        "boolean" => {
            reject_unknown_schema_keys(object, &["type"], path)?;
            Ok(Schema::Bool)
        }
        "number" => {
            reject_unknown_schema_keys(object, &["type", "minimum", "maximum"], path)?;
            let min = optional_finite_number(object.get("minimum"), &format!("{path}.minimum"))?;
            let max = optional_finite_number(object.get("maximum"), &format!("{path}.maximum"))?;
            if min.zip(max).is_some_and(|(min, max)| min > max) {
                return Err(schema_error(path, "`minimum` cannot exceed `maximum`"));
            }
            Ok(Schema::Num { min, max })
        }
        "array" => {
            reject_unknown_schema_keys(object, &["type", "items"], path)?;
            let items = object
                .get("items")
                .ok_or_else(|| schema_error(path, "array schema is missing `items`"))?;
            Ok(Schema::Arr(Box::new(schema_from_value(
                items,
                &format!("{path}.items"),
            )?)))
        }
        "object" => {
            reject_unknown_schema_keys(object, &["type", "properties", "required"], path)?;
            let properties = match object.get("properties") {
                Some(Value::Obj(properties)) => properties,
                Some(_) => {
                    return Err(schema_error(
                        &format!("{path}.properties"),
                        "expected an object",
                    ))
                }
                None => return Err(schema_error(path, "object schema is missing `properties`")),
            };
            let required = parse_required(object.get("required"), path)?;
            for name in &required {
                if !properties.contains_key(name) {
                    return Err(schema_error(
                        &format!("{path}.required"),
                        &format!("{name:?} is not declared in `properties`"),
                    ));
                }
            }
            let mut fields = Vec::with_capacity(properties.len());
            for (name, property) in properties {
                if name.is_empty() || name.contains('.') || name.contains('[') {
                    return Err(schema_error(
                        &format!("{path}.properties"),
                        &format!(
                            "property name {name:?} cannot be represented by the POC answer-path syntax"
                        ),
                    ));
                }
                fields.push(Field {
                    name: name.clone(),
                    schema: schema_from_value(property, &format!("{path}.properties.{name}"))?,
                    required: required.contains(name),
                });
            }
            Ok(Schema::Obj(fields))
        }
        other => Err(schema_error(
            &format!("{path}.type"),
            &format!("unsupported type {other:?}"),
        )),
    }
}

fn reject_unknown_schema_keys(
    object: &BTreeMap<String, Value>,
    allowed: &[&str],
    path: &str,
) -> Result<(), SchemaError> {
    for key in object.keys() {
        let metadata = matches!(key.as_str(), "$schema" | "$id" | "title" | "description");
        if !metadata && !allowed.contains(&key.as_str()) {
            return Err(schema_error(
                &format!("{path}.{key}"),
                "unsupported keyword; Digest refuses to ignore constraints silently",
            ));
        }
    }
    Ok(())
}

fn optional_finite_number(value: Option<&Value>, path: &str) -> Result<Option<f64>, SchemaError> {
    match value {
        None => Ok(None),
        Some(Value::Num(number)) if number.is_finite() => Ok(Some(*number)),
        Some(Value::Num(_)) => Err(schema_error(path, "expected a finite number")),
        Some(_) => Err(schema_error(path, "expected a number")),
    }
}

fn parse_required(
    value: Option<&Value>,
    path: &str,
) -> Result<std::collections::BTreeSet<String>, SchemaError> {
    let mut required = std::collections::BTreeSet::new();
    let Some(value) = value else {
        return Ok(required);
    };
    let Value::Arr(items) = value else {
        return Err(schema_error(
            &format!("{path}.required"),
            "expected an array",
        ));
    };
    for (index, item) in items.iter().enumerate() {
        let Value::Str(name) = item else {
            return Err(schema_error(
                &format!("{path}.required[{index}]"),
                "expected a property name",
            ));
        };
        if !required.insert(name.clone()) {
            return Err(schema_error(
                &format!("{path}.required[{index}]"),
                &format!("duplicate property {name:?}"),
            ));
        }
    }
    Ok(required)
}

fn schema_error(path: &str, message: &str) -> SchemaError {
    SchemaError {
        path: path.to_string(),
        message: message.to_string(),
    }
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

/// Digest raw model output against a schema.
///
/// Never panics on ambiguity; never silently guesses. Returns a parse error
/// only when the text is unrecoverable even after every named repair pass.
pub fn digest(raw: &str, schema: &Schema) -> Result<Digestion, ParseError> {
    let mut repairs = Vec::new();
    match parse_leniently(raw, &mut repairs)? {
        Lenient::One(value) => Ok(check_value(value, schema, repairs, Vec::new())),
        Lenient::Many(documents) => Ok(Digestion {
            outcome: Outcome::Clarify(vec![multi_document_question(&documents)]),
            repairs,
            answers: Vec::new(),
        }),
    }
}

/// Apply answers only to ambiguities Digest actually raised, then validate
/// the result again. Missing answers remain questions; unrelated paths cannot
/// be overwritten through this API.
pub fn digest_with_answers(
    raw: &str,
    schema: &Schema,
    answers: impl IntoIterator<Item = Answer>,
) -> Result<Digestion, AnswerError> {
    let mut repairs = Vec::new();
    let mut value = match parse_leniently(raw, &mut repairs)? {
        Lenient::One(value) => value,
        Lenient::Many(documents) => {
            // The only question the boundary raised is "which document?";
            // an answer at `$` is the human choosing one. Any other path
            // was never requested.
            let mut chosen = None;
            let mut applied = Vec::new();
            for answer in answers {
                if answer.path != "$" {
                    return Err(AnswerError::NotRequested { path: answer.path });
                }
                if chosen.is_some() {
                    return Err(AnswerError::Duplicate { path: answer.path });
                }
                chosen = Some(answer.value.clone());
                applied.push(answer);
            }
            return Ok(match chosen {
                Some(value) => check_value(value, schema, repairs, applied),
                None => Digestion {
                    outcome: Outcome::Clarify(vec![multi_document_question(&documents)]),
                    repairs,
                    answers: applied,
                },
            });
        }
    };

    let mut probe_repairs = repairs.clone();
    let mut questions = Vec::new();
    let _ = check(
        value.clone(),
        schema,
        "$",
        &mut probe_repairs,
        &mut questions,
    );
    let requested: std::collections::BTreeSet<String> = questions
        .into_iter()
        .map(|question| question.path)
        .collect();

    let mut applied = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for answer in answers {
        if !requested.contains(&answer.path) {
            return Err(AnswerError::NotRequested { path: answer.path });
        }
        if !seen.insert(answer.path.clone()) {
            return Err(AnswerError::Duplicate { path: answer.path });
        }
        set_json_path(&mut value, &answer.path, answer.value.clone())?;
        applied.push(answer);
    }

    Ok(check_value(value, schema, repairs, applied))
}

fn check_value(
    value: Value,
    schema: &Schema,
    mut repairs: Vec<Repair>,
    answers: Vec<Answer>,
) -> Digestion {
    let mut questions = Vec::new();
    let checked = check(value, schema, "$", &mut repairs, &mut questions);

    let outcome = if questions.is_empty() {
        Outcome::Resolved(checked)
    } else {
        Outcome::Clarify(questions)
    };
    Digestion {
        outcome,
        repairs,
        answers,
    }
}

fn set_json_path(root: &mut Value, path: &str, replacement: Value) -> Result<(), AnswerError> {
    if path == "$" {
        *root = replacement;
        return Ok(());
    }
    let segments = parse_json_path(path).ok_or_else(|| AnswerError::InvalidPath {
        path: path.to_string(),
    })?;
    let mut current = root;
    for segment in &segments[..segments.len().saturating_sub(1)] {
        current = descend(current, segment).ok_or_else(|| AnswerError::InvalidPath {
            path: path.to_string(),
        })?;
    }
    match (segments.last(), current) {
        (Some(PathSegment::Key(key)), Value::Obj(object)) => {
            object.insert(key.clone(), replacement);
            Ok(())
        }
        (Some(PathSegment::Index(index)), Value::Arr(array)) if *index < array.len() => {
            array[*index] = replacement;
            Ok(())
        }
        _ => Err(AnswerError::InvalidPath {
            path: path.to_string(),
        }),
    }
}

#[derive(Debug)]
enum PathSegment {
    Key(String),
    Index(usize),
}

fn parse_json_path(path: &str) -> Option<Vec<PathSegment>> {
    let bytes = path.as_bytes();
    if !path.starts_with('$') || path.len() == 1 {
        return None;
    }
    let mut segments = Vec::new();
    let mut i = 1;
    while i < bytes.len() {
        match bytes[i] {
            b'.' => {
                i += 1;
                let start = i;
                while i < bytes.len() && bytes[i] != b'.' && bytes[i] != b'[' {
                    i += 1;
                }
                if start == i {
                    return None;
                }
                segments.push(PathSegment::Key(path[start..i].to_string()));
            }
            b'[' => {
                i += 1;
                let start = i;
                while i < bytes.len() && bytes[i].is_ascii_digit() {
                    i += 1;
                }
                if start == i || bytes.get(i) != Some(&b']') {
                    return None;
                }
                let index = path[start..i].parse().ok()?;
                segments.push(PathSegment::Index(index));
                i += 1;
            }
            _ => return None,
        }
    }
    (!segments.is_empty()).then_some(segments)
}

fn descend<'a>(value: &'a mut Value, segment: &PathSegment) -> Option<&'a mut Value> {
    match (segment, value) {
        (PathSegment::Key(key), Value::Obj(object)) => object.get_mut(key),
        (PathSegment::Index(index), Value::Arr(array)) => array.get_mut(*index),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Repair pipeline — parse leniently, log every act
// ---------------------------------------------------------------------------

/// The lenient parse either yields one value or refuses to choose between
/// several complete documents: dropping all but the first would be silent
/// loss wearing a `stripped_prose` label.
enum Lenient {
    One(Value),
    Many(Vec<String>),
}

fn multi_document_question(documents: &[String]) -> Question {
    Question {
        path: "$".to_string(),
        prompt: format!(
            "the input contains {} complete JSON documents — which one did they mean?",
            documents.len()
        ),
        candidates: documents.to_vec(),
    }
}

/// Find every complete balanced JSON span in `text`, in order. Only spans
/// that survive the repair pipeline on their own count as documents, so
/// prose braces ("{see above}") do not trigger false ambiguity.
fn balanced_documents(text: &str) -> Vec<String> {
    let mut documents = Vec::new();
    let mut from = 0;
    while let Some(open_rel) = text[from..].find(['{', '[']) {
        let open = from + open_rel;
        let bytes = text.as_bytes();
        let mut scan = Scan {
            b: bytes,
            i: open,
            in_str: false,
        };
        let mut depth = 0usize;
        let mut end = None;
        while let Some(c) = scan.step() {
            if scan.in_str {
                continue;
            }
            match c {
                b'{' | b'[' => depth += 1,
                b'}' | b']' => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        end = Some(scan.i);
                        break;
                    }
                }
                _ => {}
            }
        }
        let Some(end) = end else { break };
        let span = text[open..end].to_string();
        if repair_pipeline(&span, &mut Vec::new()).is_ok() {
            documents.push(span);
        }
        from = end;
    }
    documents
}

fn parse_leniently(raw: &str, repairs: &mut Vec<Repair>) -> Result<Lenient, ParseError> {
    if let Ok(v) = parse(raw) {
        return Ok(Lenient::One(v));
    }

    // A fenced block is one declared document; strip the fence before asking
    // whether the remainder holds several.
    let mut text = raw.to_string();
    if let Some(stripped) = strip_fences(&text) {
        if stripped != text {
            text = stripped;
            repairs.push(Repair::StrippedFence);
            if let Ok(v) = parse(&text) {
                return Ok(Lenient::One(v));
            }
        }
    }

    let documents = balanced_documents(&text);
    if documents.len() > 1 {
        return Ok(Lenient::Many(documents));
    }

    repair_pipeline(&text, repairs).map(Lenient::One)
}

fn repair_pipeline(input: &str, repairs: &mut Vec<Repair>) -> Result<Value, ParseError> {
    if let Ok(v) = parse(input) {
        return Ok(v);
    }

    let mut text = input.to_string();
    type RepairPass = fn(&str) -> Option<String>;
    // Requoting runs before the Python-literal pass so that words like
    // `True` inside single-quoted user strings become protected string
    // content instead of being rewritten — repairing quotes must never
    // corrupt the text the quotes carry.
    let passes: [(RepairPass, Repair); 5] = [
        (strip_fences, Repair::StrippedFence),
        (extract_json, Repair::StrippedProse),
        (requote_strings, Repair::RequotedStrings),
        (python_literals, Repair::PythonLiterals),
        (quote_bare_keys, Repair::QuotedKeys),
    ];

    let mut last_err = parse(&text).unwrap_err();
    for (pass, tag) in passes {
        if let Some(changed) = pass(&text) {
            if changed != text {
                text = changed;
                repairs.push(tag.clone());
                match parse(&text) {
                    Ok(v) => return Ok(v),
                    Err(e) => last_err = e,
                }
            }
        }
    }
    // trailing commas last: cheap, common, and safe once quoting is sane
    if let Some(changed) = remove_trailing_commas(&text) {
        if changed != text {
            text = changed;
            repairs.push(Repair::RemovedTrailingCommas);
            match parse(&text) {
                Ok(v) => return Ok(v),
                Err(e) => last_err = e,
            }
        }
    }
    Err(last_err)
}

/// Walk bytes tracking whether we're inside a double-quoted string.
/// Every structural pass below uses this to avoid touching string contents.
struct Scan<'a> {
    b: &'a [u8],
    i: usize,
    in_str: bool,
}

impl<'a> Scan<'a> {
    /// Advance one byte, updating string state. Returns the byte consumed.
    fn step(&mut self) -> Option<u8> {
        let c = *self.b.get(self.i)?;
        if self.in_str {
            if c == b'\\' {
                self.i += 2; // skip escaped char
                return Some(c);
            }
            if c == b'"' {
                self.in_str = false;
            }
        } else if c == b'"' {
            self.in_str = true;
        }
        self.i += 1;
        Some(c)
    }
}

fn strip_fences(s: &str) -> Option<String> {
    let start = s.find("```")?;
    let after = &s[start + 3..];
    // optional language tag up to end of line
    let body_start = after.find('\n').map(|n| n + 1).unwrap_or(0);
    let body = &after[body_start..];
    let end = body.find("```").unwrap_or(body.len());
    Some(body[..end].trim().to_string())
}

fn extract_json(s: &str) -> Option<String> {
    let open = s.find(['{', '['])?;
    let bytes = s.as_bytes();
    let mut scan = Scan {
        b: bytes,
        i: open,
        in_str: false,
    };
    let mut depth = 0usize;
    let mut end = None;
    while let Some(c) = scan.step() {
        if scan.in_str {
            continue;
        }
        match c {
            b'{' | b'[' => depth += 1,
            b'}' | b']' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    end = Some(scan.i);
                    break;
                }
            }
            _ => {}
        }
    }
    let end = end?;
    if open == 0 && end == s.trim_end().len() {
        return None; // nothing to strip
    }
    Some(s[open..end].to_string())
}

fn python_literals(s: &str) -> Option<String> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    let mut in_str = false;
    while i < b.len() {
        let c = b[i];
        if in_str {
            out.push(c);
            if c == b'\\' && i + 1 < b.len() {
                out.push(b[i + 1]);
                i += 2;
                continue;
            }
            if c == b'"' {
                in_str = false;
            }
            i += 1;
            continue;
        }
        if c == b'"' {
            in_str = true;
            out.push(c);
            i += 1;
            continue;
        }
        let mut matched = false;
        for (find, put) in [("True", "true"), ("False", "false"), ("None", "null")] {
            if b[i..].starts_with(find.as_bytes()) && word_boundary(b, i, find.len()) {
                out.extend_from_slice(put.as_bytes());
                i += find.len();
                matched = true;
                break;
            }
        }
        if !matched {
            out.push(c);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

fn word_boundary(b: &[u8], at: usize, len: usize) -> bool {
    let before_ok = at == 0 || !is_word(b[at - 1]);
    let after_ok = at + len >= b.len() || !is_word(b[at + len]);
    before_ok && after_ok
}

fn is_word(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_'
}

fn requote_strings(s: &str) -> Option<String> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    let mut in_dq = false;
    while i < b.len() {
        let c = b[i];
        if in_dq {
            out.push(c);
            if c == b'\\' && i + 1 < b.len() {
                out.push(b[i + 1]);
                i += 2;
                continue;
            }
            if c == b'"' {
                in_dq = false;
            }
            i += 1;
        } else if c == b'"' {
            in_dq = true;
            out.push(c);
            i += 1;
        } else if c == b'\'' {
            // single-quoted string: convert to double-quoted
            out.push(b'"');
            i += 1;
            while i < b.len() {
                match b[i] {
                    b'\\' if i + 1 < b.len() && b[i + 1] == b'\'' => {
                        out.push(b'\'');
                        i += 2;
                    }
                    b'\'' => {
                        i += 1;
                        break;
                    }
                    b'"' => {
                        out.extend_from_slice(b"\\\"");
                        i += 1;
                    }
                    other => {
                        out.push(other);
                        i += 1;
                    }
                }
            }
            out.push(b'"');
        } else {
            out.push(c);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

fn quote_bare_keys(s: &str) -> Option<String> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    let mut in_str = false;
    while i < b.len() {
        let c = b[i];
        if in_str {
            out.push(c);
            if c == b'\\' && i + 1 < b.len() {
                out.push(b[i + 1]);
                i += 2;
                continue;
            }
            if c == b'"' {
                in_str = false;
            }
            i += 1;
            continue;
        }
        if c == b'"' {
            in_str = true;
            out.push(c);
            i += 1;
            continue;
        }
        out.push(c);
        i += 1;
        if c == b'{' || c == b',' {
            // lookahead: ws, identifier, ws, ':'
            let mut j = i;
            while j < b.len() && (b[j] as char).is_whitespace() {
                j += 1;
            }
            let ident_start = j;
            while j < b.len() && is_word(b[j]) {
                j += 1;
            }
            if j > ident_start {
                let mut k = j;
                while k < b.len() && (b[k] as char).is_whitespace() {
                    k += 1;
                }
                if k < b.len() && b[k] == b':' {
                    // emit ws, quoted ident; resume at j
                    out.extend_from_slice(&b[i..ident_start]);
                    out.push(b'"');
                    out.extend_from_slice(&b[ident_start..j]);
                    out.push(b'"');
                    i = j;
                }
            }
        }
    }
    String::from_utf8(out).ok()
}

fn remove_trailing_commas(s: &str) -> Option<String> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    let mut in_str = false;
    while i < b.len() {
        let c = b[i];
        if in_str {
            out.push(c);
            if c == b'\\' && i + 1 < b.len() {
                out.push(b[i + 1]);
                i += 2;
                continue;
            }
            if c == b'"' {
                in_str = false;
            }
            i += 1;
            continue;
        }
        if c == b'"' {
            in_str = true;
            out.push(c);
            i += 1;
            continue;
        }
        if c == b',' {
            let mut j = i + 1;
            while j < b.len() && (b[j] as char).is_whitespace() {
                j += 1;
            }
            if j < b.len() && (b[j] == b'}' || b[j] == b']') {
                i += 1; // drop the comma
                continue;
            }
        }
        out.push(c);
        i += 1;
    }
    String::from_utf8(out).ok()
}

// ---------------------------------------------------------------------------
// Schema check — coerce where honest, ask where not
// ---------------------------------------------------------------------------

fn check(
    value: Value,
    schema: &Schema,
    path: &str,
    repairs: &mut Vec<Repair>,
    questions: &mut Vec<Question>,
) -> Value {
    match (schema, value) {
        (Schema::Any, v) => v,

        (Schema::Bool, Value::Bool(b)) => Value::Bool(b),
        (Schema::Bool, Value::Str(s)) => match s.trim().to_ascii_lowercase().as_str() {
            "true" | "yes" | "y" | "1" => {
                repairs.push(Repair::Coerced {
                    path: path.to_string(),
                    from: format!("string {s:?}"),
                    to: "bool".into(),
                });
                Value::Bool(true)
            }
            "false" | "no" | "n" | "0" => {
                repairs.push(Repair::Coerced {
                    path: path.to_string(),
                    from: format!("string {s:?}"),
                    to: "bool".into(),
                });
                Value::Bool(false)
            }
            _ => {
                questions.push(Question {
                    path: path.to_string(),
                    prompt: format!("expected true/false, got {s:?}"),
                    candidates: vec!["true".into(), "false".into()],
                });
                Value::Null
            }
        },

        (Schema::Num { min, max }, Value::Num(n)) => {
            range_check(n, *min, *max, path, questions);
            Value::Num(n)
        }
        (Schema::Num { min, max }, Value::Str(s)) => {
            digest_numeric_string(&s, *min, *max, path, repairs, questions)
        }

        (Schema::Str, Value::Str(s)) => Value::Str(s),
        (Schema::Str, Value::Num(n)) => {
            repairs.push(Repair::Coerced {
                path: path.to_string(),
                from: "number".into(),
                to: "string".into(),
            });
            Value::Str(Value::Num(n).to_string())
        }
        (Schema::Str, Value::Bool(b)) => {
            repairs.push(Repair::Coerced {
                path: path.to_string(),
                from: "bool".into(),
                to: "string".into(),
            });
            Value::Str(b.to_string())
        }

        (Schema::Choice(opts), Value::Str(s)) => {
            if opts.iter().any(|o| o == &s) {
                return Value::Str(s);
            }
            let folded = s.trim().to_ascii_lowercase();
            let hits: Vec<&String> = opts
                .iter()
                .filter(|o| o.to_ascii_lowercase() == folded)
                .collect();
            match hits.as_slice() {
                // exactly one option matches ignoring case: an honest repair
                [hit] => {
                    repairs.push(Repair::CaseFolded {
                        path: path.to_string(),
                        original: s.clone(),
                    });
                    return Value::Str((*hit).clone());
                }
                // several options differ only by case: choosing the first
                // declared one would be a coin flip wearing a repair label
                [_, _, ..] => {
                    questions.push(Question {
                        path: path.to_string(),
                        prompt: format!("{s:?} matches more than one option ignoring case"),
                        candidates: hits.into_iter().cloned().collect(),
                    });
                    return Value::Null;
                }
                [] => {}
            }
            questions.push(Question {
                path: path.to_string(),
                prompt: format!("{s:?} is not a known option"),
                candidates: opts.clone(),
            });
            Value::Null
        }

        (Schema::Arr(inner), Value::Arr(items)) => Value::Arr(
            items
                .into_iter()
                .enumerate()
                .map(|(i, v)| check(v, inner, &format!("{path}[{i}]"), repairs, questions))
                .collect(),
        ),

        (Schema::Obj(fields), Value::Obj(mut map)) => {
            let mut out = BTreeMap::new();
            for field in fields {
                let fpath = format!("{path}.{}", field.name);
                match map.remove(&field.name) {
                    // When the declared schema cannot hold null, an explicit
                    // null on an optional field says what omitting the key
                    // says. Drop it — named, not silent — rather than asking
                    // the human to disambiguate a stated absence. Schema::Any
                    // admits null as a value, so it takes the pass-through arm.
                    Some(Value::Null)
                        if !field.required && !matches!(field.schema, Schema::Any) =>
                    {
                        repairs.push(Repair::DroppedNullOptional { path: fpath });
                    }
                    Some(v) => {
                        out.insert(
                            field.name.clone(),
                            check(v, &field.schema, &fpath, repairs, questions),
                        );
                    }
                    None if field.required => {
                        questions.push(Question {
                            path: fpath.clone(),
                            prompt: "required field is missing".into(),
                            candidates: vec![],
                        });
                    }
                    None => {}
                }
            }
            // Polysemy clause: unknown keys are extra meaning, not error.
            // They pass through untouched.
            for (k, v) in map {
                out.insert(k, v);
            }
            Value::Obj(out)
        }

        // genuine type mismatch with no honest coercion → ask
        (expected, got) => {
            questions.push(Question {
                path: path.to_string(),
                prompt: format!(
                    "expected {}, got {} ({got})",
                    schema_name(expected),
                    got.type_name()
                ),
                candidates: vec![],
            });
            Value::Null
        }
    }
}

fn schema_name(s: &Schema) -> &'static str {
    match s {
        Schema::Any => "any",
        Schema::Bool => "bool",
        Schema::Num { .. } => "number",
        Schema::Str => "string",
        Schema::Choice(_) => "choice",
        Schema::Arr(_) => "array",
        Schema::Obj(_) => "object",
    }
}

fn range_check(
    n: f64,
    min: Option<f64>,
    max: Option<f64>,
    path: &str,
    questions: &mut Vec<Question>,
) {
    let low = min.is_some_and(|m| n < m);
    let high = max.is_some_and(|m| n > m);
    if low || high {
        questions.push(Question {
            path: path.to_string(),
            prompt: format!(
                "{n} is outside the allowed range {}..={}",
                min.map(|m| m.to_string()).unwrap_or_else(|| "-∞".into()),
                max.map(|m| m.to_string()).unwrap_or_else(|| "∞".into()),
            ),
            candidates: vec![],
        });
    }
}

/// The signature move. A string arrives where a number was expected.
/// Classical code throws. A guesser flips a coin. Digest reads the string
/// for what it actually holds — one meaning, a hedge, or several — and
/// responds in kind: coerce, resolve, or ask.
fn digest_numeric_string(
    s: &str,
    min: Option<f64>,
    max: Option<f64>,
    path: &str,
    repairs: &mut Vec<Repair>,
    questions: &mut Vec<Question>,
) -> Value {
    let t = s.trim();

    // univocal: "42"
    if let Some(n) = parse_finite_number(t) {
        repairs.push(Repair::Coerced {
            path: path.to_string(),
            from: format!("string {s:?}"),
            to: "number".into(),
        });
        range_check(n, min, max, path, questions);
        return Value::Num(n);
    }

    // hedged but resolvable: "~5", "about 5", "around 5", "approx 5"
    let lower = t.to_ascii_lowercase();
    for prefix in [
        "~",
        "about ",
        "around ",
        "approx ",
        "approximately ",
        "roughly ",
    ] {
        if let Some(rest) = lower.strip_prefix(prefix) {
            if let Some(n) = parse_finite_number(rest.trim()) {
                repairs.push(Repair::HedgeResolved {
                    path: path.to_string(),
                    original: s.to_string(),
                });
                range_check(n, min, max, path, questions);
                return Value::Num(n);
            }
        }
    }

    // enumerable polysemy: "2 or 3" → ask, with candidates
    if let Some((a, b)) = lower.split_once(" or ") {
        if let (Some(x), Some(y)) = (parse_finite_number(a.trim()), parse_finite_number(b.trim())) {
            questions.push(Question {
                path: path.to_string(),
                prompt: format!("{s:?} holds two readings — which did they mean?"),
                candidates: vec![fmt_num(x), fmt_num(y)],
            });
            return Value::Null;
        }
    }

    // range polysemy: "2-3", "2–3"
    for dash in ['-', '–'] {
        if let Some((a, b)) = t.split_once(dash) {
            if let (Some(x), Some(y)) =
                (parse_finite_number(a.trim()), parse_finite_number(b.trim()))
            {
                questions.push(Question {
                    path: path.to_string(),
                    prompt: format!("{s:?} is a range — which value should apply?"),
                    candidates: vec![fmt_num(x), fmt_num(y)],
                });
                return Value::Null;
            }
        }
    }

    // vague quantifier: not resolvable, not enumerable — ask openly
    if ["a few", "several", "some", "a couple"]
        .iter()
        .any(|v| lower.contains(v))
    {
        questions.push(Question {
            path: path.to_string(),
            prompt: format!("{s:?} is a vague quantity — what number should it be?"),
            candidates: vec![],
        });
        return Value::Null;
    }

    questions.push(Question {
        path: path.to_string(),
        prompt: format!("expected a number, got {s:?}"),
        candidates: vec![],
    });
    Value::Null
}

fn parse_finite_number(input: &str) -> Option<f64> {
    input
        .parse::<f64>()
        .ok()
        .filter(|number| number.is_finite())
}

fn fmt_num(n: f64) -> String {
    Value::Num(n).to_string()
}

// ---------------------------------------------------------------------------
// Reconciliation — sample several times, commit once
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub struct Reconciliation {
    pub outcome: Outcome,
    pub per_sample: Vec<Digestion>,
}

/// Digest several samples of the same request and reconcile them.
/// Agreement is committed; disagreement becomes a question with the
/// disagreeing readings as candidates. The model's variance is treated as
/// polysemy, not noise.
pub fn reconcile(samples: &[&str], schema: &Schema) -> Result<Reconciliation, ParseError> {
    if samples.is_empty() {
        return Err(ParseError {
            at: 0,
            msg: "reconciliation requires at least one sample".into(),
        });
    }
    let mut digestions = Vec::new();
    for s in samples {
        digestions.push(digest(s, schema)?);
    }

    // A sample that still needs clarification is evidence, not an abstention.
    // Never let resolved samples silently outvote unresolved meaning.
    let mut unresolved: Vec<Question> = Vec::new();
    for digestion in &digestions {
        if let Outcome::Clarify(questions) = &digestion.outcome {
            for question in questions {
                merge_question(&mut unresolved, question);
            }
        }
    }
    if !unresolved.is_empty() {
        return Ok(Reconciliation {
            outcome: Outcome::Clarify(unresolved),
            per_sample: digestions,
        });
    }

    let resolved: Vec<&Value> = digestions
        .iter()
        .filter_map(|d| match &d.outcome {
            Outcome::Resolved(v) => Some(v),
            _ => None,
        })
        .collect();

    // unanimous?
    if resolved.windows(2).all(|w| w[0] == w[1]) {
        let v = resolved[0].clone();
        return Ok(Reconciliation {
            outcome: Outcome::Resolved(v),
            per_sample: digestions,
        });
    }

    // field-wise majority for objects; otherwise whole-value majority
    let outcome = if resolved.iter().all(|v| v.as_obj().is_some()) {
        reconcile_objects(&resolved)
    } else {
        majority(&resolved)
    };
    Ok(Reconciliation {
        outcome,
        per_sample: digestions,
    })
}

fn merge_question(questions: &mut Vec<Question>, incoming: &Question) {
    if let Some(existing) = questions
        .iter_mut()
        .find(|existing| existing.path == incoming.path)
    {
        for candidate in &incoming.candidates {
            if !existing.candidates.contains(candidate) {
                existing.candidates.push(candidate.clone());
            }
        }
    } else {
        questions.push(incoming.clone());
    }
}

fn majority(resolved: &[&Value]) -> Outcome {
    let mut best: Option<(&Value, usize)> = None;
    for v in resolved {
        let count = resolved.iter().filter(|o| **o == *v).count();
        if best.is_none_or(|(_, c)| count > c) {
            best = Some((v, count));
        }
    }
    match best {
        Some((v, c)) if c * 2 > resolved.len() => Outcome::Resolved((*v).clone()),
        _ => Outcome::Clarify(vec![Question {
            path: "$".into(),
            prompt: "samples disagree with no majority".into(),
            candidates: resolved.iter().map(|v| v.to_string()).collect(),
        }]),
    }
}

fn reconcile_objects(resolved: &[&Value]) -> Outcome {
    let maps: Vec<&BTreeMap<String, Value>> =
        resolved.iter().map(|v| v.as_obj().unwrap()).collect();
    let mut keys: Vec<&String> = maps.iter().flat_map(|m| m.keys()).collect();
    keys.sort();
    keys.dedup();

    let mut out = BTreeMap::new();
    let mut questions = Vec::new();
    let n = maps.len();

    for key in keys {
        let mut counts: Vec<(&Value, usize)> = Vec::new();
        for m in &maps {
            if let Some(v) = m.get(key) {
                match counts.iter_mut().find(|(cv, _)| *cv == v) {
                    Some((_, c)) => *c += 1,
                    None => counts.push((v, 1)),
                }
            }
        }
        counts.sort_by(|a, b| b.1.cmp(&a.1));
        match counts.first() {
            Some((v, c)) if *c * 2 > n => {
                out.insert(key.clone(), (*v).clone());
            }
            _ => questions.push(Question {
                path: format!("$.{key}"),
                prompt: "samples disagree on this field".into(),
                candidates: counts.iter().map(|(v, _)| v.to_string()).collect(),
            }),
        }
    }

    if questions.is_empty() {
        Outcome::Resolved(Value::Obj(out))
    } else {
        Outcome::Clarify(questions)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn order_schema() -> Schema {
        Schema::obj([
            Field::req("item", Schema::Str),
            Field::req("qty", Schema::num_range(1.0, 99.0)),
            Field::opt("gift_wrap", Schema::Bool),
        ])
    }

    #[test]
    fn repairing_quotes_never_corrupts_the_text_the_quotes_carry() {
        // Regression: `python_literals` used to run before requoting and
        // rewrote `True` inside a single-quoted user string.
        let raw = "{'item': 'True love espresso', 'qty': 2}";
        let d = digest(raw, &order_schema()).unwrap();
        let Outcome::Resolved(value) = d.outcome else {
            panic!("expected resolved");
        };
        let obj = value.as_obj().unwrap();
        assert_eq!(obj["item"], Value::Str("True love espresso".into()));
    }

    #[test]
    fn a_second_document_is_a_question_not_silent_loss() {
        let raw = r#"{"item":"espresso","qty":2} {"item":"tea","qty":9}"#;
        let d = digest(raw, &order_schema()).unwrap();
        let Outcome::Clarify(questions) = d.outcome else {
            panic!("choosing one document silently drops the other");
        };
        assert_eq!(questions.len(), 1);
        assert_eq!(questions[0].path, "$");
        assert_eq!(questions[0].candidates.len(), 2);
        assert!(questions[0].candidates[1].contains("tea"));

        // The human's answer at `$` selects a document and resolves.
        let chosen = parse(&questions[0].candidates[1]).unwrap();
        let d = digest_with_answers(raw, &order_schema(), [Answer::new("$", chosen)]).unwrap();
        let Outcome::Resolved(value) = d.outcome else {
            panic!("expected resolved after choosing");
        };
        assert_eq!(
            value.as_obj().unwrap()["item"],
            Value::Str("tea".to_string())
        );
        assert_eq!(d.answers.len(), 1);
    }

    #[test]
    fn prose_braces_do_not_trigger_false_document_ambiguity() {
        let raw = "Here you go: {\"item\": \"espresso\", \"qty\": 2} {enjoy your day}";
        let d = digest(raw, &order_schema()).unwrap();
        assert!(d.is_resolved(), "outcome: {:?}", d.outcome);
    }

    #[test]
    fn case_folding_between_two_options_is_a_question_not_a_coin_flip() {
        let schema = Schema::obj([Field::req(
            "mode",
            Schema::choice(["Read", "read", "write"]),
        )]);
        // Unique case-insensitive match still folds, with a repair.
        let d = digest(r#"{"mode": "WRITE"}"#, &schema).unwrap();
        assert!(d.is_resolved());
        assert!(matches!(d.repairs.last(), Some(Repair::CaseFolded { .. })));
        // Two declared options differing only by case: ask, never pick.
        let d = digest(r#"{"mode": "READ"}"#, &schema).unwrap();
        let Outcome::Clarify(questions) = d.outcome else {
            panic!("expected clarify");
        };
        assert_eq!(questions[0].candidates, vec!["Read", "read"]);
    }

    #[test]
    fn digests_fenced_output_with_trailing_comma() {
        let raw = "Sure! Here's the order:\n```json\n{\"item\": \"espresso\", \"qty\": 2,}\n```";
        let d = digest(raw, &order_schema()).unwrap();
        assert!(d.is_resolved());
        assert!(d.repairs.contains(&Repair::StrippedFence));
        assert!(d.repairs.contains(&Repair::RemovedTrailingCommas));
    }

    #[test]
    fn null_on_an_optional_field_is_a_named_drop_not_a_question() {
        // Null and omission assert the same thing for an optional field, so
        // the drop is ledgered rather than asked about.
        let raw = r#"{"item": "espresso", "qty": 2, "gift_wrap": null}"#;
        let d = digest(raw, &order_schema()).unwrap();
        let Outcome::Resolved(value) = &d.outcome else {
            panic!("expected resolved, not a question about a stated absence");
        };
        assert!(value.as_obj().unwrap().get("gift_wrap").is_none());
        assert!(
            d.repairs.contains(&Repair::DroppedNullOptional {
                path: "$.gift_wrap".to_string()
            }),
            "repairs: {:?}",
            d.repairs
        );
    }

    #[test]
    fn null_on_an_optional_any_field_is_a_preserved_value_not_a_drop() {
        // Schema::Any admits null as a value, so an explicit null there is
        // data, not a stated absence — dropping it would be loss wearing a
        // repair's name.
        let schema = Schema::obj([
            Field::req("item", Schema::Str),
            Field::opt("note", Schema::Any),
        ]);
        let raw = r#"{"item": "espresso", "note": null}"#;
        let d = digest(raw, &schema).unwrap();
        let Outcome::Resolved(value) = &d.outcome else {
            panic!("expected resolved");
        };
        assert_eq!(value.as_obj().unwrap().get("note"), Some(&Value::Null));
        assert!(
            !d.repairs
                .iter()
                .any(|r| matches!(r, Repair::DroppedNullOptional { .. })),
            "repairs: {:?}",
            d.repairs
        );
    }

    #[test]
    fn tri_state_null_schemas_are_rejected_at_the_boundary_not_mismodeled() {
        // Patch-style semantics (omitted = unchanged, null = clear) need a
        // nullable type this language deliberately cannot express. The
        // spelling must fail loudly at schema load, never silently collapse
        // into optional-and-droppable.
        let err = Schema::from_json_schema(
            r#"{"type": "object", "properties": {"col": {"type": ["string", "null"]}}}"#,
        )
        .expect_err("nullable union types must not load");
        assert!(err.path.contains("col"), "path: {}", err.path);
    }

    #[test]
    fn ambiguity_becomes_a_question_not_a_coin_flip() {
        let raw = r#"{"item": "espresso", "qty": "2 or 3"}"#;
        let d = digest(raw, &order_schema()).unwrap();
        match d.outcome {
            Outcome::Clarify(qs) => {
                assert_eq!(qs.len(), 1);
                assert_eq!(qs[0].path, "$.qty");
                assert_eq!(qs[0].candidates, vec!["2", "3"]);
            }
            other => panic!("expected clarification, got {other:?}"),
        }
    }

    #[test]
    fn a_requested_answer_is_applied_and_recorded() {
        let raw = r#"{"item": "espresso", "qty": "2 or 3"}"#;
        let d = digest_with_answers(
            raw,
            &order_schema(),
            [Answer::new("$.qty", Value::Num(2.0))],
        )
        .unwrap();

        assert!(d.is_resolved());
        assert_eq!(d.answers, vec![Answer::new("$.qty", Value::Num(2.0))]);
        match d.outcome {
            Outcome::Resolved(Value::Obj(object)) => {
                assert_eq!(object.get("qty"), Some(&Value::Num(2.0)));
            }
            other => panic!("expected answered value, got {other:?}"),
        }
    }

    #[test]
    fn a_missing_required_field_can_be_answered() {
        let raw = r#"{"item": "espresso"}"#;
        let d = digest_with_answers(
            raw,
            &order_schema(),
            [Answer::new("$.qty", Value::Num(2.0))],
        )
        .unwrap();

        match d.outcome {
            Outcome::Resolved(Value::Obj(object)) => {
                assert_eq!(object.get("qty"), Some(&Value::Num(2.0)));
            }
            other => panic!("expected answered value, got {other:?}"),
        }
    }

    #[test]
    fn answers_cannot_mutate_paths_digest_did_not_question() {
        let raw = r#"{"item": "espresso", "qty": "2 or 3"}"#;
        let error = digest_with_answers(
            raw,
            &order_schema(),
            [Answer::new("$.item", Value::Str("tea".into()))],
        )
        .unwrap_err();

        assert_eq!(
            error,
            AnswerError::NotRequested {
                path: "$.item".into()
            }
        );
    }

    #[test]
    fn answers_support_ambiguous_array_members() {
        let raw = r#"["2 or 3"]"#;
        let d = digest_with_answers(
            raw,
            &Schema::Arr(Box::new(Schema::num())),
            [Answer::new("$[0]", Value::Num(3.0))],
        )
        .unwrap();

        assert!(d.is_resolved());
    }

    #[test]
    fn honest_coercions_are_named() {
        let raw = r#"{"item": "espresso", "qty": "2", "gift_wrap": "yes"}"#;
        let d = digest(raw, &order_schema()).unwrap();
        assert!(d.is_resolved());
        assert_eq!(
            d.repairs
                .iter()
                .filter(|r| matches!(r, Repair::Coerced { .. }))
                .count(),
            2
        );
    }

    #[test]
    fn hedges_resolve_with_a_record() {
        let raw = r#"{"item": "espresso", "qty": "~3"}"#;
        let d = digest(raw, &order_schema()).unwrap();
        assert!(d.is_resolved());
        assert!(d
            .repairs
            .iter()
            .any(|r| matches!(r, Repair::HedgeResolved { .. })));
    }

    #[test]
    fn unknown_keys_are_extra_meaning_not_error() {
        let raw = r#"{"item": "espresso", "qty": 1, "note": "oat milk"}"#;
        let d = digest(raw, &order_schema()).unwrap();
        match d.outcome {
            Outcome::Resolved(Value::Obj(m)) => assert!(m.contains_key("note")),
            other => panic!("expected resolved object, got {other:?}"),
        }
    }

    #[test]
    fn python_flavored_output_is_repaired() {
        let raw = "{'item': 'espresso', 'qty': 2, 'gift_wrap': True}";
        let d = digest(raw, &order_schema()).unwrap();
        assert!(d.is_resolved());
        assert!(d.repairs.contains(&Repair::RequotedStrings));
        assert!(d.repairs.contains(&Repair::PythonLiterals));
    }

    #[test]
    fn bare_keys_are_quoted() {
        let raw = r#"{item: "espresso", qty: 2}"#;
        let d = digest(raw, &order_schema()).unwrap();
        assert!(d.is_resolved());
        assert!(d.repairs.contains(&Repair::QuotedKeys));
    }

    #[test]
    fn json_schema_subset_drives_digestion() {
        let schema = Schema::from_json_schema(
            r#"{
                "type": "object",
                "properties": {
                    "item": {"type": "string"},
                    "qty": {"type": "number", "minimum": 1, "maximum": 99},
                    "gift_wrap": {"type": "boolean"},
                    "size": {"enum": ["small", "double", "triple"]}
                },
                "required": ["item", "qty"]
            }"#,
        )
        .unwrap();

        let d = digest(
            r#"{"item":"espresso","qty":"2 or 3","size":"DOUBLE"}"#,
            &schema,
        )
        .unwrap();
        assert_eq!(d.repairs[0].code(), "case_folded");
        assert!(matches!(d.outcome, Outcome::Clarify(_)));
    }

    #[test]
    fn unsupported_schema_constraints_are_rejected() {
        let error =
            Schema::from_json_schema(r#"{"type":"string","pattern":"^[a-z]+$"}"#).unwrap_err();

        assert_eq!(error.path, "$schema.pattern");
        assert!(error.message.contains("refuses to ignore"));
    }

    #[test]
    fn invalid_schema_range_is_rejected() {
        let error =
            Schema::from_json_schema(r#"{"type":"number","minimum":10,"maximum":1}"#).unwrap_err();

        assert!(error.message.contains("minimum"));
    }

    #[test]
    fn reconcile_takes_field_majority_and_asks_about_dissent() {
        let s1 = r#"{"item": "espresso", "qty": 2}"#;
        let s2 = r#"{"item": "espresso", "qty": 2}"#;
        let s3 = r#"{"item": "espresso", "qty": 3}"#;
        let r = reconcile(&[s1, s2, s3], &order_schema()).unwrap();
        match r.outcome {
            Outcome::Resolved(Value::Obj(m)) => {
                assert_eq!(m.get("qty"), Some(&Value::Num(2.0)));
            }
            other => panic!("expected majority resolution, got {other:?}"),
        }
    }

    #[test]
    fn reconcile_with_no_majority_asks() {
        let s1 = r#"{"qty": 1, "item": "a"}"#;
        let s2 = r#"{"qty": 2, "item": "a"}"#;
        let r = reconcile(&[s1, s2], &Schema::Any).unwrap();
        assert!(matches!(r.outcome, Outcome::Clarify(_)));
    }

    #[test]
    fn reconcile_never_discards_a_clarifying_sample() {
        let resolved = r#"{"item":"espresso","qty":2}"#;
        let ambiguous = r#"{"item":"espresso","qty":"2 or 3"}"#;

        let reconciliation = reconcile(&[resolved, ambiguous, ambiguous], &order_schema()).unwrap();

        match reconciliation.outcome {
            Outcome::Clarify(questions) => {
                assert_eq!(questions.len(), 1);
                assert_eq!(questions[0].path, "$.qty");
            }
            other => panic!("expected clarification, got {other:?}"),
        }
    }

    #[test]
    fn reconcile_merges_candidates_for_the_same_path() {
        let first = r#"{"item":"espresso","qty":"2 or 3"}"#;
        let second = r#"{"item":"espresso","qty":"4 or 5"}"#;

        let reconciliation = reconcile(&[first, second], &order_schema()).unwrap();

        match reconciliation.outcome {
            Outcome::Clarify(questions) => {
                assert_eq!(questions.len(), 1);
                assert_eq!(questions[0].path, "$.qty");
                assert_eq!(questions[0].candidates, ["2", "3", "4", "5"]);
            }
            other => panic!("expected clarification, got {other:?}"),
        }
    }

    #[test]
    fn reconcile_rejects_an_empty_sample_set() {
        let error = reconcile(&[], &order_schema()).unwrap_err();
        assert!(error.msg.contains("at least one sample"));
    }
}
