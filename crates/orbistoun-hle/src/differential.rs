//! What a published implementation of the same interface does, recorded so orbistoun can be
//! diffed against it.
//!
//! The records come from `tools/differential/reference.c`, run wherever the reference library
//! lives. It emits its inputs with its results and the checker rebuilds each call from them, so
//! the two sides cannot drift into comparing different calls. Agreement shows orbistoun matches
//! that library, not the target, whose C library is FreeBSD-derived; so a record lands at
//! [`Oracle::Differential`] rather than `measured`, and stays probeable (D478).
//!
//! [`Oracle::Differential`]: crate::knowledge::Oracle::Differential

use std::collections::BTreeMap;

/// One argument as the reference recorded passing it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Argument {
    /// A NUL-terminated string, as bytes rather than text.
    ///
    /// `strcmp` compares as `unsigned char`, so `"\x80"` against `"a"` is positive and a signed-char
    /// implementation answers backwards; a `String` could not hold that case.
    Text(Vec<u8>),
    /// A raw byte blob, as plain hex.
    ///
    /// Element data contains NULs (a four-byte `5` is `05 00 00 00`), which a NUL-terminated field
    /// would stop at.
    Bytes(Vec<u8>),
    /// A null pointer, which is a value and not a missing argument.
    ///
    /// `strtok(NULL, ..)` continues from the last call, which an empty string does not, and both
    /// occur in one sequence.
    Null,
    /// A signed integer, passed as written.
    Signed(i64),
    /// An unsigned integer, passed as written.
    Unsigned(u64),
}

impl Argument {
    /// Reads one `s:`, `i:` or `u:` field.
    ///
    /// `s:` takes the rest of the field verbatim, spaces and colons included: `"   42"` is exactly
    /// the input whose leading-space handling is being checked.
    #[must_use]
    pub fn parse(field: &str) -> Option<Self> {
        let (tag, rest) = field.split_at(field.find(':')? + 1);
        match tag {
            "s:" => unescape(rest).map(Self::Text),
            "b:" => hex_bytes(rest).map(Self::Bytes),
            "n:" if rest.is_empty() => Some(Self::Null),
            "i:" => rest.parse().ok().map(Self::Signed),
            "u:" => rest.parse().ok().map(Self::Unsigned),
            _ => None,
        }
    }
}

/// Decodes a text field's `\xNN` escapes back to the bytes the reference passed.
///
/// The format escapes high bytes, pipes and backslashes. A malformed escape answers `None`
/// rather than dropping the byte, so no case is compared against the wrong input.
fn unescape(field: &str) -> Option<Vec<u8>> {
    let raw = field.as_bytes();
    let mut out = Vec::with_capacity(raw.len());
    let mut at = 0;
    while at < raw.len() {
        if raw[at] != b'\\' {
            out.push(raw[at]);
            at += 1;
            continue;
        }
        let digits = field.get(at + 2..at + 4)?;
        if raw.get(at + 1) != Some(&b'x') {
            return None;
        }
        out.push(u8::from_str_radix(digits, 16).ok()?);
        at += 4;
    }
    Some(out)
}

/// Decodes a `b:` field's plain hex, which must be whole bytes.
///
/// An odd digit count means a truncated record and is refused rather than rounded.
fn hex_bytes(field: &str) -> Option<Vec<u8>> {
    if field.len() % 2 != 0 {
        return None;
    }
    (0..field.len())
        .step_by(2)
        .map(|at| u8::from_str_radix(field.get(at..at + 2)?, 16).ok())
        .collect()
}

/// One call the reference made, and what it answered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Case {
    /// Unique, and what an assertion names to claim it.
    pub id: String,
    /// The function the reference called.
    pub function: String,
    /// The arguments it passed, in order.
    pub arguments: Vec<Argument>,
    /// What it returned.
    pub returned: u64,
    /// What `errno` held afterwards. Zero when it was not set.
    pub errno: u64,
    /// Out-parameters by name: an end-pointer offset, a buffer window.
    pub out: BTreeMap<String, String>,
}

/// Every case one reference run produced.
#[derive(Debug, Clone, Default)]
pub struct Reference {
    /// The library it ran against, as `glibc 2.39`. Every record cites it.
    pub library: String,
    /// The cases, in the order the run made them.
    pub cases: Vec<Case>,
}

impl Reference {
    /// Reads one run's output.
    ///
    /// Unknown record types are ignored, so a newer reference stays readable. A malformed known
    /// record drops its case, since a case missing its return value would compare against zero.
    #[must_use]
    pub fn parse(text: &str) -> Self {
        let mut out = Self::default();
        let mut by_id: BTreeMap<String, usize> = BTreeMap::new();
        for line in text.lines() {
            let fields: Vec<&str> = line.split('|').collect();
            match fields.as_slice() {
                ["REF", "library", name, version] => {
                    out.library = format!("{name} {version}");
                }
                ["REF", "case", id, function, arguments @ ..] => {
                    let parsed: Option<Vec<Argument>> =
                        arguments.iter().map(|a| Argument::parse(a)).collect();
                    let Some(parsed) = parsed else { continue };
                    by_id.insert((*id).to_owned(), out.cases.len());
                    out.cases.push(Case {
                        id: (*id).to_owned(),
                        function: (*function).to_owned(),
                        arguments: parsed,
                        returned: 0,
                        errno: 0,
                        out: BTreeMap::new(),
                    });
                }
                ["REF", "ret", id, value] => {
                    if let (Some(&at), Some(n)) = (by_id.get(*id), hex(value)) {
                        out.cases[at].returned = n;
                    }
                }
                ["REF", "errno", id, value] => {
                    if let (Some(&at), Some(n)) = (by_id.get(*id), hex(value)) {
                        out.cases[at].errno = n;
                    }
                }
                ["REF", "out", id, name, value] => {
                    if let Some(&at) = by_id.get(*id) {
                        out.cases[at]
                            .out
                            .insert((*name).to_owned(), (*value).to_owned());
                    }
                }
                _ => {}
            }
        }
        out
    }

    /// The cases grouped into sequences, in the order they were recorded.
    ///
    /// A step is `name#N`: stateful functions such as `strtok` are recorded as numbered steps and
    /// replayed in order against one buffer. A case with no `#` is a sequence of one.
    #[must_use]
    pub fn sequences(&self) -> Vec<(String, Vec<&Case>)> {
        let mut out: Vec<(String, Vec<&Case>)> = Vec::new();
        for case in &self.cases {
            let name = case.id.split('#').next().unwrap_or(&case.id).to_owned();
            match out.last_mut() {
                Some((last, steps)) if *last == name => steps.push(case),
                _ => out.push((name, vec![case])),
            }
        }
        out
    }

    /// The case with this id.
    #[must_use]
    pub fn get(&self, id: &str) -> Option<&Case> {
        self.cases.iter().find(|c| c.id == id)
    }
}

/// A `0x`-prefixed hexadecimal value.
fn hex(text: &str) -> Option<u64> {
    u64::from_str_radix(text.strip_prefix("0x")?, 16).ok()
}

#[cfg(test)]
mod tests {
    use super::{Argument, Reference};

    const RUN: &str = concat!(
        "REF|library|glibc|2.39\n",
        "REF|case|strtoul/trailing-text|strtoul|s:42abc|i:10\n",
        "REF|ret|strtoul/trailing-text|0x2a\n",
        "REF|errno|strtoul/trailing-text|0x0\n",
        "REF|out|strtoul/trailing-text|end_offset|0x2\n"
    );

    /// A run is read back with its library, its inputs and all three answers.
    #[test]
    fn a_run_reads_back_whole() {
        let reference = Reference::parse(RUN);
        assert_eq!(reference.library, "glibc 2.39");
        let case = reference.get("strtoul/trailing-text").expect("the case");
        assert_eq!(case.function, "strtoul");
        assert_eq!(
            case.arguments,
            vec![Argument::Text(b"42abc".to_vec()), Argument::Signed(10)]
        );
        assert_eq!(case.returned, 0x2a);
        assert_eq!(case.out.get("end_offset").map(String::as_str), Some("0x2"));
    }

    /// A subject's leading spaces survive parsing, because they are the case under test.
    #[test]
    fn a_subject_keeps_the_whitespace_that_makes_it_interesting() {
        let run = "REF|case|x|strtoul|s:   42|i:10\nREF|ret|x|0x2a\n";
        let reference = Reference::parse(run);
        assert_eq!(
            reference.get("x").expect("the case").arguments[0],
            Argument::Text(b"   42".to_vec())
        );
    }

    /// An empty string argument is a value, not a missing field.
    #[test]
    fn an_empty_subject_is_an_argument() {
        let run = "REF|case|x|strcspn|s:abc|s:\nREF|ret|x|0x3\n";
        let reference = Reference::parse(run);
        assert_eq!(
            reference.get("x").expect("the case").arguments[1],
            Argument::Text(Vec::new())
        );
    }

    /// An escaped high byte decodes to the byte, which a raw text field could not carry.
    #[test]
    fn an_escaped_byte_decodes_to_the_byte() {
        let run = "REF|case|x|strcmp|s:\\x80|s:a\nREF|ret|x|0x1\n";
        let reference = Reference::parse(run);
        assert_eq!(
            reference.get("x").expect("the case").arguments[0],
            Argument::Text(vec![0x80])
        );
    }

    /// A pipe inside a subject is escaped, so the format survives its own content.
    #[test]
    fn an_escaped_separator_does_not_split_the_record() {
        let run = "REF|case|x|strcmp|s:a\\x7cb|s:a\nREF|ret|x|0x1\n";
        let reference = Reference::parse(run);
        assert_eq!(
            reference.get("x").expect("the case").arguments[0],
            Argument::Text(b"a|b".to_vec())
        );
    }

    /// A byte blob carries NULs, which a text field could not.
    #[test]
    fn a_blob_argument_carries_nul_bytes() {
        let run = "REF|case|x|qsort|b:0500000003000000|u:4|s:int32-asc\nREF|ret|x|0x0\n";
        let reference = Reference::parse(run);
        assert_eq!(
            reference.get("x").expect("the case").arguments[0],
            Argument::Bytes(vec![5, 0, 0, 0, 3, 0, 0, 0])
        );
    }

    /// An empty blob is an empty array, not a missing argument.
    #[test]
    fn an_empty_blob_is_an_empty_array() {
        let run = "REF|case|x|bsearch|b:|u:4|s:int32-asc|i:1\nREF|ret|x|0x0\n";
        let reference = Reference::parse(run);
        assert_eq!(
            reference.get("x").expect("the case").arguments[0],
            Argument::Bytes(Vec::new())
        );
    }

    /// Half a byte is a truncated record, and the case is dropped rather than guessed.
    #[test]
    fn an_odd_length_blob_is_refused() {
        let run = "REF|case|x|qsort|b:050|u:4|s:int32-asc\nREF|ret|x|0x0\n";
        assert!(
            Reference::parse(run).get("x").is_none(),
            "a truncated array must not become a case"
        );
    }

    /// Steps of one sequence group together, and a lone case is a sequence of one.
    #[test]
    fn numbered_steps_group_into_one_sequence() {
        let run = concat!(
            "REF|case|strtok/a#0|strtok|s:x,y|s:,\nREF|ret|strtok/a#0|0x1\n",
            "REF|case|strtok/a#1|strtok|n:|s:,\nREF|ret|strtok/a#1|0x1\n",
            "REF|case|strcmp/b|strcmp|s:a|s:a\nREF|ret|strcmp/b|0x0\n"
        );
        let reference = Reference::parse(run);
        let grouped = reference.sequences();
        assert_eq!(grouped.len(), 2);
        assert_eq!(grouped[0].0, "strtok/a");
        assert_eq!(grouped[0].1.len(), 2, "both steps, in order");
        assert_eq!(grouped[1].1.len(), 1, "a lone case is a sequence of one");
    }

    /// A null argument is distinct from an empty string.
    #[test]
    fn a_null_argument_is_distinct_from_an_empty_string() {
        let run = "REF|case|x|strtok|n:|s:\nREF|ret|x|0x0\n";
        let case = Reference::parse(run);
        let case = case.get("x").expect("the case");
        assert_eq!(case.arguments[0], Argument::Null);
        assert_eq!(case.arguments[1], Argument::Text(Vec::new()));
    }

    /// A record type this does not know is skipped, not fatal.
    #[test]
    fn an_unknown_record_does_not_stop_the_rest() {
        let run = "REF|something|new|0x1\nREF|case|x|strspn|s:a|s:a\nREF|ret|x|0x1\n";
        assert_eq!(Reference::parse(run).cases.len(), 1);
    }
}
