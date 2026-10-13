//! A call spec: a measurement written as data, run over the command protocol.
//!
//! Most questions put to the hardware share one shape: find a function, give it buffers filled
//! with a byte nothing writes by chance, call it with each set of arguments in turn, and dump the
//! buffers afterwards. A [`Script`] says that as steps, and [`run`] carries them out through a
//! [`Client`], so asking such a question needs a file rather than new probe code. The transcript
//! the client keeps is the one `orbistoun-cli probe` reads.
//!
//! ```toml
//! [[step]]
//! resolve = { library = "libSceAjm", symbol = "sceAjmInstanceCreate" }
//! as = "create"
//!
//! [[step]]
//! scratch = { bytes = 16, fill = 0xcc }
//! as = "out"
//!
//! [[step]]
//! call = "create"
//! args = [0, "0x10", "out"]
//!
//! [[step]]
//! read = "out"
//! bytes = 16
//! ```
//!
//! A value is a number, a `0x` string, or the name an earlier step bound with `as`: a resolved
//! address, a scratch block's address, or a call's return value.

use std::collections::BTreeMap;
use std::io::{Read, Write};

use serde::Deserialize;

use crate::Outcome;
use crate::client::{Answer, Client, ClientError};

/// A spec: its steps, in order.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Script {
    /// What to do, in order.
    #[serde(rename = "step", default)]
    pub steps: Vec<Step>,
}

/// One step: exactly one of `resolve`, `scratch`, `call` or `read`, optionally naming its result.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Step {
    /// The name later steps refer to this step's result by.
    #[serde(rename = "as")]
    pub name: Option<String>,
    /// Look a symbol up.
    pub resolve: Option<Symbol>,
    /// Take a scratch block.
    pub scratch: Option<Block>,
    /// Call this address.
    pub call: Option<Value>,
    /// The call's arguments, at most six.
    #[serde(default)]
    pub args: Vec<Value>,
    /// Read memory from this address.
    pub read: Option<Value>,
    /// How many bytes the read takes.
    pub bytes: Option<u64>,
}

/// A symbol to resolve.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Symbol {
    /// The library that exports it.
    pub library: String,
    /// Its name.
    pub symbol: String,
}

/// A scratch block to take.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Block {
    /// Its size.
    pub bytes: u64,
    /// The byte every one of them holds before the call.
    pub fill: u8,
}

/// A number, or text: `0x` hex, or a name bound earlier.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(untagged)]
pub enum Value {
    /// A literal.
    Number(u64),
    /// Hex, or a name.
    Text(String),
}

/// Why a script stopped.
#[derive(Debug, thiserror::Error)]
pub enum ScriptError {
    /// The spec did not parse.
    #[error("the spec does not parse: {0}")]
    Spec(#[from] toml::de::Error),
    /// A step names no action, or more than one.
    #[error("step {step} must do exactly one of resolve, scratch, call and read")]
    Shape {
        /// Which step, from one.
        step: usize,
    },
    /// A step refers to a name nothing bound, or a value that is not a number.
    #[error("step {step}: `{value}` is neither a number nor a name an earlier step bound")]
    Unbound {
        /// Which step, from one.
        step: usize,
        /// The value as written.
        value: String,
    },
    /// A read without its length, or a call with more arguments than registers.
    #[error("step {step}: {detail}")]
    Argument {
        /// Which step, from one.
        step: usize,
        /// What is wrong.
        detail: &'static str,
    },
    /// A step's result is needed by name and the step produced none.
    #[error("step {step} binds `{name}`, and its answer carried no value: {outcome}")]
    NoValue {
        /// Which step, from one.
        step: usize,
        /// The name it was to bind.
        name: String,
        /// What it answered instead.
        outcome: String,
    },
    /// The session failed under the step.
    #[error("step {step}: {source}")]
    Client {
        /// Which step, from one.
        step: usize,
        /// What the client said.
        source: ClientError,
    },
}

impl Script {
    /// Reads a spec.
    ///
    /// # Errors
    ///
    /// [`ScriptError::Spec`] when it is not one, and [`ScriptError::Shape`] for a step that does
    /// not name exactly one action - found before anything is sent.
    pub fn parse(text: &str) -> Result<Self, ScriptError> {
        let script: Self = toml::from_str(text)?;
        for (index, step) in script.steps.iter().enumerate() {
            let actions = [
                step.resolve.is_some(),
                step.scratch.is_some(),
                step.call.is_some(),
                step.read.is_some(),
            ];
            if actions.iter().filter(|&&a| a).count() != 1 {
                return Err(ScriptError::Shape { step: index + 1 });
            }
        }
        Ok(script)
    }
}

/// Carries a script's steps out in order, answering what each produced.
///
/// A step whose answer is not a value - a symbol absent, a call that died - still counts; the
/// script stops only where a later step needs a value it did not produce, or the session fails.
/// Everything sent and received is in the client's transcript either way.
///
/// # Errors
///
/// The first step that could not be carried out.
pub fn run<S: Read + Write>(
    client: &mut Client<S>,
    script: &Script,
) -> Result<Vec<Answer>, ScriptError> {
    let mut bound: BTreeMap<String, u64> = BTreeMap::new();
    let mut answers = Vec::with_capacity(script.steps.len());
    for (index, step) in script.steps.iter().enumerate() {
        let number = index + 1;
        let value = |v: &Value| resolve_value(v, &bound, number);
        let answer = if let Some(symbol) = &step.resolve {
            client.resolve(&symbol.library, &symbol.symbol)
        } else if let Some(block) = &step.scratch {
            client.scratch(block.bytes, block.fill)
        } else if let Some(address) = &step.call {
            if step.args.len() > 6 {
                return Err(ScriptError::Argument {
                    step: number,
                    detail: "a call takes at most six arguments",
                });
            }
            let address = value(address)?;
            let arguments = step.args.iter().map(value).collect::<Result<Vec<_>, _>>()?;
            client.call(address, &arguments)
        } else if let Some(address) = &step.read {
            let Some(length) = step.bytes else {
                return Err(ScriptError::Argument {
                    step: number,
                    detail: "a read needs `bytes`",
                });
            };
            client.read(value(address)?, length)
        } else {
            return Err(ScriptError::Shape { step: number });
        }
        .map_err(|source| ScriptError::Client {
            step: number,
            source,
        })?;
        if let Some(name) = &step.name {
            let Outcome::Returned(produced) = answer.outcome else {
                return Err(ScriptError::NoValue {
                    step: number,
                    name: name.clone(),
                    outcome: format!("{:?}", answer.outcome),
                });
            };
            bound.insert(name.clone(), produced);
        }
        answers.push(answer);
    }
    Ok(answers)
}

/// A value's number: a literal, `0x` hex, or a name an earlier step bound.
fn resolve_value(
    value: &Value,
    bound: &BTreeMap<String, u64>,
    step: usize,
) -> Result<u64, ScriptError> {
    match value {
        Value::Number(number) => Ok(*number),
        Value::Text(text) => bound
            .get(text)
            .copied()
            .or_else(|| {
                text.strip_prefix("0x")
                    .and_then(|hex| u64::from_str_radix(hex, 16).ok())
            })
            .ok_or_else(|| ScriptError::Unbound {
                step,
                value: text.clone(),
            }),
    }
}

/// How big a block a drafted spec gives each pointer argument, and what fills it.
///
/// Larger than most out-structures the measurements so far found, so a write past the declared
/// width shows in the dump rather than landing outside it.
const DRAFT_BLOCK: (u64, u8) = (0x40, 0xcc);

/// What a function is, for a drafted spec.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Declared<'a> {
    /// The library that exports it.
    pub library: &'a str,
    /// Its name, or its NID as `0x` hex where it has none.
    pub symbol: &'a str,
    /// Its NID.
    pub nid: u64,
    /// How many integer arguments it takes.
    pub arity: u8,
}

/// A spec to start a measurement of `function` from: resolve it, give every pointer-shaped
/// argument a [`DRAFT_BLOCK`], call it, dump each block, then the same call with every pointer
/// argument null.
///
/// `shape` is the argument signature a run recorded, such as `(u32, ptr, u64)`; an argument it
/// calls `ptr` gets a block, any other is zero. With no run to go on, every argument is zero and
/// the draft says so. The integer arms are the asker's to set: a draft is a start, not a question.
#[must_use]
pub fn draft(function: &Declared<'_>, shape: Option<&str>) -> String {
    use std::fmt::Write as _;

    let kinds: Vec<&str> = shape
        .map(|s| {
            s.trim_matches(|c| c == '(' || c == ')')
                .split(',')
                .map(str::trim)
                .collect()
        })
        .unwrap_or_default();
    let is_pointer = |index: usize| kinds.get(index).is_some_and(|&k| k == "ptr");
    let (bytes, fill) = DRAFT_BLOCK;

    let mut out = String::new();
    let _ = writeln!(
        out,
        "# {} - {}, NID {:#018x}, {} argument(s).",
        function.symbol, function.library, function.nid, function.arity
    );
    match shape {
        Some(shape) => {
            let _ = writeln!(
                out,
                "# Argument shapes from a run: {shape}. Each `ptr` gets {bytes:#x} bytes of {fill:#04x}."
            );
        }
        None => {
            let _ = writeln!(
                out,
                "# No run has called it, so which arguments are pointers is unknown: all are zero."
            );
        }
    }
    let _ = writeln!(
        out,
        concat!(
            "# A draft: set the integer arguments to the arms the question needs. Arguments past\n",
            "# the function's real count hold whatever its caller's registers did, and may only\n",
            "# look like pointers.\n"
        )
    );
    let _ = writeln!(
        out,
        "[[step]]\nresolve = {{ library = \"{}\", symbol = \"{}\" }}\nas = \"function\"\n",
        function.library, function.symbol
    );
    let mut args = Vec::new();
    for index in 0..usize::from(function.arity) {
        if is_pointer(index) {
            let _ = writeln!(
                out,
                "[[step]]\nscratch = {{ bytes = {bytes:#x}, fill = {fill:#04x} }}\nas = \"arg{index}\"\n"
            );
            args.push(format!("\"arg{index}\""));
        } else {
            args.push("0".to_owned());
        }
    }
    let _ = writeln!(
        out,
        "[[step]]\ncall = \"function\"\nargs = [{}]\n",
        args.join(", ")
    );
    for index in (0..usize::from(function.arity)).filter(|&i| is_pointer(i)) {
        let _ = writeln!(out, "[[step]]\nread = \"arg{index}\"\nbytes = {bytes:#x}\n");
    }
    if (0..usize::from(function.arity)).any(is_pointer) {
        let nulls = vec!["0"; usize::from(function.arity)].join(", ");
        let _ = writeln!(
            out,
            "# The null arm: every pointer argument zero.\n[[step]]\ncall = \"function\"\nargs = [{nulls}]"
        );
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{Declared, Script, ScriptError, Value, draft};

    /// A drafted spec parses as a spec, gives each pointer argument a block it reads back, and ends
    /// with the null arm; with no shape every argument is zero and nothing is read.
    #[test]
    fn a_draft_is_a_spec_with_a_block_per_pointer() {
        let function = Declared {
            library: "libSceAjm",
            symbol: "sceAjmInstanceCreate",
            nid: 0x1234,
            arity: 4,
        };
        let text = draft(&function, Some("(u32, u64, u32, ptr)"));
        let spec = Script::parse(&text).expect("a draft parses");
        let actions: Vec<&str> = spec
            .steps
            .iter()
            .map(|s| {
                if s.resolve.is_some() {
                    "resolve"
                } else if s.scratch.is_some() {
                    "scratch"
                } else if s.call.is_some() {
                    "call"
                } else {
                    "read"
                }
            })
            .collect();
        assert_eq!(actions, ["resolve", "scratch", "call", "read", "call"]);
        assert_eq!(spec.steps[2].args[3], Value::Text("arg3".to_owned()));
        assert_eq!(spec.steps[4].args, vec![Value::Number(0); 4]);

        let blind = Script::parse(&draft(&function, None)).expect("parses");
        assert_eq!(blind.steps.len(), 2, "resolve and one call: {blind:?}");
    }

    /// A step must do exactly one thing, found before anything is sent; values are numbers, hex
    /// or names.
    #[test]
    fn a_spec_is_checked_before_it_runs() {
        let two = "[[step]]\nresolve = { library = \"l\", symbol = \"s\" }\nread = \"x\"\n";
        assert!(matches!(
            Script::parse(two),
            Err(ScriptError::Shape { step: 1 })
        ));
        let none = "[[step]]\nas = \"x\"\n";
        assert!(matches!(
            Script::parse(none),
            Err(ScriptError::Shape { step: 1 })
        ));
        let call = "[[step]]\ncall = \"0x10\"\nargs = [1, \"0x2\", \"out\"]\n";
        let script = Script::parse(call).expect("parses");
        assert_eq!(
            script.steps[0].args,
            [
                Value::Number(1),
                Value::Text("0x2".to_owned()),
                Value::Text("out".to_owned())
            ]
        );
    }
}
