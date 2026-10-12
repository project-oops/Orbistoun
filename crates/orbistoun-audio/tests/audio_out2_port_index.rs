//! The first word of PPSA21564's port parameters, as a console answered it in a fresh process
//! (REQ-ac03, `report-1791839056.txt` 76-123). A process of its own, because a process makes one
//! context and ports are numbered across it.

use orbistoun_core::GUEST_ARG_REGISTERS;

/// An implementation by name, so the test cannot reach one a guest cannot.
fn call(name: &str, args: [u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (_, function) = orbistoun_audio::implementations()
        .iter()
        .find(|(n, _)| *n == name)
        .unwrap_or_else(|| panic!("{name} is not served"));
    function(&args)
}

/// On the title's context, eight-channel ports with the first word 0 and 1 are made - the word in
/// the handle's upper half, the process's count of ports made in its lower - and 2, 3, 7 and 0x100
/// are refused, the port written -1, for type 0 and type 2 alike.
#[test]
fn a_port_s_first_word_is_its_index_in_the_handle() {
    let mut param = [0_u8; 0x40];
    for (i, value) in [0x14_u32, 0x200, 0, 1, 0x200, 1].into_iter().enumerate() {
        param[i * 4..i * 4 + 4].copy_from_slice(&value.to_le_bytes());
    }
    let memory = vec![0_u8; 0x78e9c + 16];
    let memory_at = (memory.as_ptr() as u64 + 15) & !15;
    let mut context = [0xcc_u8; 8];
    let args = [
        param.as_ptr() as u64,
        memory_at,
        0x78e9c,
        context.as_mut_ptr() as u64,
        0,
        0,
    ];
    assert_eq!(call("sceAudioOut2ContextCreate", args), 0);
    let mut user = [0_u8; 8];
    let args = [0xff, user.as_mut_ptr() as u64, 0, 0, 0, 0];
    assert_eq!(call("sceAudioOut2UserCreate", args), 0);

    let create = |index: u32, port_type: u32| {
        let mut block = [0_u8; 0x40];
        block[0..4].copy_from_slice(&index.to_le_bytes());
        block[4..8].copy_from_slice(&(8_u32 << 8).to_le_bytes());
        block[8..12].copy_from_slice(&48_000_u32.to_le_bytes());
        block[12..16].copy_from_slice(&port_type.to_le_bytes());
        block[16..24].copy_from_slice(&user);
        let mut port = [0xff_u8; 8];
        let args = [0, block.as_ptr() as u64, port.as_mut_ptr() as u64, 0, 0, 0];
        (
            call("sceAudioOut2PortCreate", args),
            u64::from_le_bytes(port),
        )
    };
    let mut answered = Vec::new();
    for port_type in [0_u32, 2] {
        for index in [0_u32, 1, 2, 3, 7, 0x100] {
            answered.push(create(index, port_type));
        }
    }
    let none = u64::MAX;
    assert_eq!(
        answered,
        [
            (0, 0),
            (0, 0x1_0001),
            (0x8026_800e, none),
            (0x8026_800e, none),
            (0x8026_8011, none),
            (0x8026_8001, none),
            (0, 2),
            (0, 0x1_0003),
            (0x8026_800e, none),
            (0x8026_800e, none),
            (0x8026_8011, none),
            (0x8026_8001, none),
        ]
    );
}
