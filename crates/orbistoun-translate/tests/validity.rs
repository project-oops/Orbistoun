//! Every module the translator makes is valid SPIR-V for a Vulkan 1.3 device, as `spirv-val`
//! decides it.
//!
//! A device test shows a module runs on one driver, which is not validity: a driver may accept an
//! invalid module, miscompile it or fault on it, and another driver may refuse what the first ran.
//! Here each shape a title's programs were found to make invalid is translated and validated, and
//! so is a program of each instruction the translator names, at each stage. Without a validator
//! every test says it skipped (`ORBISTOUN_SPIRV_VAL` or `VULKAN_SDK` names one); the shader census
//! (`orbistoun-cli shader-census --validate`) asks the same of every program a title keeps.

use orbistoun_shader::{EncodingTable, OperandTable, decode};
use orbistoun_spirv::validator::{self, Validation};
use orbistoun_translate::wavefront::{ComputeInputs, MeshPrimitive, Stage, UserData, Window};
use orbistoun_translate::{Fidelity, Strategy, Width, translate_with_user_data};

/// `s_endpgm`.
const END: u32 = 0xbf81_0000;

fn tables() -> &'static (EncodingTable, OperandTable) {
    static TABLES: std::sync::OnceLock<(EncodingTable, OperandTable)> = std::sync::OnceLock::new();
    TABLES.get_or_init(|| {
        (
            EncodingTable::builtin().expect("encodings"),
            OperandTable::builtin().expect("operands"),
        )
    })
}

/// Whether a validator is here to ask; says so loudly where it is not, since a harness hides a
/// passing test's output.
fn validator_or_skip(what: &str) -> bool {
    if validator::available() {
        return true;
    }
    println!(
        "[{what}] SKIPPED - no spirv-val at {}: set {} or VULKAN_SDK",
        validator::validator().display(),
        validator::VALIDATOR_VARIABLE
    );
    false
}

/// `program` translated for `stage` at `strategy` with `user_data`.
fn translated(
    program: &[u32],
    strategy: Strategy,
    stage: Stage,
    user_data: &UserData,
) -> Result<orbistoun_translate::Translated, orbistoun_translate::TranslateError> {
    let (encodings, operands) = tables();
    let bytes: Vec<u8> = program.iter().flat_map(|word| word.to_le_bytes()).collect();
    let decoded = decode(&bytes, encodings, operands);
    translate_with_user_data(
        &decoded,
        encodings,
        strategy,
        (stage, MeshPrimitive::Triangles),
        Window::default(),
        *user_data,
    )
}

/// Panics with the validator's message where `module` is invalid.
fn assert_valid(what: &str, module: &[u32]) {
    match validator::validate(module) {
        Validation::Valid => {}
        Validation::Invalid(message) => panic!("{what}: spirv-val refuses the module:\n{message}"),
        Validation::Unavailable(reason) => panic!("{what}: the validator did not run: {reason}"),
    }
}

/// The strategy a title's dispatches and draws are translated at: whichever fidelity the program
/// needs, over a 64-lane wave.
const AUTO: Strategy = Strategy::Predicated {
    fidelity: Fidelity::Auto,
    width: Width::Wave64,
};

/// A guest dispatch's user data: seventeen words, a one-wave group with two thread-id components.
fn dispatch() -> UserData {
    UserData {
        count: 17,
        compute: Some(ComputeInputs {
            workgroup_ids: [false, false, false],
            table_buffers: 0,
            loop_cap: 0,
            thread_id_components: 2,
            threads: [8, 8, 1],
            ..ComputeInputs::default()
        }),
        ..UserData::default()
    }
}

/// A fetch from a 2D array answers its texel where the array has the layer and zero past its last
/// (D897), choosing between two four-component vectors. Below SPIR-V 1.4 that choice takes a
/// condition as wide as the vectors, and the result is the texel's type, not a word: Grand Theft
/// Auto V's compute copies (runtime program `0x74005b7e8600` among them) made a module typed the
/// choice as a word, which `spirv-val` refuses.
#[test]
fn a_fetch_from_a_layer_of_an_array_validates() {
    if !validator_or_skip("a layered fetch") {
        return;
    }
    // v_mov_b32 v2, s16 (the layer); image_load v[4:7], v[0:2], s[0:7] dmask:0xf dim:2D_ARRAY;
    // s_waitcnt; image_store v[4:7], v[0:1], s[8:15] dmask:0xf dim:2D; s_endpgm.
    let program = [
        0x7e04_0210,
        0xf000_1f28,
        0x0000_0400,
        0xbf8c_0000,
        0xf020_1f08,
        0x0002_0400,
        END,
    ];
    for per_invocation in [None, Some(64)] {
        let made = translated(
            &program,
            AUTO,
            Stage::Compute,
            &UserData {
                per_invocation,
                ..dispatch()
            },
        )
        .expect("an array fetch translates");
        assert!(made.textures.first().is_some_and(|texture| texture.layered));
        assert_valid(
            &format!("a layered fetch, per invocation {per_invocation:?}"),
            &made.module,
        );
    }
}

/// A pixel shader that reads its quad's other lanes finds its place in the quad from the
/// subgroup index, an integer input; Vulkan requires a fragment entry point's integer inputs to be
/// flat, built-ins among them (`VUID-StandaloneSpirv-Flat-04744`). Grand Theft Auto V's pixel
/// shaders that permute within their quad made modules without it.
#[test]
fn a_pixel_shader_reading_its_quad_validates() {
    if !validator_or_skip("a quad read") {
        return;
    }
    // v_mov_b32_dpp v1, v0 quad_perm:[1,0,3,2] row_mask:0xf bank_mask:0xf; s_endpgm.
    let program = [0x7e02_02fa, 0xff00_b100, END];
    for fidelity in [Fidelity::Auto, Fidelity::Lane, Fidelity::Wavefront] {
        let made = translated(
            &program,
            Strategy::Predicated {
                fidelity,
                width: Width::Wave64,
            },
            Stage::Fragment,
            &UserData::default(),
        )
        .expect("a quad permute translates for a pixel shader");
        assert_valid(&format!("a quad read at {fidelity:?}"), &made.module);
    }
}

/// From SPIR-V 1.4, which a mesh module is, the entry point names every global variable it
/// reaches, and a translation declares an image only when the program first reads one - after the
/// entry point is written. Grand Theft Auto V's primitive shaders that sample made modules whose
/// image was missing from the interface.
#[test]
fn a_primitive_shader_reading_an_image_validates() {
    if !validator_or_skip("a primitive shader's image") {
        return;
    }
    // v_mov_b32 v10, 0; v_mov_b32 v11, 0; image_load v[4:7], v[10:11], s[0:7] dmask:0xf dim:2D;
    // s_waitcnt; s_endpgm. The address is written first: a primitive shader's entry registers are
    // the geometry engine's inputs.
    let program = [
        0x7e14_0280,
        0x7e16_0280,
        0xf000_1f08,
        0x0000_040a,
        0xbf8c_0000,
        END,
    ];
    for per_invocation in [None, Some(64)] {
        let made = translated(
            &program,
            AUTO,
            Stage::Mesh,
            &UserData {
                count: 4,
                per_invocation,
                ..UserData::default()
            },
        )
        .expect("an image read translates for a primitive shader");
        assert!(!made.textures.is_empty(), "the program reads an image");
        assert_valid(
            &format!("a primitive shader's image, per invocation {per_invocation:?}"),
            &made.module,
        );
    }
}

/// Every instruction this target has a name for, as words that decode back to it, followed by
/// `s_endpgm`. Operand fields are zero.
fn known_encodings() -> Vec<(String, Vec<u32>)> {
    let (encodings, _) = tables();
    let mut out = Vec::new();
    for (family, opcode, name) in encodings.names() {
        // The decoder stops at padding, so it never reaches the translator.
        if name == "s_code_end" || name == "s_endpgm" {
            continue;
        }
        let Some(encoding) = encodings.encodings().iter().find(|e| e.name == family) else {
            continue;
        };
        let low = opcode & ((1 << encoding.opcode.width) - 1);
        let mut words = vec![encoding.value | (low << encoding.opcode.shift)];
        words.resize((encoding.width_bytes / 4) as usize, 0);
        if let Some(rest) = &encoding.opcode_extension
            && let Some(word) = words.get_mut(rest.word)
        {
            *word |= (opcode >> encoding.opcode.width) << rest.shift;
        }
        words.push(END);
        out.push((name.to_owned(), words));
    }
    out
}

/// Each instruction the translator accepts, alone, makes a valid module at each stage: a dispatch,
/// a pixel shader and a primitive shader, the wave whole or one invocation a lane where the stage
/// has that model. What one instruction does to a module's types and interface is then checked
/// for every instruction, not only those a title happened to reach.
#[test]
fn every_instruction_translates_to_a_valid_module_at_every_stage() {
    if !validator_or_skip("every instruction") {
        return;
    }
    let cases: Vec<(String, Stage, Option<u32>, Vec<u32>)> = known_encodings()
        .into_iter()
        .flat_map(|(name, words)| {
            [
                (Stage::Compute, None),
                (Stage::Compute, Some(64)),
                (Stage::Fragment, None),
                (Stage::Mesh, None),
            ]
            .map(|(stage, per_invocation)| (name.clone(), stage, per_invocation, words.clone()))
        })
        .collect();
    let next = std::sync::atomic::AtomicUsize::new(0);
    let validated = std::sync::atomic::AtomicUsize::new(0);
    let failures = std::sync::Mutex::new(Vec::new());
    std::thread::scope(|scope| {
        for _ in 0..4 {
            scope.spawn(|| {
                while let Some((name, stage, per_invocation, words)) =
                    cases.get(next.fetch_add(1, std::sync::atomic::Ordering::Relaxed))
                {
                    let user_data = match stage {
                        Stage::Compute => UserData {
                            per_invocation: *per_invocation,
                            ..dispatch()
                        },
                        _ => UserData {
                            count: 4,
                            ..UserData::default()
                        },
                    };
                    let Ok(made) = translated(words, AUTO, *stage, &user_data) else {
                        continue;
                    };
                    validated.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    if let Validation::Invalid(message) | Validation::Unavailable(message) =
                        validator::validate(&made.module)
                    {
                        let first = message.lines().take(2).collect::<Vec<_>>().join(" / ");
                        failures
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner)
                            .push(format!("{name} ({stage:?}, {per_invocation:?}): {first}"));
                    }
                }
            });
        }
    });
    let mut failures = failures
        .into_inner()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    failures.sort();
    let validated = validated.into_inner();
    println!("{validated} modules validated");
    assert!(validated > 100, "only {validated} instructions translated");
    assert!(
        failures.is_empty(),
        "{} of {validated} modules are invalid:\n{}",
        failures.len(),
        failures.join("\n")
    );
}
