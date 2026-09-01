//! Proving the dispatch runner against a shader whose answer is known.
//!
//! This runs before the runner is trusted with anything translated. If a shader that
//! writes a constant does not produce that constant, the fault is in the harness, and
//! establishing that cheaply is what stops a harness bug being read as a translator
//! bug later.
//!
//! # A missing device skips loudly
//!
//! Rust has no first-class skip, so the honest default is easy to get wrong: a test
//! that finds no device, returns early and reports `ok` makes the suite green on a
//! machine where the most important test never ran.
//!
//! These print an unmissable line instead, and `bin/orbistoun check` surfaces it. That
//! is the same rule obSCEne's harness follows - an absence that looks identical to a
//! success is worse than no test at all.

use orbistoun_gpu_vulkan::{Availability, dispatch, probe};

/// Prints a skip in a form that survives being scrolled past.
///
/// Returns whether the caller should continue.
fn device_or_skip(test: &str) -> bool {
    match probe() {
        Availability::Available { properties } => {
            // The properties are printed, not just the name. A device that flushes
            // subnormals runs a different program from one that does not, and a suite
            // that reports only "it ran" cannot tell those two runs apart afterwards.
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

#[test]
fn a_shader_that_writes_a_constant_produces_that_constant() {
    // The whole chain: build a module, create a device, bind a buffer, dispatch, read
    // back. Every part of it has to work for this value to appear, which is what makes
    // it worth running first.
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

#[test]
fn the_buffer_is_zeroed_before_the_shader_runs() {
    // Elements the shader never touches must read as zero, not as whatever previously
    // occupied that memory. Without this, a shader that writes nothing at all could
    // appear to have written something - and a translator emitting a shader that does
    // nothing is exactly the failure worth catching.
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

#[test]
fn a_malformed_module_is_rejected_rather_than_run() {
    // The driver validates what it is given, and a runner that accepted nonsense would
    // let a broken translator look like a working one with strange output.
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

#[test]
fn a_module_that_came_from_a_command_stream_runs() {
    // The last gap in the path. Everything before this asserts that a submission
    // *produces* a module; nothing asserted that the module produced this way is one a
    // device will accept. Those are different claims, and the difference is exactly
    // where the two driver faults earlier in this subsystem lived.
    //
    // Nothing here calls the translator. The shader is found because a command stream
    // named its address, which is the only way one is ever found in a real frame.
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

    let mut stream: Vec<u32> = Vec::new();
    for (register, value) in [
        (low, u32::try_from(ADDRESS & 0xFFFF_FFFF).expect("low")),
        (high, u32::try_from(ADDRESS >> 32).expect("high")),
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
    // No registrations here on purpose: this is the path that finds a shader from the
    // packets alone, which is what has to work when the guest hand-rolls a buffer.
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

    // The observation window the translator writes registers into: the vector file then
    // the scalar file.
    let observed = (orbistoun_translate::OBSERVED_REGISTERS * 2) as usize;
    let output = dispatch(module, observed, 64, [1, 1, 1]).expect("dispatch");
    assert_eq!(
        output.observed[0], 9,
        "the shader the command stream named should have written v0; observed {:?}",
        output.observed
    );
}
