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

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

/// Digest raw model output against a schema.
///
/// Never panics on ambiguity; never silently guesses. Returns a parse error
/// only when the text is unrecoverable even after every named repair pass.
pub fn digest(raw: &str, schema: &Schema) -> Result<Digestion, ParseError> {
    let mut repairs = Vec::new();
    let value = parse_leniently(raw, &mut repairs)?;

    let mut questions = Vec::new();
    let checked = check(value, schema, "$", &mut repairs, &mut questions);

    let outcome = if questions.is_empty() {
        Outcome::Resolved(checked)
    } else {
        Outcome::Clarify(questions)
    };
    Ok(Digestion { outcome, repairs })
}

// ---------------------------------------------------------------------------
// Repair pipeline — parse leniently, log every act
// ---------------------------------------------------------------------------

fn parse_leniently(raw: &str, repairs: &mut Vec<Repair>) -> Result<Value, ParseError> {
    if let Ok(v) = parse(raw) {
        return Ok(v);
    }

    let mut text = raw.to_string();
    type RepairPass = fn(&str) -> Option<String>;
    let passes: [(RepairPass, Repair); 5] = [
        (strip_fences, Repair::StrippedFence),
        (extract_json, Repair::StrippedProse),
        (python_literals, Repair::PythonLiterals),
        (requote_strings, Repair::RequotedStrings),
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
            if let Some(hit) = opts.iter().find(|o| o.to_ascii_lowercase() == folded) {
                repairs.push(Repair::CaseFolded {
                    path: path.to_string(),
                    original: s.clone(),
                });
                return Value::Str(hit.clone());
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
    if let Ok(n) = t.parse::<f64>() {
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
            if let Ok(n) = rest.trim().parse::<f64>() {
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
        if let (Ok(x), Ok(y)) = (a.trim().parse::<f64>(), b.trim().parse::<f64>()) {
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
            if let (Ok(x), Ok(y)) = (a.trim().parse::<f64>(), b.trim().parse::<f64>()) {
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
    let mut digestions = Vec::new();
    for s in samples {
        digestions.push(digest(s, schema)?);
    }
    let resolved: Vec<&Value> = digestions
        .iter()
        .filter_map(|d| match &d.outcome {
            Outcome::Resolved(v) => Some(v),
            _ => None,
        })
        .collect();

    if resolved.is_empty() {
        let mut qs: Vec<Question> = Vec::new();
        for d in &digestions {
            if let Outcome::Clarify(questions) = &d.outcome {
                for q in questions {
                    if !qs.iter().any(|e| e.path == q.path) {
                        qs.push(q.clone());
                    }
                }
            }
        }
        return Ok(Reconciliation {
            outcome: Outcome::Clarify(qs),
            per_sample: digestions,
        });
    }

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
    fn digests_fenced_output_with_trailing_comma() {
        let raw = "Sure! Here's the order:\n```json\n{\"item\": \"espresso\", \"qty\": 2,}\n```";
        let d = digest(raw, &order_schema()).unwrap();
        assert!(d.is_resolved());
        assert!(d.repairs.contains(&Repair::StrippedFence));
        assert!(d.repairs.contains(&Repair::RemovedTrailingCommas));
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
}
