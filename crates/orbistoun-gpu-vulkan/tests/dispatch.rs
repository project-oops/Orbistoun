//! The dispatch runner, proved against a shader whose answer is known.
//!
//! If a shader that writes a constant does not produce it, the fault is in the harness, not the
//! translator. A missing device skips with a line `bin/orbistoun check` surfaces, because a test
//! that returns early and reports `ok` makes the suite green where it never ran.

use orbistoun_gpu_vulkan::{Availability, dispatch, probe};

/// Prints a skip in a form that survives being scrolled past. Returns whether the caller should
/// continue.
fn device_or_skip(test: &str) -> bool {
    match probe() {
        Availability::Available { properties } => {
            // The properties are printed, not just the name: a device that flushes subnormals runs
            // a different program.
            println!(
                "[{test}] device: {} (subgroup {}, subnormals {})",
                properties.device,
                properties.subgroup_size,
                if properties.subnormals_preserved {
                    "preserved"
                } else {
                    "flushed"
                }
            );
            true
        }
        Availability::Unavailable { reason } => {
            println!();
            println!("!! SKIPPED: {test}");
            println!("!! no Vulkan device: {reason}");
            println!("!! this test did NOT run - install a driver, or a software");
            println!("!! implementation, before reading this suite as green");
            println!();
            false
        }
    }
}

/// A shader that writes a constant produces that constant.
#[test]
fn a_shader_that_writes_a_constant_produces_that_constant() {
    // Every part of the chain (module, device, buffer, dispatch, readback) has to work for this
    // value to appear.
    const VALUE: u32 = 0xABCD_1234;
    const WORDS: usize = 4;

    if !device_or_skip("a_shader_that_writes_a_constant_produces_that_constant") {
        return;
    }

    let module = orbistoun_spirv::storage_buffer_write_module(VALUE, WORDS as u32);
    let result = dispatch(&module, WORDS, WORDS, [1, 1, 1])
        .expect("dispatch")
        .observed;

    assert_eq!(
        result[0], VALUE,
        "the shader wrote {VALUE:#x} to element zero; the buffer came back as {result:#x?}"
    );
}

/// The buffer is zeroed before the shader runs.
#[test]
fn the_buffer_is_zeroed_before_the_shader_runs() {
    // Elements the shader never touches read as zero, so a shader that writes nothing cannot appear
    // to have written something.
    const WORDS: usize = 4;

    if !device_or_skip("the_buffer_is_zeroed_before_the_shader_runs") {
        return;
    }

    let module = orbistoun_spirv::storage_buffer_write_module(0xFFFF_FFFF, WORDS as u32);
    let result = dispatch(&module, WORDS, WORDS, [1, 1, 1])
        .expect("dispatch")
        .observed;

    assert_eq!(
        &result[1..],
        &[0, 0, 0],
        "untouched elements must be zero, got {result:#x?}"
    );
}

/// A malformed module is rejected rather than run.
#[test]
fn a_malformed_module_is_rejected_rather_than_run() {
    // A runner that accepted nonsense would make a broken translator look like a working one.
    if !device_or_skip("a_malformed_module_is_rejected_rather_than_run") {
        return;
    }

    // Right magic word, and then nothing that parses.
    let rubbish = vec![0x0723_0203, 0x0001_0300, 0, 1, 0, 0xDEAD_BEEF, 0xDEAD_BEEF];
    assert!(
        dispatch(&rubbish, 4, 4, [1, 1, 1]).is_err(),
        "a malformed module must be refused"
    );
}

/// Guest memory as one mapping, for the end-to-end test below.
struct Mapping {
    base: u64,
    bytes: Vec<u8>,
}

impl orbistoun_gpu::pipeline::GuestMemory for Mapping {
    fn read(&self, address: u64, length: usize) -> Option<&[u8]> {
        let offset = usize::try_from(address.checked_sub(self.base)?).ok()?;
        self.bytes.get(offset..offset.checked_add(length)?)
    }
}

/// Where the fixture puts its shader. Arbitrary, and deliberately not zero.
const ADDRESS: u64 = 0x1_0000;

/// A module found through a command stream runs on a device.
#[test]
fn a_module_that_came_from_a_command_stream_runs() {
    // A submission produces a module; this asserts a device accepts it. The shader is found because
    // a command stream named its address, as in a real frame.
    use orbistoun_gpu::pipeline::Pipeline;
    use orbistoun_translate::{Fidelity, Strategy, Width};

    if !device_or_skip("a_module_that_came_from_a_command_stream_runs") {
        return;
    }

    // v_mov_b32 v0, 9 ; s_endpgm, then rubbish that is not part of the shader.
    let words: [u32; 2] = [0x7E00_0000 | (1 << 9) | (128 + 9), 0xBF81_0000];
    let mut bytes: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
    bytes.extend(std::iter::repeat_n(0xABu8, 1024));
    let memory = Mapping {
        base: ADDRESS,
        bytes,
    };

    let vocabulary = orbistoun_gpu::registers::Vocabulary::builtin().expect("vocabulary");
    let (low, high) = vocabulary
        .shader_registers()
        .fold((0, 0), |acc, (r, (s, hi))| {
            if s == "compute" {
                if *hi { (acc.0, *r) } else { (*r, acc.1) }
            } else {
                acc
            }
        });
    let (opcode, base) = vocabulary
        .opcode_for_register(low)
        .expect("an opcode reaching the shader registers");

    // The registers hold the address in 256-byte units, low word then high, as the GL cube capture
    // shows (orbistoun-gpu `tests/captures/`).
    let mut stream: Vec<u32> = Vec::new();
    for (register, value) in [
        (
            low,
            u32::try_from((ADDRESS >> 8) & 0xFFFF_FFFF).expect("low"),
        ),
        (high, u32::try_from(ADDRESS >> 40).expect("high")),
    ] {
        stream.push((3 << 30) | (1 << 16) | (u32::from(opcode) << 8));
        stream.push(register - base);
        stream.push(value);
    }
    let stream: Vec<u8> = stream.iter().flat_map(|w| w.to_le_bytes()).collect();

    let mut pipeline = Pipeline::new(Strategy::Predicated {
        fidelity: Fidelity::Lane,
        width: Width::default(),
    })
    .expect("pipeline");
    // No registrations: this is the path that finds a shader from the packets alone.
    let submission = pipeline.submit(
        &stream,
        orbistoun_gpu::pipeline::Queue::Compute,
        &[],
        &memory,
    );
    assert_eq!(
        submission.report.failures,
        Vec::new(),
        "{:?}",
        submission.report
    );

    let module = submission
        .modules
        .values()
        .next()
        .expect("a module for the backend");

    // The observation window the translator writes registers into: the vector file, then the scalar
    // file.
    let observed = (orbistoun_translate::OBSERVED_REGISTERS * 2) as usize;
    let output = dispatch(module, observed, 64, [1, 1, 1]).expect("dispatch");
    assert_eq!(
        output.observed[0], 9,
        "the shader the command stream named should have written v0; observed {:?}",
        output.observed
    );
}
