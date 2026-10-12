//! PPSA21564's AudioOut2 set-up as the first of its process, as a console answered it (REQ-ac02,
//! `report-1791833977.txt` 30-66). A process of its own, because a process makes one context.

use orbistoun_core::GUEST_ARG_REGISTERS;

/// `SCE_AUDIO_OUT2_ERROR_INVALID_ARGUMENT`.
const INVALID_ARGUMENT: u64 = 0x8026_8001;

/// An implementation by name, so the test cannot reach one a guest cannot.
fn call(name: &str, args: [u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (_, function) = orbistoun_audio::implementations()
        .iter()
        .find(|(n, _)| *n == name)
        .unwrap_or_else(|| panic!("{name} is not served"));
    function(&args)
}

/// The title's block makes the process's first context, handle 0, in memory of the size it needs;
/// on it a port of eight channels is made with type 0 and with type 2, numbered 0 and 1.
#[test]
fn ppsa21564s_context_and_ports_are_made_in_a_fresh_process() {
    let mut param = [0_u8; 0x40];
    for (i, value) in [0x14_u32, 0x200, 0, 1, 0x200, 1].into_iter().enumerate() {
        param[i * 4..i * 4 + 4].copy_from_slice(&value.to_le_bytes());
    }
    let at = param.as_ptr() as u64;
    let mut size = [0xcc_u8; 8];
    assert_eq!(
        call(
            "sceAudioOut2ContextQueryMemory",
            [at, size.as_mut_ptr() as u64, 0, 0, 0, 0]
        ),
        0
    );
    let need = u64::from_le_bytes(size);
    assert_eq!(need, 0x78e9c);
    let memory = vec![0_u8; usize::try_from(need).expect("fits") + 16];
    let memory_at = (memory.as_ptr() as u64 + 15) & !15;
    let mut context = [0xcc_u8; 8];
    let context_at = context.as_mut_ptr() as u64;
    assert_eq!(
        call(
            "sceAudioOut2ContextCreate",
            [at, memory_at, need, context_at, 0, 0]
        ),
        0
    );
    assert_eq!(context, [0; 8], "the first context is 0");

    let mut user = [0_u8; 8];
    assert_eq!(
        call(
            "sceAudioOut2UserCreate",
            [0xff, user.as_mut_ptr() as u64, 0, 0, 0, 0]
        ),
        0
    );
    let mut ports = Vec::new();
    for port_type in [0_u32, 2] {
        let mut block = [0_u8; 0x40];
        block[4..8].copy_from_slice(&(8_u32 << 8).to_le_bytes());
        block[8..12].copy_from_slice(&48_000_u32.to_le_bytes());
        block[12..16].copy_from_slice(&port_type.to_le_bytes());
        block[16..24].copy_from_slice(&user);
        let mut port = [0xff_u8; 8];
        let args = [0, block.as_ptr() as u64, port.as_mut_ptr() as u64, 0, 0, 0];
        assert_eq!(call("sceAudioOut2PortCreate", args), 0, "type {port_type}");
        ports.push(u64::from_le_bytes(port));
    }
    assert_eq!(ports, [0, 1]);

    // The context's runtime (REQ-24b2, `report-1791836505.txt` 93-164): Advance and Push answer 0
    // on it, non-blocking (0) or blocking (1); a context never made and a flag of 2 are refused.
    for context in [0_u64, 0] {
        assert_eq!(
            call("sceAudioOut2ContextAdvance", [context, 0, 0, 0, 0, 0]),
            0
        );
    }
    for flag in [0_u64, 1] {
        assert_eq!(call("sceAudioOut2ContextPush", [0, flag, 0, 0, 0, 0]), 0);
    }
    assert_eq!(
        call("sceAudioOut2ContextPush", [0, 2, 0, 0, 0, 0]),
        INVALID_ARGUMENT
    );
    for bad in [1_u64, u64::MAX] {
        assert_eq!(
            call("sceAudioOut2ContextAdvance", [bad, 0, 0, 0, 0, 0]),
            INVALID_ARGUMENT
        );
        assert_eq!(
            call("sceAudioOut2ContextPush", [bad, 1, 0, 0, 0, 0]),
            INVALID_ARGUMENT
        );
    }
    // The queue level (REQ-24b2, `report-1791836505.txt` 85-92): queued 0 and available 2, each
    // a u32 written over the low half of its slot; one null slot is fine, both are refused, as are
    // contexts 1 and -1.
    let mut queued = [0xff_u8; 8];
    let mut available = [0xff_u8; 8];
    let (q, a) = (queued.as_mut_ptr() as u64, available.as_mut_ptr() as u64);
    assert_eq!(
        call("sceAudioOut2ContextGetQueueLevel", [0, q, a, 0, 0, 0]),
        0
    );
    assert_eq!(queued, [0, 0, 0, 0, 0xff, 0xff, 0xff, 0xff]);
    assert_eq!(available, [2, 0, 0, 0, 0xff, 0xff, 0xff, 0xff]);
    assert_eq!(
        call("sceAudioOut2ContextGetQueueLevel", [0, 0, a, 0, 0, 0]),
        0
    );
    assert_eq!(
        call("sceAudioOut2ContextGetQueueLevel", [0, q, 0, 0, 0, 0]),
        0
    );
    assert_eq!(
        call("sceAudioOut2ContextGetQueueLevel", [0, 0, 0, 0, 0, 0]),
        INVALID_ARGUMENT
    );
    for bad in [1_u64, u64::MAX] {
        assert_eq!(
            call("sceAudioOut2ContextGetQueueLevel", [bad, q, a, 0, 0, 0]),
            INVALID_ARGUMENT
        );
    }
}
