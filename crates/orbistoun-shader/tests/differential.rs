//! Differential test: our decoder against a reference disassembler.
//!
//! Unit tests use streams generated from the encoding table, so a wrong row passes them.
//! These fixtures were compiled by LLVM from source in `tools/shader-fixtures/`, and LLVM's
//! disassembler reported where each instruction begins. Offsets are the assertion: each is
//! the sum of every length before it, so the first disagreement names the instruction whose
//! encoding is wrong. The reference detects errors; corrections come from the published
//! specification (D085). Fixtures are committed; regenerate with
//! `tools/shader-fixtures/generate.sh`.

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
                // Everything left on the line; operands contain spaces.
                operands: parts.collect::<Vec<_>>().join(" "),
            }
        })
        // Stop at the padding the compiler writes past the end of a shader (`arith` is
        // nineteen instructions and forty-eight `s_code_end`). The decoder stops there, so
        // the comparison does too; the fixture keeps the full compiler output.
        .take_while(|entry: &Reference| entry.mnemonic != PADDING)
        .collect()
}

/// The instruction compilers pad past the end of a shader with.
///
/// An illegal instruction, so a prefetch running off the end faults instead of executing
/// what follows in memory.
const PADDING: &str = "s_code_end";

fn read_binary(name: &str) -> Vec<u8> {
    let path = fixtures_dir().join(format!("{name}.gcn"));
    std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// Encoding families a mnemonic prefix is allowed to belong to.
///
/// Catches two families with swapped identifying bits that share a width, which offsets
/// alone would not.
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
        // An unrecognised prefix means this check has nothing to say; offsets still apply.
        &[]
    }
}

/// The fixtures, and what each was chosen to exercise.
const FIXTURES: &[(&str, &str)] = &[
    ("arith", "vector ALU, the bulk of any real shader"),
    ("minmax", "v_min_f32/v_max_f32, the saturating VOP2 pair"),
    ("unary", "unary vector float ALU and transcendentals"),
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
    (
        "sampling",
        "the image opcodes a compiled fixture never reaches, `image_sample_lz` above all",
    ),
    (
        "primitive",
        "the NGG vertex program's prefetch, nop, send-message, shift and carry-add",
    ),
    (
        "texture",
        "the textured pixel shader's whole-quad mode, branches, compares and conversions",
    ),
];

/// Every fixture on disk is read by this suite.
#[test]
fn every_fixture_on_disk_is_in_the_list() {
    // The list above is written by hand; a fixture missing from it would be silently
    // skipped. Generated fixtures use their own extension, distinct from guest dumps.
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

/// Every instruction boundary and length matches the reference.
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

        // Checked first: a desynchronised decode explains any number of downstream
        // mismatches.
        assert!(
            decoded.is_trustworthy(),
            concat!(
                "{} ({}): decode was not trustworthy - ",
                "desynchronised={}, overran={}, trailing={}"
            ),
            name,
            exercises,
            decoded.desynchronised,
            decoded.overran,
            decoded.trailing_bytes
        );

        for (index, expected) in reference.iter().enumerate() {
            let actual = decoded.instructions.get(index).unwrap_or_else(|| {
                panic!(
                    concat!(
                        "{}: reference has {} instructions, we decoded {} - ",
                        "first missing is {} at {:#x}"
                    ),
                    name,
                    reference.len(),
                    decoded.instructions.len(),
                    expected.mnemonic,
                    expected.offset
                )
            });

            assert_eq!(
                actual.offset, expected.offset,
                concat!(
                    "{}: instruction {} ({}) should start at {:#x} but we put it ",
                    "at {:#x}. Every offset is the sum of the lengths before it, so the ",
                    "encoding for the *previous* instruction has the wrong length."
                ),
                name, index, expected.mnemonic, expected.offset, actual.offset
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

/// No instruction in real compiler output is unrecognised.
#[test]
fn no_instruction_in_real_compiler_output_is_unrecognised() {
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

/// Each instruction decodes into an encoding family its mnemonic permits.
#[test]
fn instructions_are_classified_into_the_right_encoding_family() {
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

/// A worklist over the whole fixture corpus renders a named top blocker.
#[test]
fn a_worklist_over_the_whole_fixture_corpus_reads_sensibly() {
    // The only test running decode, coverage, ranking and rendering together on material
    // not generated from the table.
    use orbistoun_shader::{CorpusCoverage, MnemonicTable, report};

    let table = EncodingTable::builtin().expect("table");
    let mnemonics = MnemonicTable::builtin().expect("mnemonics");
    let mut coverage = CorpusCoverage::new();

    // Nothing is supported, so every instruction is a blocker.
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
        // Derived from the list. No translator ran, so the line says the count is the
        // opcode-level bound.
        rendered.contains(&format!("0 of {}", FIXTURES.len())),
        "nothing is supported, so no shader is complete:
{rendered}"
    );
    assert!(
        rendered.contains("no translation was attempted"),
        "the count is a bound, and the report has to say so:
{rendered}"
    );
    assert!(
        !rendered.contains("suspect"),
        "every fixture should decode cleanly:\n{rendered}"
    );
    // The top blocker carries a name. Anchored on the table header, since the summary
    // line also contains "shaders".
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

/// Whether two operand texts are the same number written differently.
///
/// The reference prints a memory offset in hex and a branch offset in decimal; this decoder
/// prints every immediate in hex.
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
/// The probe solver skips the same words, which keeps a solved layout comparable to a
/// printed one.
const MODIFIERS: &[&str] = &[
    "off", "glc", "slc", "dlc", "lds", "gds", "offen", "idxen", "tfe", "nv", "done", "compr", "vm",
    "unorm",
];

/// An operand the reference spells as a name, and the code it stands for.
///
/// `mrt0` and `p10` are spellings, while the decoder reports the field's value. The codes
/// are measured by `derive_symbolic_codes` in `orbistoun-gen operands`, which holds an
/// instruction constant, varies the name and reads the bits that moved. This test checks the
/// decoder against the reference given that mapping; it does not establish the mapping.
fn symbolic_code(token: &str) -> Option<u32> {
    let numbered = |prefix: &str, base: u32, count: u32| -> Option<u32> {
        let index: u32 = token.strip_prefix(prefix)?.parse().ok()?;
        (index < count).then_some(base + index)
    };
    match token {
        "mrtz" => Some(8),
        // Two unrelated nines: the export target `null`, and the geometry-engine allocation
        // request; the assembler encodes `s_sendmsg sendmsg(MSG_GS_ALLOC_REQ)` as 0xbf900009,
        // the word in `primitive.s`. The shared arm is the lint's doing.
        "null" | "sendmsg(MSG_GS_ALLOC_REQ)" => Some(9),
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
/// The reference does not put a comma between every operand: an export prints `mrt0 v0`,
/// and an interpolation prints attribute and channel as one token, `attr3.y`. A register
/// range collapses to its base register, since the operand field encodes the base and the
/// span is a property of the instruction.
fn normalise(reference: &str) -> Vec<String> {
    let mut out = Vec::new();
    for piece in reference.split(',') {
        for token in piece.split_whitespace() {
            // A leading sign on a register is a source modifier, a separate field; the
            // operand field holds the plain register. Stripped only before a register
            // letter, so `-1.0` never matches `1.0`.
            let token = match token.strip_prefix('-') {
                Some(rest) if rest.starts_with('v') || rest.starts_with('s') => rest,
                _ => token,
            };
            // `off` is the reference's spelling of a flat access with no scalar base; the
            // field holds the code our operand table names `null`. Offered alongside, and
            // the token is dropped as a modifier.
            if token == "off" {
                out.push("null".to_owned());
                continue;
            }
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
            // Both spellings: `null` is an export target in an export and a special register
            // in a vector instruction. Accepting a decoded `9` for `null` is a small
            // loosening; other operands of the same instruction catch a field misread as a
            // number.
            if let Some(code) = symbolic_code(token) {
                out.push(code.to_string());
            }
            // `dmask:0xf` and the like: a field value printed with the field's name. The
            // probe solver splits these the same way.
            if let Some((name, value)) = token.split_once(':')
                && !name.is_empty()
                && value.starts_with(|c: char| c.is_ascii_digit())
            {
                out.push(value.to_owned());
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
/// Every entry is a gap with a reason, and the list is asserted exact, so closing or
/// opening a gap fails until the list is updated.
const NO_OPERANDS_DECODED: &[&str] = &[
    // Structured immediates: one field carrying several values. `s_waitcnt` packs three
    // counters into sixteen bits and the reference prints only those not at maximum, so
    // text and field have no positional correspondence; `s_clause` is the same shape. The
    // solver refuses rather than fit a field to text it cannot explain.
    "s_clause",
    "s_waitcnt",
];

/// Every instruction that decodes no operands is listed in `NO_OPERANDS_DECODED`.
#[test]
fn an_instruction_that_decodes_no_operands_is_a_listed_gap() {
    // `every_decoded_operand_appears_in_the_reference` passes vacuously for an instruction
    // that decodes no operands, so such gaps are inventoried here.
    let table = EncodingTable::builtin().expect("table");
    let operands = operands();
    let mut silent: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();

    for (name, _) in FIXTURES {
        let reference = read_reference(name);
        let decoded = decode(&read_binary(name), &table, &operands);
        for (expected, actual) in reference.iter().zip(&decoded.instructions) {
            // Only instructions the reference printed operands for; `s_endpgm` takes none.
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

/// Every decoded operand appears in the reference's operand text.
#[test]
fn every_decoded_operand_appears_in_the_reference() {
    // A misread inline constant becomes a valid register index, so a wrong operand
    // layout yields a shader that compiles and draws the wrong thing.
    let table = EncodingTable::builtin().expect("table");
    let operands = operands();
    let mut checked = 0usize;

    for (name, _) in FIXTURES {
        let reference = read_reference(name);
        let decoded = decode(&read_binary(name), &table, &operands);

        for (expected, actual) in reference.iter().zip(&decoded.instructions) {
            // Families with no established layout decode nothing and are skipped; an
            // established layout must account for every operand.
            if !actual.operands_decoded {
                continue;
            }
            let printed = normalise(&expected.operands);
            for operand in &actual.operands {
                let rendered = operand.to_string();
                // A 64-bit operand is printed by its pair name (`vcc` for the pair based at
                // `vcc_lo`), while the decoder reports the low register. A named modifier at
                // its default is not printed, so a decoded zero immediate with no
                // counterpart is accepted; any non-zero immediate must appear.
                let omitted_default = rendered == "0x0";
                assert!(
                    omitted_default
                        || printed.iter().any(|p| {
                            *p == rendered
                                || format!("{p}_lo") == rendered
                                || same_number(p, &rendered)
                        }),
                    concat!(
                        "{}: {} at {:#x} - we decoded operand {}, ",
                        "the reference printed [{}]"
                    ),
                    name,
                    expected.mnemonic,
                    expected.offset,
                    rendered,
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
