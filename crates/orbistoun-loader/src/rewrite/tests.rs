use super::{Field, Form, Host, How, Refusal, emulate, extrq, insertq, plan, trampoline};

/// Inputs from a fixed xorshift sequence, so a failure reproduces.
struct Inputs(u64);

impl Inputs {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn wide(&mut self) -> u128 {
        (u128::from(self.next()) << 64) | u128::from(self.next())
    }
}

/// The field length that names a whole quadword.
const WHOLE: u32 = 0;

/// `extrq` moves the field down and clears the rest of the low quadword; zero is sixty-four.
#[test]
fn extrq_moves_a_field_down_to_bit_zero() {
    let dst = 0x1111_2222_3333_4444_u128 << 64 | 0x0123_4567_89ab_cdef;
    assert_eq!(extrq(dst, 8, 8) as u64, 0xcd);
    assert_eq!(extrq(dst, 16, 48) as u64, 0x0123);
    assert_eq!(extrq(dst, WHOLE, 0) as u64, 0x0123_4567_89ab_cdef);
    assert_eq!(extrq(dst, 4, 0) >> 64, 0, "the upper quadword is cleared");
}

/// `insertq` writes the source's low bits into the field and keeps every other bit.
#[test]
fn insertq_writes_the_field_and_keeps_the_rest() {
    let dst = 0x1111_2222_3333_4444_u128 << 64 | 0x0123_4567_89ab_cdef;
    assert_eq!(insertq(dst, 0xff, 8, 8) as u64, 0x0123_4567_89ab_ffef);
    assert_eq!(
        insertq(dst, 0xabcd_0000_0000_0042, 4, 60) as u64,
        0x2123_4567_89ab_cdef
    );
    assert_eq!(insertq(dst, 7, WHOLE, 0) as u64, 7);
    assert_eq!(
        insertq(dst, 0, 4, 0) >> 64,
        0,
        "the upper quadword is cleared"
    );
}

/// Each form decodes from its published encoding with its registers and field.
#[test]
fn each_form_decodes_from_its_encoding() {
    let cases: [(&[u8], Form, usize); 8] = [
        (
            &[0x66, 0x0f, 0x78, 0xc2, 0x10, 0x20],
            Form::Extrq {
                dst: 2,
                src: 2,
                field: Field::Immediate { len: 16, index: 32 },
            },
            6,
        ),
        (
            &[0x66, 0x44, 0x0f, 0x79, 0xc5],
            Form::Extrq {
                dst: 8,
                src: 5,
                field: Field::Register,
            },
            5,
        ),
        (
            &[0x66, 0x0f, 0x79, 0xd5],
            Form::Extrq {
                dst: 2,
                src: 5,
                field: Field::Register,
            },
            4,
        ),
        (
            &[0xf2, 0x0f, 0x78, 0xfc, 0x18, 0x00],
            Form::Insertq {
                dst: 7,
                src: 4,
                field: Field::Immediate { len: 24, index: 0 },
            },
            6,
        ),
        (
            &[0xf2, 0x41, 0x0f, 0x79, 0xc1],
            Form::Insertq {
                dst: 0,
                src: 9,
                field: Field::Register,
            },
            5,
        ),
        (&[0x0f, 0x01, 0xfa], Form::Monitorx, 3),
        (&[0x0f, 0x01, 0xfb], Form::Mwaitx, 3),
        (&[0x0f, 0x01, 0xfd], Form::Rdpru, 3),
    ];
    for (bytes, form, len) in cases {
        assert_eq!(
            Form::decode(bytes, 0x1000),
            Some((form, len)),
            "{bytes:02x?}"
        );
    }
    assert_eq!(Form::decode(&[0x90], 0x1000), None);
}

/// A register-form field takes its length and index from the control register, which for `insertq`
/// is the source's high quadword.
#[test]
fn emulation_reads_a_register_field_from_its_control_quadword() {
    let mut xmm = [0_u128; 16];
    xmm[1] = 0xffff_0000_0000_abcd;
    xmm[2] = 0x0408; // length 8, index 4
    emulate(
        Form::Extrq {
            dst: 1,
            src: 2,
            field: Field::Register,
        },
        &mut xmm,
    )
    .expect("carried out");
    assert_eq!(xmm[1], 0xbc);
    xmm[3] = (0x0804_u128 << 64) | 0xf; // length 4, index 8
    emulate(
        Form::Insertq {
            dst: 1,
            src: 3,
            field: Field::Register,
        },
        &mut xmm,
    )
    .expect("carried out");
    assert_eq!(xmm[1], 0xfbc);
}

/// What reads console state, or nothing has used, is refused by name.
#[test]
fn rdpru_clzero_and_mcommit_are_refused_by_name() {
    let mut xmm = [0_u128; 16];
    for form in [Form::Rdpru, Form::Clzero, Form::Mcommit] {
        let refusal = emulate(form, &mut xmm).expect_err("refused");
        assert_eq!(refusal, Refusal(form));
        assert!(refusal.to_string().contains(form.mnemonic()), "{refusal}");
    }
    assert!(emulate(Form::Mwaitx, &mut xmm).is_ok());
}

/// A host with every extension rewrites nothing; one with none rewrites each site by its length.
#[test]
fn a_site_is_rewritten_by_whether_a_jump_fits() {
    let code = [
        0x66, 0x0f, 0x78, 0xc2, 0x10, 0x20, // 0x1000 extrq xmm2, 16, 32
        0x66, 0x0f, 0x79, 0xd5, // 0x1006 extrq xmm2, xmm5: too short for a jump
        0x0f, 0x01, 0xfa, // 0x100a monitorx
        0x0f, 0x01, 0xfb, // 0x100d mwaitx
        0x0f, 0x01, 0xfd, // 0x1010 rdpru
        0xc3,
    ];
    assert!(
        plan(&[(&code, 0x1000)], Host::FULL, 0x9000)
            .rewrites
            .is_empty()
    );
    let done = plan(&[(&code, 0x1000)], Host::BARE, 0x9000);
    let how: Vec<(u64, How)> = done.rewrites.iter().map(|r| (r.at, r.how)).collect();
    assert_eq!(
        how,
        [
            (0x1000, How::Trampoline),
            (0x1006, How::AtFault),
            (0x100a, How::InPlace),
            (0x100d, How::InPlace),
            (0x1010, How::Refused),
        ]
    );
    let patches: Vec<(u64, &[u8])> = done
        .patches
        .iter()
        .map(|(at, bytes)| (*at, bytes.as_slice()))
        .collect();
    assert_eq!(
        patches,
        [
            (0x1000, &[0xe9, 0xfb, 0x7f, 0x00, 0x00, 0xcc][..]),
            (0x100a, &[0x0f, 0x1f, 0x00][..]),
            (0x100d, &[0xf3, 0x90, 0x90][..]),
        ]
    );
    assert!(!done.room.is_empty());
}

/// The trampoline is the sequence its comments name, ending in a jump back past the site.
#[test]
fn a_trampoline_disassembles_as_written() {
    use iced_x86::{Decoder, DecoderOptions, Formatter, IntelFormatter};
    let form = Form::Insertq {
        dst: 9,
        src: 9,
        field: Field::Register,
    };
    let code = trampoline(form, 0x9000, 0x1005).expect("in reach");
    let mut formatter = IntelFormatter::new();
    let listed: Vec<String> = Decoder::with_ip(64, &code, 0x9000, DecoderOptions::NONE)
        .into_iter()
        .map(|instruction| {
            let mut text = String::new();
            formatter.format(&instruction, &mut text);
            text
        })
        .collect();
    assert_eq!(
        listed.first().map(String::as_str),
        Some("lea rsp,[rsp-80h]")
    );
    assert!(
        listed.contains(&"movdqu [rsp+10h],xmm9".to_owned()),
        "{listed:#?}"
    );
    assert!(
        listed.contains(&"movdqu xmm9,[rsp]".to_owned()),
        "{listed:#?}"
    );
    assert!(listed.contains(&"popfq".to_owned()), "{listed:#?}");
    assert_eq!(
        listed.last().map(String::as_str),
        Some("jmp 0000000000001005h")
    );
}

#[cfg(all(test, target_arch = "x86_64"))]
mod executed;
