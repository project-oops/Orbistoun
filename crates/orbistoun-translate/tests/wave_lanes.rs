//! Guest compute programs read from files, translated as a dispatch's are both ways - the whole wave
//! in one invocation, and one invocation for each lane or pair of lanes - to measure what each costs
//! in module words and to say what of its wave a program needs.
//!
//! `ORBISTOUN_LANE_PROGRAMS` lists `path|user words|group id components|thread id components|threads`
//! entries separated by `;`, each file the program's bytes from its first instruction, `threads`
//! the group's threads along x (y and z one). Nothing runs without it: guest bytes are not part of
//! the tree.

use orbistoun_shader::{EncodingTable, OperandTable, decode_program};
use orbistoun_translate::wavefront::{
    BufferFormats, ComputeInputs, MeshPrimitive, Stage, UserData, Window,
};
use orbistoun_translate::{Fidelity, Strategy, Width, lane_exact, translate_with_user_data};

/// The subgroup the measured device runs on.
const SUBGROUP: u32 = 32;

/// Each listed program's needs, and its words translated whole and per lane.
#[test]
fn listed_programs_translate_both_ways() {
    let Ok(listed) = std::env::var("ORBISTOUN_LANE_PROGRAMS") else {
        return;
    };
    let encodings = EncodingTable::builtin().expect("encodings");
    let operands = OperandTable::builtin().expect("operands");
    let mut refused = Vec::new();
    for entry in listed.split(';').filter(|entry| !entry.is_empty()) {
        let fields: Vec<&str> = entry.split('|').collect();
        let [path, count, ids, components, threads] = fields[..] else {
            panic!("an entry is path|user words|ids|components|threads: {entry}");
        };
        let number = |text: &str| text.parse::<u32>().expect("a number");
        let bytes = std::fs::read(path).expect("the program's bytes");
        let decoded = decode_program(&bytes, &encodings, &operands);
        println!("{path}: {} instructions", decoded.instructions.len());
        let needs = lane_exact::lane_needs(&decoded, &encodings, 64);
        for (need, offset, name) in &needs.needs {
            println!("  needs {need}: first {name} at {offset:#x}");
        }
        let ids = number(ids);
        let mut formats = BufferFormats::default();
        for slot in 0..64 {
            formats.set(slot, Some(0x3104_dfac));
        }
        let user_data = |per_invocation: Option<u32>| UserData {
            count: number(count),
            compute: Some(ComputeInputs {
                workgroup_ids: [ids > 0, ids > 1, ids > 2],
                thread_id_components: number(components),
                threads: [number(threads), 1, 1],
                device_addressed: true,
                ..ComputeInputs::default()
            }),
            draw_buffers: true,
            local_words: Some(16384),
            buffer_formats: Some(formats),
            dx10_clamp: Some(true),
            per_invocation,
            program_address: std::path::Path::new(path)
                .file_stem()
                .and_then(|stem| stem.to_str())
                .and_then(|stem| stem.strip_prefix("mem-"))
                .and_then(|hex| u64::from_str_radix(hex, 16).ok()),
            ..UserData::default()
        };
        for (model, per_invocation) in [("whole wave", None), ("per lane", Some(SUBGROUP))] {
            let translated = translate_with_user_data(
                &decoded,
                &encodings,
                Strategy::Predicated {
                    fidelity: Fidelity::Auto,
                    width: Width::Wave64,
                },
                (Stage::Compute, MeshPrimitive::default()),
                Window::spanning_address(0x7400_0000_0000, 64).expect("a window"),
                user_data(per_invocation),
            );
            match translated {
                Ok(translated) => println!(
                    "  {model}: {} words, warnings {:?}",
                    translated.module.len(),
                    translated.warnings
                ),
                Err(why) => {
                    println!("  {model}: refused: {why}");
                    refused.push(path);
                }
            }
        }
    }
    assert!(refused.is_empty(), "refused: {refused:?}");
}

/// What a captured dispatch's program was translated against, as its capture keeps it.
#[derive(serde::Deserialize)]
struct Context {
    strategy: Strategy,
    stage: Stage,
    primitive: MeshPrimitive,
    window: Window,
    user_data: UserData,
}

/// A dispatch captured by a run (`replay-dispatch`'s directory), its program translated again both
/// ways and each module replayed over the capture's inputs: its words, each run's device time and
/// a hash of what it left, which the two ways must agree on.
///
/// `ORBISTOUN_LANE_CAPTURE` names a copy of the capture's directory, whose module and groups this
/// rewrites; `ORBISTOUN_LANE_GROUPS` (`x,y,z`) dispatches fewer groups than it was captured with,
/// and `ORBISTOUN_LANE_MODELS` (`whole`, `lane` or both, comma-separated) which ways run.
#[test]
fn a_captured_dispatch_replays_both_ways() {
    let Ok(dir) = std::env::var("ORBISTOUN_LANE_CAPTURE") else {
        return;
    };
    let dir = std::path::Path::new(&dir);
    let encodings = EncodingTable::builtin().expect("encodings");
    let operands = OperandTable::builtin().expect("operands");
    let text = std::fs::read_to_string(dir.join("translation.json")).expect("the context");
    let context: Context = serde_json::from_str(&text).expect("a context");
    let bytes = std::fs::read(dir.join("program.bin")).expect("the program");
    let decoded = decode_program(&bytes, &encodings, &operands);
    let manifest = dir.join("dispatch.txt");
    if let Ok(groups) = std::env::var("ORBISTOUN_LANE_GROUPS") {
        let groups = groups.replace(',', " ");
        let rewritten: Vec<String> = std::fs::read_to_string(&manifest)
            .expect("the manifest")
            .lines()
            .map(|line| {
                if line.starts_with("groups ") {
                    format!("groups {groups}")
                } else {
                    line.to_owned()
                }
            })
            .collect();
        std::fs::write(&manifest, rewritten.join("\n") + "\n").expect("the manifest written");
    }
    let models = std::env::var("ORBISTOUN_LANE_MODELS").unwrap_or_else(|_| "whole,lane".into());
    let mut outputs = Vec::new();
    for model in models.split(',') {
        let mut user_data = context.user_data;
        if model == "whole" {
            user_data.per_invocation = None;
        }
        let translated = translate_with_user_data(
            &decoded,
            &encodings,
            context.strategy,
            (context.stage, context.primitive),
            context.window,
            user_data,
        )
        .expect("the program translates");
        println!(
            "{model}: {} words, warnings {:?}",
            translated.module.len(),
            translated.warnings
        );
        let module: Vec<u8> = translated
            .module
            .iter()
            .flat_map(|word| word.to_le_bytes())
            .collect();
        std::fs::write(dir.join("module.spv"), module).expect("the module written");
        let replay =
            orbistoun_gpu_vulkan::replay_dispatch(dir, 3, None).expect("the capture replays");
        print!("{model}: {replay}");
        assert!(!replay.lost, "{model}: the dispatch lost the device");
        let hashes: Vec<u64> = replay
            .runs
            .iter()
            .filter_map(|run| run.outcome.as_ref().ok().map(|out| out.hash))
            .collect();
        outputs.push((model.to_owned(), hashes));
    }
    if let [(_, first), (_, second)] = &outputs[..] {
        assert_eq!(
            first.last(),
            second.last(),
            "both ways leave the same output"
        );
    }
}
