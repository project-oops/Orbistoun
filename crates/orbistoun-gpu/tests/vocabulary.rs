//! The packet vocabulary, against captures of a guest that used it.
//!
//! # The circle this breaks
//!
//! Every layer above `data/packets.toml` is verified against something external:
//! instruction decoding against a reference disassembler, translation against a real GPU.
//! The table itself is transcribed and checked against nothing, and its own comment calls
//! the shader-address rows the least certain thing in the file.
//!
//! It is also the layer where a mistake is silent. A wrong register base attributes every
//! write in its class to the wrong register - consistently, so nothing looks odd. A wrong
//! shader-address row means shaders are looked for in the wrong place. Neither produces
//! an error; both produce a submission that yields nothing and looks like an
//! unremarkable frame.
//!
//! # Why a capture is a pair
//!
//! A recorded command buffer on its own would have to be read *through* the table under
//! test, so agreement would prove nothing. Each capture records what a library call
//! asked for **and** the bytes it appended: the call states the answer, the bytes are the
//! question.
//!
//! # An empty corpus is reported, not passed
//!
//! There are no captures yet - they need a guest that reaches the graphics layer, which
//! is the loader side's work. Until then this reports that it checked nothing, because
//! "nothing to check" and "everything checks out" must never look the same. That is the
//! same rule the device-dependent tests follow.

use std::path::{Path, PathBuf};

use orbistoun_gpu::registers::{Vocabulary, register_writes, shader_candidates};
use orbistoun_gpu::walk;
use serde::Deserialize;

/// What a capture claims the guest asked for.
#[derive(Debug, Deserialize)]
struct Capture {
    /// The library call this came from.
    call: String,
    /// Which queue the buffer belongs to.
    #[allow(dead_code)]
    queue: String,
    /// Anything worth knowing later.
    #[serde(default)]
    #[allow(dead_code)]
    note: String,
    /// Registers the call is known to have written.
    #[serde(default)]
    register: Vec<RegisterExpectation>,
    /// Shaders the call is known to have set up.
    #[serde(default)]
    shader: Vec<ShaderExpectation>,
}

/// A register the call wrote, and to what.
#[derive(Debug, Deserialize)]
struct RegisterExpectation {
    register: u32,
    value: u32,
}

/// A shader the call set up.
#[derive(Debug, Deserialize)]
struct ShaderExpectation {
    stage: String,
    address: u64,
}

fn captures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("captures")
}

/// Every capture on disk, as (name, claims, bytes).
///
/// A `.toml` with no `.bin` beside it is an error rather than a skip: it means a capture
/// was added half way, and silently ignoring it would leave somebody believing it was
/// being checked.
fn captures() -> Vec<(String, Capture, Vec<u8>)> {
    let directory = captures_dir();
    let Ok(entries) = std::fs::read_dir(&directory) else {
        return Vec::new();
    };

    let mut found = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("toml") {
            continue;
        }
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or_default()
            .to_owned();

        let text =
            std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        let capture: Capture =
            toml::from_str(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()));

        let binary = path.with_extension("bin");
        let bytes = std::fs::read(&binary).unwrap_or_else(|e| {
            panic!(
                "{}: {e} - a capture's .toml needs its .bin beside it, or half of it \
                 would be checked and nobody would know which half",
                binary.display()
            )
        });
        found.push((stem, capture, bytes));
    }
    found.sort_by(|a, b| a.0.cmp(&b.0));
    found
}

/// Checks one capture, returning how many expectations held or what disagreed.
///
/// Errors rather than panics, so the comparison itself can be tested. A comparator that
/// panics can only be exercised by data that makes it pass - which would leave the one
/// thing this file exists to do never having been run.
fn check(
    name: &str,
    capture: &Capture,
    bytes: &[u8],
    vocabulary: &Vocabulary,
) -> Result<usize, String> {
    let walked = walk(bytes);
    let writes = register_writes(&walked, bytes, vocabulary);
    let mut checked = 0usize;

    for expectation in &capture.register {
        let Some(found) = writes
            .iter()
            .find(|write| write.register == expectation.register)
        else {
            return Err(format!(
                "{name} ({}): the call wrote register {:#x}, and decoding its bytes found \
                 no write to that register at all. Registers seen: {:?}. Suspect the \
                 register bases in data/packets.toml before suspecting the capture",
                capture.call,
                expectation.register,
                writes.iter().map(|w| w.register).collect::<Vec<_>>()
            ));
        };
        if found.value != expectation.value {
            return Err(format!(
                "{name} ({}): register {:#x} was written with {:#x} according to the \
                 call, and {:#x} according to the packets",
                capture.call, expectation.register, expectation.value, found.value
            ));
        }
        checked += 1;
    }

    let candidates = shader_candidates(&writes, vocabulary);
    for expectation in &capture.shader {
        let Some(found) = candidates
            .iter()
            .find(|candidate| candidate.stage == expectation.stage)
        else {
            return Err(format!(
                "{name} ({}): the call set up a {} shader, and decoding its bytes found \
                 none. Stages seen: {:?}. Suspect the shader-address rows in \
                 data/packets.toml",
                capture.call,
                expectation.stage,
                candidates.iter().map(|c| &c.stage).collect::<Vec<_>>()
            ));
        };
        if found.address != expectation.address {
            return Err(format!(
                "{name} ({}): the {} shader is at {:#x} according to the call, and {:#x} \
                 according to the packets. A difference in only the top or bottom half \
                 means the address halves are paired the wrong way round",
                capture.call, expectation.stage, expectation.address, found.address
            ));
        }
        checked += 1;
    }

    Ok(checked)
}

#[test]
fn every_capture_agrees_with_the_register_vocabulary() {
    let vocabulary = Vocabulary::builtin().expect("vocabulary");
    let captures = captures();

    if captures.is_empty() {
        // Loud, and deliberately not an ignored test: a harness reports an ignored test
        // as a kind of pass, and this needs to read as "checked nothing".
        println!();
        println!("!! NO CAPTURES: the packet vocabulary was checked against nothing.");
        println!("!! data/packets.toml remains transcribed and unverified.");
        println!(
            "!! See {}/README.md for what one is.",
            captures_dir().display()
        );
        println!();
        return;
    }

    let mut checked = 0usize;
    for (name, capture, bytes) in &captures {
        match check(name, capture, bytes, &vocabulary) {
            Ok(count) => checked += count,
            Err(reason) => panic!("{reason}"),
        }
    }

    println!(
        "{checked} expectation(s) verified across {} capture(s)",
        captures.len()
    );
    assert!(
        checked > 0,
        "{} capture(s) on disk and not one expectation between them - a capture that \
         claims nothing checks nothing",
        captures.len()
    );
}

/// A command stream that writes one register, built the way a guest would.
///
/// Synthetic, and **not evidence about the vocabulary** - it is generated from the same
/// table it would be checking, so of course they agree. It exists to exercise the
/// comparison, which otherwise could not run at all until a real capture arrived.
fn synthetic_stream(register: u32, value: u32, vocabulary: &Vocabulary) -> Vec<u8> {
    let (opcode, base) = vocabulary
        .opcode_for_register(register)
        .expect("an opcode reaching that register");
    let words: [u32; 3] = [
        (3 << 30) | (1 << 16) | (u32::from(opcode) << 8),
        register - base,
        value,
    ];
    words.iter().flat_map(|w| w.to_le_bytes()).collect()
}

fn synthetic_capture(register: u32, value: u32) -> Capture {
    Capture {
        call: "synthetic".to_owned(),
        queue: "compute".to_owned(),
        note: String::new(),
        register: vec![RegisterExpectation { register, value }],
        shader: Vec::new(),
    }
}

#[test]
fn the_comparison_accepts_a_capture_that_agrees() {
    // Proves the harness can load, decode and compare. Says nothing about whether the
    // table is right, and is only here because the corpus is empty - a comparator that
    // has never run is not a check.
    let vocabulary = Vocabulary::builtin().expect("vocabulary");
    let register = 0x2E0C;
    let bytes = synthetic_stream(register, 0x1234, &vocabulary);

    let result = check(
        "synthetic",
        &synthetic_capture(register, 0x1234),
        &bytes,
        &vocabulary,
    );
    assert_eq!(result, Ok(1), "the comparison should accept agreement");
}

#[test]
fn the_comparison_rejects_a_capture_that_disagrees() {
    // The half that matters. A comparator that only ever sees agreement cannot be
    // distinguished from one that returns success unconditionally, and this file's whole
    // purpose is to fail when the table is wrong.
    let vocabulary = Vocabulary::builtin().expect("vocabulary");
    let register = 0x2E0C;
    let bytes = synthetic_stream(register, 0x1234, &vocabulary);

    // The bytes say 0x1234; the capture claims the call asked for something else.
    let wrong_value = check(
        "synthetic",
        &synthetic_capture(register, 0x9999),
        &bytes,
        &vocabulary,
    );
    let reason = wrong_value.expect_err("a mismatched value must be reported");
    assert!(reason.contains("0x9999"), "got: {reason}");

    // And a register the packets never wrote at all.
    let wrong_register = check(
        "synthetic",
        &synthetic_capture(0x2C0C, 0x1234),
        &bytes,
        &vocabulary,
    );
    let reason = wrong_register.expect_err("a missing register must be reported");
    assert!(
        reason.contains("no write to that register"),
        "got: {reason}"
    );
}

#[test]
fn the_capture_directory_explains_itself() {
    // The format is what the other side of this has to produce, so the description of it
    // is part of the deliverable rather than a nicety. A missing README means somebody
    // captures the wrong thing and finds out a round trip later.
    let readme = captures_dir().join("README.md");
    assert!(
        Path::new(&readme).exists(),
        "{} should describe what a capture is",
        readme.display()
    );
}
