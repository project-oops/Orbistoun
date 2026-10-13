//! `sceFontRenderCharGlyphImage` and its horizontal form with no font, as obSCEne measured them
//! (`REQ-20260926T1952Z-7795`, `130-layout/font-render-glyph`, `report-1791727748.txt` lines
//! 5878-5911): `0x80460005`, and the blocks passed behind it are left as they were.

use orbistoun_core::GUEST_ARG_REGISTERS;

fn call(name: &str, args: [u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (_, handler) = orbistoun_video::implementations()
        .iter()
        .find(|(n, _)| *n == name)
        .unwrap_or_else(|| panic!("{name} is implemented"));
    handler(&args)
}

#[test]
fn a_render_with_no_font_is_refused_and_writes_nothing() {
    for name in [
        "sceFontRenderCharGlyphImage",
        "sceFontRenderCharGlyphImageHorizontal",
    ] {
        for code in [u64::from(b'A'), u64::from(b' ')] {
            let surface = [0xcc_u8; 0x40];
            let metrics = [0xcc_u8; 0x20];
            let args = [
                0,
                code,
                surface.as_ptr() as u64,
                metrics.as_ptr() as u64,
                0,
                0,
            ];
            assert_eq!(call(name, args), 0x8046_0005, "{name} {code:#x}");
            assert_eq!(surface, [0xcc; 0x40]);
            assert_eq!(metrics, [0xcc; 0x20]);
        }
    }
}

/// A renderer is created over a font memory object in PPSA21564's shape and destroyed, and a
/// null memory object or out-pointer is refused `0x80460002` (`REQ-20261013T1730Z-fr03`).
#[test]
fn a_renderer_is_created_and_destroyed_as_measured() {
    let selection = call("sceFontSelectRendererFt", [0; 6]);
    assert_ne!(selection, 0);
    assert_eq!(call("sceFontSelectRendererFt", [1, 0, 0, 0, 0, 0]), 0);
    let memory = [0_u8; 0x40];
    let mut renderer = 0_u64;
    let out = std::ptr::addr_of_mut!(renderer) as u64;
    let create = |memory: u64, out: u64| {
        call(
            "sceFontCreateRendererWithEdition",
            [memory, selection, 0x0700_1000_0000_0000, out, 0, 0],
        )
    };
    assert_eq!(create(memory.as_ptr() as u64, out), 0);
    assert_ne!(renderer, 0);
    assert_eq!(create(0, out), 0x8046_0002);
    assert_eq!(create(memory.as_ptr() as u64, 0), 0x8046_0002);
    assert_eq!(call("sceFontDestroyRenderer", [out, 0, 0, 0, 0, 0]), 0);
    assert_eq!(renderer, 0);
}
