//! `libScePngDec` - decoding a PNG into the guest's own buffer.
//!
//! Served as obSCEne measured it on FW 12.40 (`REQ-20261012T1818Z-2bfa`,
//! `REQ-20261012T1822Z-8912`, `000-boot/png-dec-enc`, `report-1791878041.txt`): the memory a
//! decoder needs, the context `Create` writes into it, which colour space and flags a header
//! reports, the pixel orders a decode writes, and every error code those rows show. The parameter
//! blocks' field widths are the OpenOrbis toolchain's (`orbis/_types/png.h`, credited in
//! ACKNOWLEDGEMENTS.md); which field is which comes from the measured behaviour - a size of 7
//! refused as a size, a short pitch refused as a pitch. What no row covers answers the
//! placeholder: grey images, a 16-bit image on an attribute-1 decoder, an image wider than its
//! decoder, and a pitch or alpha the rows never exercised.
//!
//! Every arity is `6`, the trampoline's full capture.

use orbistoun_core::{GUEST_ARG_REGISTERS, GuestError};
use orbistoun_hle::guest_module;
use orbistoun_mem::guest;

guest_module! {
    "libScePngDec" {
        "scePngDecQueryMemorySize" => 6,
        "scePngDecCreate" => 6,
        "scePngDecParseHeader" => 6,
        "scePngDecDecode" => 6,
        "scePngDecDelete" => 6,
    }
}

/// A size field that is not the structure's, or memory short of what was queried (measured).
const INVALID_SIZE: u64 = 0x8069_0002;
/// A parameter out of range: attribute, width, reserved, pixel format, pitch, buffer (measured).
const INVALID_PARAM: u64 = 0x8069_0003;
/// A handle that is not a live decoder's (measured).
const INVALID_HANDLE: u64 = 0x8069_0004;
/// A 16-bit image on a decoder created for 8 bits (measured).
const UNSUPPORTED: u64 = 0x8069_0005;
/// Data too short to be a PNG (measured at 7 bytes).
const DATA_FORMAT: u64 = 0x8069_0010;
/// A header chunk whose checksum does not hold (measured).
const DECODE_ERROR: u64 = 0x8069_0012;

/// The create parameter's own size: `{size, attribute, max_width}`, 12 bytes (measured: 16 is
/// refused).
const CREATE_PARAM_SIZE: u32 = 12;
/// The widest image a decoder can be created for (measured: 1000000 sizes, 1000001 is refused).
const MAX_WIDTH: u32 = 1_000_000;

/// The memory a decoder of this attribute and width needs: `((w + 0x47) & !7) * (8 << attr) +
/// 0xd090`, which every measured point fits (w 1, 8, 4096 and 1000000 at attributes 0 and 1).
const fn memory_size(attribute: u32, width: u32) -> u64 {
    ((width as u64 + 0x47) & !7) * (8 << attribute) + 0xd090
}

/// The create parameter, checked as `QueryMemorySize` and `Create` check it: its size, an
/// attribute of 0 or 1, and a width of 1 to 1000000.
fn create_param(at: u64) -> Result<(u32, u32), u64> {
    let read = |offset| param_u32(at, offset);
    let (Some(size), Some(attribute), Some(width)) = (read(0), read(4), read(8)) else {
        return Err(u64::from(GuestError::Unimplemented.as_raw()));
    };
    if size != CREATE_PARAM_SIZE {
        return Err(INVALID_SIZE);
    }
    if attribute > 1 || width == 0 || width > MAX_WIDTH {
        return Err(INVALID_PARAM);
    }
    Ok((attribute, width))
}

/// `scePngDecQueryMemorySize(param)`: the bytes a decoder needs, or why not.
pub(crate) fn query_memory_size(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    match create_param(args[0]) {
        Ok((attribute, width)) => memory_size(attribute, width),
        Err(code) => code,
    }
}

/// `scePngDecCreate(param, memory, memorySize, &handle)`: a decoder in the guest's memory.
///
/// Writes the 0x18-byte context the console writes at the memory's base - itself, the parameter
/// size, the attribute, the width and a work size - and hands back its address as the handle
/// (measured: handle equal to the base). The work size was measured once, 0xd400 at width 64 and
/// attribute 0, which is the memory size less 0x90; elsewhere that difference is assumed.
pub(crate) fn create(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (attribute, width) = match create_param(args[0]) {
        Ok(fields) => fields,
        Err(code) => return code,
    };
    let (memory, size, out) = (args[1], args[2], args[3]);
    let needed = memory_size(attribute, width);
    if size < needed {
        return INVALID_SIZE;
    }
    let mut context = [0_u8; 0x18];
    context[0..8].copy_from_slice(&memory.to_le_bytes());
    context[8..12].copy_from_slice(&CREATE_PARAM_SIZE.to_le_bytes());
    context[12..16].copy_from_slice(&attribute.to_le_bytes());
    context[16..20].copy_from_slice(&width.to_le_bytes());
    context[20..24].copy_from_slice(&u32::try_from(needed - 0x90).unwrap_or(0).to_le_bytes());
    // SAFETY: the memory the guest handed over, at least `needed` bytes by the check above.
    let placed = unsafe { guest::write_bytes(memory, &context) };
    // SAFETY: the handle slot the guest passed for this call, valid by its contract.
    if placed && unsafe { guest::write_u64(out, memory) } {
        0
    } else {
        u64::from(GuestError::Unimplemented.as_raw())
    }
}

/// A live decoder's attribute and width, or `None` for anything else - a handle off by a byte, or
/// one `Delete` has cleared (measured: both `0x80690004`).
fn decoder(handle: u64) -> Option<(u32, u32)> {
    // SAFETY: a handle the guest passed; a live one is the context `create` wrote, and anything
    // else is read only as far as its first word.
    let live = handle != 0 && unsafe { guest::read_u64(handle) } == Some(handle);
    if !live {
        return None;
    }
    // SAFETY: as above - a live handle heads the 0x18-byte context.
    let field = |offset| unsafe { guest::read_u32(handle + offset) };
    Some((field(12)?, field(16)?))
}

/// The little-endian words of a parameter block the guest passed, by offset.
///
/// Each read is bounded by the block's published layout; the block is valid by the call's
/// contract.
fn param_u32(param: u64, offset: u64) -> Option<u32> {
    // SAFETY: see above.
    unsafe { guest::read_u32(param.wrapping_add(offset)) }
}

fn param_u64(param: u64, offset: u64) -> Option<u64> {
    // SAFETY: as for `param_u32`.
    unsafe { guest::read_u64(param.wrapping_add(offset)) }
}

/// `scePngDecDelete(handle)`: 0 for a live decoder, which is then no longer one.
pub(crate) fn delete(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    if decoder(args[0]).is_none() {
        return INVALID_HANDLE;
    }
    // SAFETY: a live handle, by the check above.
    unsafe { guest::write_u64(args[0], 0) };
    0
}

/// What a PNG's header says, as the console reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Header {
    width: u32,
    height: u32,
    /// 0x13 for RGBA, 0x3 for RGB, 0x4 for a palette (measured).
    colour_space: u16,
    bit_depth: u16,
    /// 0x1 interlaced, 0x2 a transparency chunk present (measured).
    flags: u32,
}

/// The eight bytes every PNG begins with.
const SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];

/// Reads a PNG's header the way `ParseHeader` reports it, or the code it refuses with.
fn header(png: &[u8]) -> Result<Header, u64> {
    if png.len() < 8 || png[..8] != SIGNATURE {
        return Err(DATA_FORMAT);
    }
    let mut at = 8;
    let mut found: Option<Header> = None;
    while at + 12 <= png.len() {
        let length = u32::from_be_bytes(png[at..at + 4].try_into().unwrap_or_default()) as usize;
        let kind = &png[at + 4..at + 8];
        let Some(end) = at.checked_add(12 + length).filter(|&end| end <= png.len()) else {
            return Err(DATA_FORMAT);
        };
        let body = &png[at + 8..at + 8 + length];
        let crc = u32::from_be_bytes(png[end - 4..end].try_into().unwrap_or_default());
        if kind == b"IHDR" {
            if length != 13 {
                return Err(DATA_FORMAT);
            }
            if crc32fast::hash(&png[at + 4..at + 8 + length]) != crc {
                return Err(DECODE_ERROR);
            }
            let colour_space = match body[9] {
                6 => 0x13,
                2 => 0x3,
                3 => 0x4,
                // Grey and grey-with-alpha: no row says what the console reports.
                _ => return Err(u64::from(GuestError::Unimplemented.as_raw())),
            };
            found = Some(Header {
                width: u32::from_be_bytes(body[0..4].try_into().unwrap_or_default()),
                height: u32::from_be_bytes(body[4..8].try_into().unwrap_or_default()),
                colour_space,
                bit_depth: u16::from(body[8]),
                flags: u32::from(body[12] == 1),
            });
        } else if kind == b"tRNS" {
            if let Some(header) = found.as_mut() {
                header.flags |= 0x2;
            }
        } else if kind == b"IDAT" {
            break;
        }
        at = end;
    }
    found.ok_or(DATA_FORMAT)
}

/// The PNG a parameter names: its bytes, read from guest memory.
fn png_bytes(at: u64, size: u32) -> Option<Vec<u8>> {
    let mut bytes = vec![0_u8; size as usize];
    // SAFETY: the stream the guest passed, `size` bytes by its contract.
    unsafe { guest::read_into(at, &mut bytes) }.then_some(bytes)
}

/// Writes the 16-byte image information block: width, height, colour space, bit depth, flags.
fn write_info(at: u64, header: &Header) -> bool {
    if at == 0 {
        return true;
    }
    let mut info = [0_u8; 16];
    info[0..4].copy_from_slice(&header.width.to_le_bytes());
    info[4..8].copy_from_slice(&header.height.to_le_bytes());
    info[8..10].copy_from_slice(&header.colour_space.to_le_bytes());
    info[10..12].copy_from_slice(&header.bit_depth.to_le_bytes());
    info[12..16].copy_from_slice(&header.flags.to_le_bytes());
    // SAFETY: the information block the guest passed, 16 bytes by the published layout.
    unsafe { guest::write_bytes(at, &info) }
}

/// `scePngDecParseHeader(param, info)`: the header's facts, or why not.
///
/// The parameter is `{png, size, reserved}`; a reserved word other than zero is refused.
pub(crate) fn parse_header(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let param = args[0];
    let fields = (
        param_u64(param, 0),
        param_u32(param, 8),
        param_u32(param, 12),
    );
    let (Some(png), Some(size), Some(reserved)) = fields else {
        return u64::from(GuestError::Unimplemented.as_raw());
    };
    if reserved != 0 {
        return INVALID_PARAM;
    }
    if size < 8 {
        return DATA_FORMAT;
    }
    let Some(bytes) = png_bytes(png, size) else {
        return u64::from(GuestError::Unimplemented.as_raw());
    };
    match header(&bytes) {
        Ok(found) if write_info(args[1], &found) => 0,
        Ok(_) => u64::from(GuestError::Unimplemented.as_raw()),
        Err(code) => code,
    }
}

/// The pixel orders a decode writes: R, G, B, A, or B, G, R, A (measured).
const PIXEL_RGBA: u16 = 0;
const PIXEL_BGRA: u16 = 1;

/// `scePngDecDecode(handle, param, info)`: the image, in the guest's buffer.
///
/// The parameter is `{png, image, pngSize, imageSize, pixelFormat, alpha, pitch}`. Answers the
/// image's width in the high half and height in the low (measured: `0x20002` for 2 by 2).
pub(crate) fn decode(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let Some((attribute, max_width)) = decoder(args[0]) else {
        return INVALID_HANDLE;
    };
    let param = args[1];
    let fields = (
        param_u64(param, 0),
        param_u64(param, 8),
        param_u32(param, 16),
        param_u32(param, 20),
        param_u32(param, 24),
        param_u32(param, 28),
    );
    let (Some(png), Some(image), Some(png_size), Some(image_size), Some(format_alpha), Some(pitch)) =
        fields
    else {
        return u64::from(GuestError::Unimplemented.as_raw());
    };
    let (format, alpha) = ((format_alpha & 0xffff) as u16, (format_alpha >> 16) as u8);
    if format != PIXEL_RGBA && format != PIXEL_BGRA {
        return INVALID_PARAM;
    }
    let Some(bytes) = png_bytes(png, png_size) else {
        return u64::from(GuestError::Unimplemented.as_raw());
    };
    let found = match header(&bytes) {
        Ok(found) => found,
        Err(code) => return code,
    };
    if found.bit_depth == 16 && attribute == 0 {
        return UNSUPPORTED;
    }
    // Unmeasured: a 16-bit image on an attribute-1 decoder, and one wider than the decoder.
    if found.bit_depth == 16 || found.width > max_width {
        return u64::from(GuestError::Unimplemented.as_raw());
    }
    let row = u64::from(found.width) * 4;
    let needed = u64::from(pitch) * u64::from(found.height.saturating_sub(1)) + row;
    if u64::from(pitch) < row || u64::from(image_size) < needed {
        return INVALID_PARAM;
    }
    let Some(pixels) = rgba(&bytes, alpha) else {
        return DECODE_ERROR;
    };
    let width = found.width as usize;
    for (y, line) in pixels.chunks_exact(width * 4).enumerate() {
        let mut line = line.to_vec();
        if format == PIXEL_BGRA {
            for pixel in line.chunks_exact_mut(4) {
                pixel.swap(0, 2);
            }
        }
        // SAFETY: the image buffer the guest passed, at least `needed` bytes by the check above.
        if !unsafe { guest::write_bytes(image + y as u64 * u64::from(pitch), &line) } {
            return u64::from(GuestError::Unimplemented.as_raw());
        }
    }
    if !write_info(args[2], &found) {
        return u64::from(GuestError::Unimplemented.as_raw());
    }
    (u64::from(found.width) << 16) | u64::from(found.height)
}

/// An 8-bit PNG's pixels as R, G, B, A rows: a palette and transparency expanded, and `alpha`
/// where the image has none.
fn rgba(png: &[u8], alpha: u8) -> Option<Vec<u8>> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(png));
    decoder.set_transformations(png::Transformations::EXPAND);
    let mut reader = decoder.read_info().ok()?;
    let mut buffer = vec![0_u8; reader.output_buffer_size()?];
    let frame = reader.next_frame(&mut buffer).ok()?;
    let buffer = &buffer[..frame.buffer_size()];
    match frame.color_type {
        png::ColorType::Rgba => Some(buffer.to_vec()),
        png::ColorType::Rgb => Some(
            buffer
                .chunks_exact(3)
                .flat_map(|p| [p[0], p[1], p[2], alpha])
                .collect(),
        ),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use orbistoun_core::GUEST_ARG_REGISTERS;

    /// A 2 by 2 RGBA PNG: red, green / blue, white.
    fn two_by_two() -> Vec<u8> {
        let mut out = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut out, 2, 2);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().expect("header");
            writer
                .write_image_data(&[
                    0xff, 0, 0, 0xff, 0, 0xff, 0, 0xff, 0, 0, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
                ])
                .expect("pixels");
        }
        out
    }

    fn args(values: &[u64]) -> [u64; GUEST_ARG_REGISTERS] {
        let mut out = [0; GUEST_ARG_REGISTERS];
        out[..values.len()].copy_from_slice(values);
        out
    }

    /// A host value's address, as a guest would pass one it only reads.
    fn at<T>(value: &T) -> u64 {
        std::ptr::from_ref(value).expose_provenance() as u64
    }

    /// A host value's address, as a guest would pass one the call writes.
    fn at_mut<T>(value: &mut T) -> u64 {
        std::ptr::from_mut(value).expose_provenance() as u64
    }

    /// The memory sizes at every measured point, and the measured refusals.
    #[test]
    fn the_memory_size_is_the_measured_formula() {
        let query = |size: u32, attribute: u32, width: u32| {
            let param = [size, attribute, width];
            super::query_memory_size(&args(&[at(&param)]))
        };
        assert_eq!(query(12, 0, 1), 0xd2d0);
        assert_eq!(query(12, 0, 8), 0xd2d0);
        assert_eq!(query(12, 0, 4096), 0x15290);
        assert_eq!(query(12, 0, 1_000_000), 0x7a_e490);
        assert_eq!(query(12, 1, 1), 0xd510);
        assert_eq!(query(12, 1, 4096), 0x1_d490);
        assert_eq!(query(12, 1, 1_000_000), 0xf4_f890);
        assert_eq!(query(12, 0, 0), 0x8069_0003);
        assert_eq!(query(12, 0, 1_000_001), 0x8069_0003);
        assert_eq!(query(12, 2, 8), 0x8069_0003);
        assert_eq!(query(16, 0, 8), 0x8069_0002);
    }

    /// Create writes the measured context and handle; a short block is refused; decode answers
    /// the packed size and writes both pixel orders; a handle off by 8 and a deleted one are
    /// refused.
    #[test]
    fn a_decoder_decodes_as_measured() {
        let param = [12_u32, 0, 64];
        let mut memory = vec![0xcc_u8; 0xd490 + 16];
        let base = (at_mut(&mut memory[0]) + 15) & !15;
        let mut handle = 0_u64;
        let create = |size: u64, handle: &mut u64| {
            super::create(&args(&[at(&param), base, size, at_mut(handle)]))
        };
        assert_eq!(create(0xd490 - 1, &mut handle), 0x8069_0002);
        assert_eq!(create(0xd490, &mut handle), 0);
        assert_eq!(handle, base);
        let offset = usize::try_from(base - at(&memory[0])).expect("small");
        assert_eq!(
            memory[offset + 8..offset + 24],
            [12, 0, 0, 0, 0, 0, 0, 0, 64, 0, 0, 0, 0, 0xd4, 0, 0]
        );

        let png = two_by_two();
        let mut image = [0_u8; 16];
        let mut info = [0xcc_u8; 16];
        let decode = |format: u32, pitch: u32, image: &mut [u8; 16], info: &mut [u8; 16]| {
            let mut block = [0_u8; 32];
            block[0..8].copy_from_slice(&at(&png[0]).to_le_bytes());
            block[8..16].copy_from_slice(&at_mut(&mut image[0]).to_le_bytes());
            block[16..20].copy_from_slice(&u32::try_from(png.len()).expect("small").to_le_bytes());
            block[20..24].copy_from_slice(&16_u32.to_le_bytes());
            block[24..28].copy_from_slice(&format.to_le_bytes());
            block[28..32].copy_from_slice(&pitch.to_le_bytes());
            super::decode(&args(&[handle, at(&block), at_mut(&mut info[0])]))
        };
        assert_eq!(decode(0, 8, &mut image, &mut info), 0x2_0002);
        assert_eq!(image[..8], [0xff, 0, 0, 0xff, 0, 0xff, 0, 0xff]);
        assert_eq!(info[..12], [2, 0, 0, 0, 2, 0, 0, 0, 0x13, 0, 8, 0]);
        assert_eq!(decode(1, 8, &mut image, &mut info), 0x2_0002);
        assert_eq!(image[..8], [0, 0, 0xff, 0xff, 0, 0xff, 0, 0xff]);
        assert_eq!(decode(0x84, 8, &mut image, &mut info), 0x8069_0003);
        assert_eq!(
            decode(0, 4, &mut image, &mut info),
            0x8069_0003,
            "short pitch"
        );

        assert_eq!(super::delete(&args(&[handle + 8])), 0x8069_0004, "off by 8");
        assert_eq!(super::delete(&args(&[handle])), 0);
        assert_eq!(super::delete(&args(&[handle])), 0x8069_0004, "twice");
        drop(memory);
    }

    /// The header's refusals as measured: reserved, a size under 8, and a corrupt IHDR checksum.
    #[test]
    fn a_header_is_refused_as_measured() {
        let mut png = two_by_two();
        let parse = |png: &[u8], size: u32, reserved: u32| {
            let mut block = [0_u8; 16];
            block[0..8].copy_from_slice(&at(&png[0]).to_le_bytes());
            block[8..12].copy_from_slice(&size.to_le_bytes());
            block[12..16].copy_from_slice(&reserved.to_le_bytes());
            let mut info = [0_u8; 16];
            super::parse_header(&args(&[at(&block), at_mut(&mut info[0])]))
        };
        let size = u32::try_from(png.len()).expect("small");
        assert_eq!(parse(&png, size, 0), 0);
        assert_eq!(parse(&png, size, 1), 0x8069_0003);
        assert_eq!(parse(&png, 7, 0), 0x8069_0010);
        png[29] ^= 0xff;
        assert_eq!(parse(&png, size, 0), 0x8069_0012, "IHDR checksum");
    }
}
