//! Differential test: our decoder against a reference disassembler.
//!
//! # What this proves that a unit test cannot
//!
//! Every other test in this crate uses instruction streams this project generated
//! from its own encoding table, so they check that the decoder implements the table.
//! They cannot check that the *table* is right - a wrong row is self-consistent and
//! passes every one of them.
//!
//! These fixtures come from somewhere else entirely. LLVM compiled them from source
//! in `tools/shader-fixtures/`, and LLVM's disassembler said where each instruction
//! begins. If our offsets track that reference exactly, then every instruction length
//! in the table is right - and length is the value whose being wrong is catastrophic,
//! because one bad length shifts every instruction after it.
//!
//! # Why offsets are the assertion
//!
//! Not instruction count, which can coincidentally match while the boundaries are
//! wrong. Not the bytes, which are the input. **Offsets**: the position of every
//! instruction is a claim derived from every length before it, so the first offset
//! where the two disagree names the instruction whose encoding is wrong.
//!
//! # Provenance
//!
//! The reference is used to *detect* an error. Correcting one is done from the
//! published specification, not by reading the reference implementation's tables
//! (D085, and the note on `orbistoun-gen`'s `fixtures` module).
//!
//! Fixtures are committed, so this runs on any machine with no GPU toolchain present.
//! Regenerate with `tools/shader-fixtures/generate.sh`.

use std::path::PathBuf;

use orbistoun_shader::{EncodingTable, OperandTable, decode};

/// The built-in operand table.
fn operands() -> OperandTable {
    OperandTable::builtin().expect("built-in operand table")
}

/// One instruction as the reference disassembler reported it.
struct Reference {
    offset: u32,
    length: u32,
    mnemonic: String,
    /// Operands verbatim, e.g. `v0, s[4:5], 0x8`.
    operands: String,
}

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
}

fn read_reference(name: &str) -> Vec<Reference> {
    let path = fixtures_dir().join(format!("{name}.txt"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    text.lines()
        .filter(|line| !line.starts_with('#') && !line.trim().is_empty())
        .map(|line| {
            let mut parts = line.split_whitespace();
            let offset = parts.next().expect("offset");
            let offset = u32::from_str_radix(offset.strip_prefix("0x").unwrap_or(offset), 16)
                .expect("hexadecimal offset");
            let length = parts.next().expect("length").parse().expect("length");
            let mnemonic = parts.next().unwrap_or("?").to_owned();
            Reference {
                offset,
                length,
                mnemonic,
                // Everything left on the line. Operands contain spaces, so they cannot
                // be a fixed number of whitespace-separated fields.
                operands: parts.collect::<Vec<_>>().join(" "),
            }
        })
        // Stop at the padding the compiler writes past the end of a shader. The fixture
        // records everything the reference disassembler printed, which is the right
        // thing for it to be - a faithful record of the compiler's output - but most of
        // that output is padding: `arith` is nineteen instructions followed by
        // forty-eight `s_code_end`.
        //
        // The decoder stops there deliberately, because padding is not code, so the
        // comparison has to as well. Truncating here rather than in the fixture keeps
        // the fixture honest about what was actually produced.
        .take_while(|entry: &Reference| entry.mnemonic != PADDING)
        .collect()
}

/// The instruction compilers pad past the end of a shader with.
///
/// Deliberately an illegal instruction, so a prefetch running off the end faults instead
/// of executing whatever follows it in memory.
const PADDING: &str = "s_code_end";

fn read_binary(name: &str) -> Vec<u8> {
    let path = fixtures_dir().join(format!("{name}.gcn"));
    std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// Encoding families a mnemonic prefix is allowed to belong to.
///
/// A softer check than offsets, and it catches a different fault: a table where two
/// families have swapped identifying bits can still produce correct lengths if they
/// happen to share a width, and only a classification check notices.
fn permitted_families(mnemonic: &str) -> &'static [&'static str] {
    if mnemonic.starts_with("s_load") || mnemonic.starts_with("s_store") {
        &["SMEM"]
    } else if mnemonic.starts_with("s_") {
        &["SOP1", "SOP2", "SOPC", "SOPK", "SOPP", "SMEM"]
    } else if mnemonic.starts_with("v_") {
        &["VOP1", "VOP2", "VOPC", "VOP3", "VINTRP"]
    } else if mnemonic.starts_with("global_")
        || mnemonic.starts_with("flat_")
        || mnemonic.starts_with("scratch_")
    {
        &["FLAT"]
    } else if mnemonic.starts_with("buffer_") || mnemonic.starts_with("tbuffer_") {
        &["MUBUF", "MTBUF"]
    } else if mnemonic.starts_with("image_") {
        &["MIMG"]
    } else if mnemonic.starts_with("ds_") {
        &["DS"]
    } else if mnemonic.starts_with("exp") {
        &["EXP"]
    } else {
        // An unrecognised prefix is not a failure - it means this check has nothing to
        // say about the instruction, and the offset assertion still applies.
        &[]
    }
}

/// The fixtures, and what each was chosen to exercise.
const FIXTURES: &[(&str, &str)] = &[
    ("arith", "vector ALU, the bulk of any real shader"),
    (
        "literal",
        "trailing 32-bit literals, the highest-risk length rule",
    ),
    ("control", "scalar control flow and branches"),
    ("memory", "wide memory encodings"),
    ("compare", "vector comparison, which has its own family"),
    ("shared", "local data share"),
    (
        "buffer",
        "typed buffer access through a resource descriptor",
    ),
    (
        "pixel",
        "a fragment shader, and the export every one of them ends in",
    ),
    ("image", "texture sampling"),
    (
        "unreached",
        "SOPK, MTBUF and VINTRP, which no compiled fixture produces",
    ),
];

#[test]
fn every_fixture_on_disk_is_in_the_list() {
    // Generated fixtures carry their own extension, distinct from the one a shader
    // dumped out of a title uses: that one is console-derived and banned from the index,
    // these are committed on purpose. Sharing an extension conflated an obligation with
    // its opposite, and the provenance guard was the thing that noticed.
    //
    // The list above is written by hand, and a fixture missing from it is not a
    // failure - it is silence. Adding `unreached.s` to the generator produced its
    // `.gcn` and `.txt` and changed nothing about this suite, which reported green
    // while never opening either file.
    //
    // That is the same failure mode as a skipped device test passing quietly, and it
    // deserves the same treatment: make the gap assertable rather than remembering to
    // close it.
    let mut on_disk: Vec<String> = std::fs::read_dir(fixtures_dir())
        .expect("fixtures directory")
        .filter_map(|entry| {
            let path = entry.ok()?.path();
            (path.extension()? == "gcn").then(|| path.file_stem()?.to_str().map(str::to_owned))?
        })
        .collect();
    on_disk.sort();

    let mut listed: Vec<String> = FIXTURES.iter().map(|(n, _)| (*n).to_owned()).collect();
    listed.sort();

    assert_eq!(
        on_disk, listed,
        "the fixtures on disk and the fixtures this suite reads have diverged"
    );
}

#[test]
fn every_instruction_boundary_matches_the_reference() {
    let table = EncodingTable::builtin().expect("built-in encoding table");
    let mut total = 0usize;

    for (name, exercises) in FIXTURES {
        let reference = read_reference(name);
        let bytes = read_binary(name);
        let decoded = decode(&bytes, &table, &operands());

        assert!(
            !reference.is_empty(),
            "{name}: fixture is empty - regenerate with tools/shader-fixtures/generate.sh"
        );

        // Checked before the per-instruction comparison: a desynchronised decode
        // explains any number of downstream mismatches, and reporting those instead
        // would bury the cause under its consequences.
        assert!(
            decoded.is_trustworthy(),
            "{name} ({exercises}): decode was not trustworthy - \
             desynchronised={}, overran={}, trailing={}",
            decoded.desynchronised,
            decoded.overran,
            decoded.trailing_bytes
        );

        for (index, expected) in reference.iter().enumerate() {
            let actual = decoded.instructions.get(index).unwrap_or_else(|| {
                panic!(
                    "{name}: reference has {} instructions, we decoded {} - \
                     first missing is {} at {:#x}",
                    reference.len(),
                    decoded.instructions.len(),
                    expected.mnemonic,
                    expected.offset
                )
            });

            assert_eq!(
                actual.offset, expected.offset,
                "{name}: instruction {index} ({}) should start at {:#x} but we put it \
                 at {:#x}. Every offset is the sum of the lengths before it, so the \
                 encoding for the *previous* instruction has the wrong length.",
                expected.mnemonic, expected.offset, actual.offset
            );

            assert_eq!(
                actual.length, expected.length,
                "{name}: {} at {:#x} is {} bytes, we decoded {}",
                expected.mnemonic, expected.offset, expected.length, actual.length
            );
        }

        assert_eq!(
            decoded.instructions.len(),
            reference.len(),
            "{name}: we decoded {} instructions, the reference found {}",
            decoded.instructions.len(),
            reference.len()
        );

        total += reference.len();
    }

    assert!(total > 0, "no fixtures were checked");
}

#[test]
fn no_instruction_in_real_compiler_output_is_unrecognised() {
    // The coverage claim. These are not contrived streams - they are what a real
    // compiler emits for ordinary arithmetic, branching and memory access, so an
    // unrecognised instruction here means the table has a hole in the common path.
    let table = EncodingTable::builtin().expect("built-in encoding table");

    for (name, exercises) in FIXTURES {
        let decoded = decode(&read_binary(name), &table, &operands());
        let unknown: Vec<String> = decoded
            .instructions
            .iter()
            .filter(|i| !i.is_known())
            .map(|i| format!("{:#x} (word {:#010x})", i.offset, i.word))
            .collect();
        assert!(
            unknown.is_empty(),
            "{name} ({exercises}): {} unrecognised instructions at {}",
            unknown.len(),
            unknown.join(", ")
        );
    }
}

#[test]
fn instructions_are_classified_into_the_right_encoding_family() {
    // Offsets alone cannot catch two families with swapped identifying bits when they
    // share an instruction width. The mnemonic says which family the reference thinks
    // it is; this checks we agree.
    let table = EncodingTable::builtin().expect("built-in encoding table");
    let mut checked = 0usize;

    for (name, _) in FIXTURES {
        let reference = read_reference(name);
        let decoded = decode(&read_binary(name), &table, &operands());

        for (expected, actual) in reference.iter().zip(&decoded.instructions) {
            let permitted = permitted_families(&expected.mnemonic);
            if permitted.is_empty() {
                continue;
            }
            let family = actual
                .encoding
                .and_then(|i| table.encodings().get(usize::from(i)))
                .map_or("<unrecognised>", |e| e.name.as_str());
            assert!(
                permitted.contains(&family),
                "{name}: {} at {:#x} decoded as {family}, expected one of {permitted:?}",
                expected.mnemonic,
                expected.offset
            );
            checked += 1;
        }
    }

    assert!(
        checked > 0,
        "the prefix table matched nothing - has it drifted?"
    );
}

#[test]
fn a_worklist_over_the_whole_fixture_corpus_reads_sensibly() {
    // The end-to-end shape: decode real compiler output, accumulate coverage, rank the
    // blockers, render. Each stage is unit-tested in isolation; this is the only test
    // that runs all of them against material this project did not generate from its own
    // table, which is where an interface between them would drift unnoticed.
    use orbistoun_shader::{CorpusCoverage, MnemonicTable, report};

    let table = EncodingTable::builtin().expect("table");
    let mnemonics = MnemonicTable::builtin().expect("mnemonics");
    let mut coverage = CorpusCoverage::new();

    // Nothing is supported yet, so every instruction is a blocker. That is the honest
    // starting state and the report should say so plainly.
    for (name, _) in FIXTURES {
        coverage.observe(
            name,
            &decode(&read_binary(name), &table, &operands()),
            &|_| false,
        );
    }

    let rendered = report::render(
        &coverage,
        &table,
        &mnemonics,
        None,
        orbistoun_shader::coverage::all_ordinary,
    );
    println!("\n{rendered}");

    assert!(
        // Derived from the list rather than written out, because the previous literal
        // said nine and adding a fixture made this test fail for a reason that had
        // nothing to do with what it checks.
        rendered.contains(&format!("0 of {} complete", FIXTURES.len())),
        "nothing is supported, so no shader is complete:\n{rendered}"
    );
    assert!(
        !rendered.contains("suspect"),
        "every fixture should decode cleanly:\n{rendered}"
    );
    // The top blocker must be named, not a bare opcode number - the whole point of the
    // mnemonic table is that the first line of the worklist is actionable.
    // Anchored on the table header, not on "shaders" - the summary line contains that
    // word too, so the looser match selected the summary and the assertion below was
    // checking the wrong line entirely.
    let first = rendered
        .lines()
        .skip_while(|l| !l.contains("known"))
        .nth(1)
        .expect("a blocker row");
    assert!(
        first.contains("v_") || first.contains("s_") || first.contains("exp"),
        "top blocker should carry a name: {first}"
    );
}

/// Reduces a reference operand to the form the decoder produces.
///
/// The reference prints a multi-register operand as a range - `s[4:5]` for a 64-bit
/// value held in two registers - while the decoder reports the first register, because
/// how many registers an operand spans is a property of the *instruction*, not of the
/// operand field, and that is not decoded yet.
///
/// Collapsing the range to its base is therefore the honest comparison: it checks the
/// register number, which is what the operand field encodes, and stays silent about
/// the width, which it does not.
/// Whether two operand texts are the same number written differently.
///
/// The reference prints a memory offset in hex and a branch offset in decimal, and this
/// decoder prints every immediate in hex. That is a spelling difference and nothing
/// else - so it is collapsed here rather than by making the decoder guess which
/// immediates a disassembler would have chosen to print in base ten.
fn same_number(left: &str, right: &str) -> bool {
    fn value(text: &str) -> Option<i64> {
        text.strip_prefix("0x")
            .map_or_else(|| text.parse::<i64>(), |hex| i64::from_str_radix(hex, 16))
            .ok()
    }
    matches!((value(left), value(right)), (Some(a), Some(b)) if a == b)
}

/// Tokens the reference prints that are not operands.
///
/// The probe solver skips the same words for the same reason, and the two lists agreeing
/// is what makes a solved layout comparable to a printed one.
const MODIFIERS: &[&str] = &[
    "off", "glc", "slc", "dlc", "lds", "gds", "offen", "idxen", "tfe", "nv", "done", "compr", "vm",
];

/// An operand the reference spells as a name, and the code it stands for.
///
/// # Why these are written down here when nothing else is
///
/// Every other number in this comparison is one the reference printed. These are not:
/// `mrt0` and `p10` are spellings, and the decoder reports the field's value. Something
/// has to relate the two.
///
/// The codes were **measured** rather than transcribed - `derive_symbolic_codes` in
/// `orbistoun-gen operands` holds an instruction constant, varies the name, and
/// reads the bits that moved. What is written here is that measurement's result, so this
/// test checks that the decoder agrees with the reference *given* that mapping, and does
/// not independently establish the mapping. Worth being explicit about: if the mapping
/// were wrong, the solver would have failed to find a consistent field long before this.
fn symbolic_code(token: &str) -> Option<u32> {
    let numbered = |prefix: &str, base: u32, count: u32| -> Option<u32> {
        let index: u32 = token.strip_prefix(prefix)?.parse().ok()?;
        (index < count).then_some(base + index)
    };
    match token {
        "mrtz" => Some(8),
        "null" => Some(9),
        "prim" => Some(20),
        "p10" => Some(0),
        "p20" => Some(1),
        "p0" => Some(2),
        _ => numbered("mrt", 0, 8)
            .or_else(|| numbered("pos", 12, 4))
            .or_else(|| numbered("param", 32, 32)),
    }
}

/// The reference's operand text, as the values a decoded operand could equal.
///
/// # Why a piece can hold more than one operand
///
/// The reference does not put a comma between every operand. An export prints its target
/// and its first source as `mrt0 v0`, and an interpolation prints an attribute and a
/// channel as one token, `attr3.y`. Taking the first word of each comma-piece - which is
/// what this did, to strip trailing modifiers - silently dropped the rest.
///
/// That was invisible while those families decoded no operands at all: with nothing to
/// compare, nothing could mismatch. Solving their layouts turned the omission into a
/// failure, which is the test doing its job a step later than would have been ideal.
fn normalise(reference: &str) -> Vec<String> {
    let mut out = Vec::new();
    for piece in reference.split(',') {
        for token in piece.split_whitespace() {
            // A leading sign on a *register* is a source modifier - a separate field that
            // negates the value on the way in - and the operand field itself still encodes
            // the plain register number. Stripped only before a register letter: doing it
            // unconditionally would make `-1.0` match `1.0` and hide a real sign error in
            // the inline constants, which is a fault worth keeping loud.
            let token = match token.strip_prefix('-') {
                Some(rest) if rest.starts_with('v') || rest.starts_with('s') => rest,
                _ => token,
            };
            if MODIFIERS.contains(&token) {
                continue;
            }
            // `attr3.y` is two operands: which attribute, and which of its four channels.
            if let Some(rest) = token.strip_prefix("attr") {
                if let Some((number, channel)) = rest.split_once('.') {
                    if let Some(index) = "xyzw".find(channel) {
                        out.push(number.to_owned());
                        out.push(index.to_string());
                        continue;
                    }
                }
            }
            // Both spellings, not one. A name is only an export target *in an export* -
            // `null` is one there and a special register in a vector instruction, and the
            // reference prints it in both. Replacing the token with its code broke every
            // instruction using the register sense, so the code is offered *alongside*
            // rather than instead.
            //
            // This accepts a decoded `9` where the reference said `null`. That is a real
            // loosening and a small one: the alternative is threading the family through
            // here to decide which sense applies, and the decode error it would catch -
            // a field read as a number where a register was meant - is already caught by
            // every other operand of the same instruction.
            if let Some(code) = symbolic_code(token) {
                out.push(code.to_string());
            }
            // A register range names its base: the field encodes where the group starts.
            let token = match (token.find('['), token.find(':')) {
                (Some(open), Some(colon)) if colon > open => {
                    format!("{}{}", &token[..open], &token[open + 1..colon])
                }
                _ => token.to_owned(),
            };
            out.push(token);
        }
    }
    out
}

/// Instructions the decoder knowingly reports no operands for.
///
/// Every entry is a gap with a reason, and the list is asserted to be *exact* - so
/// closing one fails here until it is removed, and opening a new one fails here too.
const NO_OPERANDS_DECODED: &[&str] = &[
    // MIMG. Its first operand is a descriptor living in several consecutive scalar
    // registers rather than a register, and decoding the field without modelling what it
    // points at produces a number that reads like a register and is not one.
    //
    // MTBUF used to be listed here for the same reason and no longer is: the descriptor
    // model arrived with the untyped buffer accesses, and a typed one is the same
    // descriptor with a format conversion on top.
    "image_sample",
    // Structured immediates: one encoded field carrying several independent values.
    // `s_waitcnt` packs three separate counters into sixteen bits and the reference
    // prints whichever are not at their maximum, so the operand text and the encoded
    // field are not in any positional correspondence at all. `s_clause` is the same shape.
    // The solver refuses rather than fitting a field to text it cannot explain.
    "s_clause",
    "s_waitcnt",
];

#[test]
fn an_instruction_that_decodes_no_operands_is_a_listed_gap() {
    // The converse of the test above, and the reason this one exists.
    //
    // `every_decoded_operand_appears_in_the_reference` iterates the operands we produced
    // and looks each up in the reference. Produce none and the loop body never runs, so
    // it passes - vacuously, silently, and most loudly for exactly the instructions that
    // matter most.
    //
    // That is not hypothetical. `v_mov_b32_e32` had no operand row at all: its
    // destination is ambiguous between an eight-bit vector register and a nine-bit
    // source whose top bit is a family constant, so the solver refused it, correctly.
    // Every move in every fixture then decoded with an empty operand list and the whole
    // differential suite stayed green. The most common instruction in the set.
    //
    // So the skip is inventoried rather than silent. A gap has to be written down here,
    // which is cheap, and the cost of not writing it down is a test suite that reports
    // success for an empty answer.
    let table = EncodingTable::builtin().expect("table");
    let operands = operands();
    let mut silent: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();

    for (name, _) in FIXTURES {
        let reference = read_reference(name);
        let decoded = decode(&read_binary(name), &table, &operands);
        for (expected, actual) in reference.iter().zip(&decoded.instructions) {
            // Only instructions the reference printed operands for. One that genuinely
            // takes none - `s_endpgm` - decodes none correctly and is not a gap.
            if expected.operands.trim().is_empty() {
                continue;
            }
            if !actual.operands_decoded || actual.operands.is_empty() {
                silent.insert(expected.mnemonic.clone());
            }
        }
    }

    let listed: std::collections::BTreeSet<String> = NO_OPERANDS_DECODED
        .iter()
        .map(|s| (*s).to_owned())
        .collect();
    assert_eq!(
        silent,
        listed,
        "
the set of instructions decoding no operands has changed.
           newly silent (add a row, or list them with a reason): {:?}
           no longer silent (delete them from NO_OPERANDS_DECODED): {:?}",
        silent.difference(&listed).collect::<Vec<_>>(),
        listed.difference(&silent).collect::<Vec<_>>()
    );
}

#[test]
fn every_decoded_operand_appears_in_the_reference() {
    // The assertion that makes translation possible. An encoding table can be right
    // about *which* instruction a word is and wrong about what it operates on, and
    // that error is silent: a misread inline constant becomes a valid register index,
    // so the resulting shader compiles, runs, and draws the wrong thing.
    let table = EncodingTable::builtin().expect("table");
    let operands = operands();
    let mut checked = 0usize;

    for (name, _) in FIXTURES {
        let reference = read_reference(name);
        let decoded = decode(&read_binary(name), &table, &operands);

        for (expected, actual) in reference.iter().zip(&decoded.instructions) {
            // Families whose operand layout has not been established decode nothing,
            // which is different from an instruction that takes nothing. Only the
            // established ones are checked, so an unfilled family cannot pass by
            // producing an empty list.
            if !actual.operands_decoded {
                continue;
            }
            let printed = normalise(&expected.operands);
            for operand in &actual.operands {
                let rendered = operand.to_string();
                // A 64-bit operand is printed by its pair name - `vcc` for the pair
                // whose low half is `vcc_lo` - exactly as `s[4:5]` is printed for the
                // pair based at `s4`. The decoder reports the register the field
                // encodes, which is the low half. Accepting both is the same collapse
                // as the range normalisation above, for the same reason: operand
                // *width* is a property of the instruction and is not decoded yet.
                // A named modifier at its default is not printed at all: the reference
                // writes `ds_read_b32 v1, v2` where the encoding holds an offset field
                // of zero. So a decoded zero immediate with no counterpart is a display
                // convention rather than a decode that invented an operand.
                //
                // Narrow on purpose - it accepts only *zero*, so a wrongly decoded
                // non-zero immediate still has to appear in the reference.
                let omitted_default = rendered == "0x0";
                assert!(
                    omitted_default
                        || printed.iter().any(|p| {
                            *p == rendered
                                || format!("{p}_lo") == rendered
                                || same_number(p, &rendered)
                        }),
                    "{name}: {} at {:#x} - we decoded operand {rendered}, \
                     the reference printed [{}]",
                    expected.mnemonic,
                    expected.offset,
                    printed.join(" | ")
                );
                checked += 1;
            }
        }
    }

    assert!(
        checked > 20,
        "only {checked} operands checked - the encoding table has lost its layouts"
    );
    println!("{checked} operands verified against the reference");
}
