//! `libScePngEnc` - encoding the guest's image into a PNG in the guest's own buffer.
//!
//! Served as obSCEne measured it on FW 12.40 (`REQ-20261013T1545Z-5071`, `000-boot/png-dec-enc`,
//! rows in obSCEne's `reports/hardware/report-20261013-184500.log`): the memory an encoder needs,
//! the context `Create` writes into it, the parameters refused and their codes, the output
//! information an encode writes, and the filter each mask chose for an image's first row. The
//! parameter blocks' layouts are the ones that check passed the console, which took them:
//! `{size, attribute, max_width, max_filter}` to create, and `{image, png, imageSize, pngSize,
//! width, height, pitch, pixelFormat, colourSpace, bitDepth, clut, filter, compression}` to
//! encode.
//!
//! The PNG written is the guest's image, filtered and deflated as the format publishes. Its
//! compressed bytes are this host's deflate's, not the console's, so their count can differ from
//! what the console answers for the same image; what no row covers answers the placeholder.
//!
//! Every arity is `6`, the trampoline's full capture.

use orbistoun_core::{GUEST_ARG_REGISTERS, GuestError};
use orbistoun_hle::guest_module;
use orbistoun_mem::guest;

guest_module! {
    "libScePngEnc" {
        "scePngEncQueryMemorySize" => 6,
        "scePngEncCreate" => 6,
        "scePngEncEncode" => 6,
        "scePngEncDelete" => 6,
    }
}

/// An image address that is not 4-byte aligned (measured).
const INVALID_ADDR: u64 = 0x8069_0101;
/// A create parameter whose size field is not 16 (measured at 12, 24 and 32).
const INVALID_SIZE: u64 = 0x8069_0102;
/// An attribute other than 0, or a pitch below the row (measured).
const INVALID_PARAM: u64 = 0x8069_0103;
/// A handle that is not a live encoder's (measured: a second `Delete`).
const INVALID_HANDLE: u64 = 0x8069_0104;
/// An output buffer too small for the PNG (measured).
const OUTPUT_SIZE: u64 = 0x8069_0110;

/// The create parameter's own size (measured: 16 is taken, 12, 24 and 32 refused).
const CREATE_PARAM_SIZE: u32 = 16;
/// The most filters an encoder was measured for.
const MAX_FILTERS: u32 = 4;

/// The placeholder for what no row covers.
fn unmeasured() -> u64 {
    u64::from(GuestError::Unimplemented.as_raw())
}

/// The memory an encoder of this width and filter count needs: `0x45c70 + (2 + filters) *
/// round_up(width * 4, 32)`, which all twenty measured points fit (widths 1, 4, 64 and 4096,
/// filters 0 to 4).
const fn memory_size(width: u32, filters: u32) -> u64 {
    let row = (width as u64 * 4).next_multiple_of(32);
    0x45c70 + (2 + filters as u64) * row
}

/// The create parameter, checked as `QueryMemorySize` and `Create` check it: its size and an
/// attribute of 0 as measured, and a width and filter count the formula was measured over.
fn create_param(at: u64) -> Result<(u32, u32, u32), u64> {
    let read = |offset| param_u32(at, offset);
    let (Some(size), Some(attribute), Some(width), Some(filters)) =
        (read(0), read(4), read(8), read(12))
    else {
        return Err(unmeasured());
    };
    if size != CREATE_PARAM_SIZE {
        return Err(INVALID_SIZE);
    }
    if attribute != 0 {
        return Err(INVALID_PARAM);
    }
    if width == 0 || filters > MAX_FILTERS {
        return Err(unmeasured());
    }
    Ok((attribute, width, filters))
}

/// The little-endian words of a parameter block the guest passed, by offset.
///
/// Each read is bounded by the block's layout; the block is valid by the call's contract.
fn param_u32(param: u64, offset: u64) -> Option<u32> {
    // SAFETY: see above.
    unsafe { guest::read_u32(param.wrapping_add(offset)) }
}

fn param_u64(param: u64, offset: u64) -> Option<u64> {
    // SAFETY: as for `param_u32`.
    unsafe { guest::read_u64(param.wrapping_add(offset)) }
}

/// The two little-endian halves of the word at `offset`, low first.
fn param_halves(param: u64, offset: u64) -> Option<(u16, u16)> {
    let word = param_u32(param, offset)?;
    Some(((word & 0xffff) as u16, (word >> 16) as u16))
}

/// `scePngEncQueryMemorySize(param)`: the bytes an encoder needs, or why not.
pub(crate) fn query_memory_size(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    match create_param(args[0]) {
        Ok((_, width, filters)) => memory_size(width, filters),
        Err(code) => code,
    }
}

/// `scePngEncCreate(param, memory, memorySize, &handle)`: an encoder in the guest's memory.
///
/// Writes the context the console writes at the memory's base - itself, the parameter size, the
/// attribute, the width, the filter count and a work size - and hands back its address as the
/// handle (measured: handle equal to the base). The work size was measured once, 0x46200 at width
/// 64 and four filters, which is the memory size less 0x70; elsewhere that difference is assumed.
pub(crate) fn create(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (attribute, width, filters) = match create_param(args[0]) {
        Ok(fields) => fields,
        Err(code) => return code,
    };
    let (memory, size, out) = (args[1], args[2], args[3]);
    let needed = memory_size(width, filters);
    // Unmeasured: memory short of the size queried.
    if size < needed {
        return unmeasured();
    }
    let mut context = [0_u8; 0x1c];
    context[0..8].copy_from_slice(&memory.to_le_bytes());
    context[8..12].copy_from_slice(&CREATE_PARAM_SIZE.to_le_bytes());
    context[12..16].copy_from_slice(&attribute.to_le_bytes());
    context[16..20].copy_from_slice(&width.to_le_bytes());
    context[20..24].copy_from_slice(&filters.to_le_bytes());
    context[24..28].copy_from_slice(&u32::try_from(needed - 0x70).unwrap_or(0).to_le_bytes());
    // SAFETY: the memory the guest handed over, at least `needed` bytes by the check above.
    let placed = unsafe { guest::write_bytes(memory, &context) };
    // SAFETY: the handle slot the guest passed for this call, valid by its contract.
    if placed && unsafe { guest::write_u64(out, memory) } {
        0
    } else {
        unmeasured()
    }
}

/// A live encoder's width, or `None` for anything else - one `Delete` has cleared (measured:
/// `0x80690104`).
fn encoder(handle: u64) -> Option<u32> {
    // SAFETY: a handle the guest passed; a live one is the context `create` wrote, and anything
    // else is read only as far as its first word.
    let live = handle != 0 && unsafe { guest::read_u64(handle) } == Some(handle);
    if !live {
        return None;
    }
    // SAFETY: as above - a live handle heads the context.
    unsafe { guest::read_u32(handle + 16) }
}

/// `scePngEncDelete(handle)`: 0 for a live encoder, which is then no longer one.
pub(crate) fn delete(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    if encoder(args[0]).is_none() {
        return INVALID_HANDLE;
    }
    // SAFETY: a live handle, by the check above.
    unsafe { guest::write_u64(args[0], 0) };
    0
}

/// The colour space of an image with alpha, 19, the only one an encode was measured with.
const COLOUR_RGBA: u16 = 19;

/// What an encode was asked for.
struct EncodeParam {
    image: u64,
    png: u64,
    image_size: u32,
    png_size: u32,
    width: u32,
    height: u32,
    pitch: u32,
    pixel_format: u16,
    colour_space: u16,
    bit_depth: u16,
    clut: u16,
    filter: u16,
    compression: u16,
}

impl EncodeParam {
    /// The block at `at`, or `None` where any of it is unreadable.
    fn read(at: u64) -> Option<Self> {
        let (pixel_format, colour_space) = param_halves(at, 36)?;
        let (bit_depth, clut) = param_halves(at, 40)?;
        let (filter, compression) = param_halves(at, 44)?;
        Some(Self {
            image: param_u64(at, 0)?,
            png: param_u64(at, 8)?,
            image_size: param_u32(at, 16)?,
            png_size: param_u32(at, 20)?,
            width: param_u32(at, 24)?,
            height: param_u32(at, 28)?,
            pitch: param_u32(at, 32)?,
            pixel_format,
            colour_space,
            bit_depth,
            clut,
            filter,
            compression,
        })
    }
}

/// `scePngEncEncode(handle, param, outputInfo)`: the image as a PNG in the guest's buffer, and
/// its length.
///
/// Answers the PNG's length and writes `{length, rows}` to the output information (measured:
/// `0x88` and 4 for a 4x4 image stored at level 0). An image address off 4-byte alignment, a pitch
/// below the row and an output buffer too small are refused as measured; only the measured image
/// shape - 8-bit RGBA, pixel format 0, no palette - is encoded, and anything else answers the
/// placeholder.
pub(crate) fn encode(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let Some(max_width) = encoder(args[0]) else {
        return INVALID_HANDLE;
    };
    let Some(param) = EncodeParam::read(args[1]) else {
        return unmeasured();
    };
    if param.image % 4 != 0 {
        return INVALID_ADDR;
    }
    let row = u64::from(param.width) * 4;
    if u64::from(param.pitch) < row {
        return INVALID_PARAM;
    }
    let needed = u64::from(param.pitch) * u64::from(param.height.saturating_sub(1)) + row;
    let measured_shape = param.pixel_format == 0
        && param.colour_space == COLOUR_RGBA
        && param.bit_depth == 8
        && param.clut == 0
        && (1..=15).contains(&param.filter)
        && param.compression <= 9
        && param.width != 0
        && param.height != 0
        && param.width <= max_width
        && u64::from(param.image_size) >= needed;
    if !measured_shape {
        return unmeasured();
    }
    let mut image = vec![0_u8; usize::try_from(needed).unwrap_or(0)];
    // SAFETY: the image the guest passed, `needed` bytes within its `image_size` by the check above.
    if !unsafe { guest::read_into(param.image, &mut image) } {
        return unmeasured();
    }
    let rows: Vec<&[u8]> = (0..param.height as usize)
        .map(|y| {
            let start = y * param.pitch as usize;
            &image[start..start + row as usize]
        })
        .collect();
    let png = png_of(
        &rows,
        (param.width, param.height),
        (param.filter, param.compression),
    );
    if png.len() > param.png_size as usize {
        return OUTPUT_SIZE;
    }
    let length = u32::try_from(png.len()).unwrap_or(u32::MAX);
    let mut info = [0_u8; 8];
    info[0..4].copy_from_slice(&length.to_le_bytes());
    info[4..8].copy_from_slice(&param.height.to_le_bytes());
    // SAFETY: the output buffer the guest passed, at least `png.len()` bytes by the check above,
    // and its 8-byte output information block.
    let written = unsafe { guest::write_bytes(param.png, &png) }
        && (args[2] == 0 || unsafe { guest::write_bytes(args[2], &info) });
    if written {
        u64::from(length)
    } else {
        unmeasured()
    }
}

/// The PNG filter types, as each row's first byte names them.
const FILTER_NONE: u8 = 0;
const FILTER_SUB: u8 = 1;
const FILTER_UP: u8 = 2;
const FILTER_AVERAGE: u8 = 3;
const FILTER_PAETH: u8 = 4;

/// The filters a mask allows: bit 0 Sub, bit 1 Up, bit 2 Average, bit 3 Paeth (measured on an
/// image's first row: masks 1, 4 and 8 wrote Sub, Average and Paeth). Up on the first row writes
/// None, as mask 2 did there: with no row above, the two are the same bytes.
fn allowed(mask: u16, first_row: bool) -> Vec<u8> {
    let mut filters = Vec::new();
    for (bit, filter) in [
        (1, FILTER_SUB),
        (2, FILTER_UP),
        (4, FILTER_AVERAGE),
        (8, FILTER_PAETH),
    ] {
        if mask & bit != 0 {
            filters.push(if first_row && filter == FILTER_UP {
                FILTER_NONE
            } else {
                filter
            });
        }
    }
    filters
}

/// A row filtered by `filter` against the row above (`prior`, zero for the first), 4 bytes a pixel.
fn filtered(row: &[u8], prior: &[u8], filter: u8) -> Vec<u8> {
    let left = |i: usize| if i >= 4 { row[i - 4] } else { 0 };
    let above_left = |i: usize| if i >= 4 { prior[i - 4] } else { 0 };
    row.iter()
        .enumerate()
        .map(|(i, &x)| {
            let (a, b, c) = (left(i), prior[i], above_left(i));
            x.wrapping_sub(match filter {
                FILTER_SUB => a,
                FILTER_UP => b,
                FILTER_AVERAGE => a.midpoint(b),
                FILTER_PAETH => paeth(a, b, c),
                _ => 0,
            })
        })
        .collect()
}

/// The Paeth predictor (the PNG specification, section 9.4).
fn paeth(a: u8, b: u8, c: u8) -> u8 {
    let p = i16::from(a) + i16::from(b) - i16::from(c);
    let (pa, pb, pc) = (
        (p - i16::from(a)).abs(),
        (p - i16::from(b)).abs(),
        (p - i16::from(c)).abs(),
    );
    if pa <= pb && pa <= pc {
        a
    } else if pb <= pc {
        b
    } else {
        c
    }
}

/// A filtered row's cost: the sum of its bytes as signed magnitudes, the lowest of which a mask of
/// several filters chooses - first in None, Sub, Up, Average, Paeth order on a tie. Mask 15 chose
/// Sub for the measured first row, where it costs least.
fn cost(filtered: &[u8]) -> u32 {
    filtered
        .iter()
        .map(|&v| u32::from(if v < 128 { v } else { 0_u8.wrapping_sub(v) }))
        .sum()
}

/// The PNG of `rows` (8-bit RGBA, `width` x `height`): signature, header, the filtered rows
/// deflated at `compression` as one data chunk, and the end.
fn png_of(rows: &[&[u8]], (width, height): (u32, u32), (mask, compression): (u16, u16)) -> Vec<u8> {
    let mut raw = Vec::with_capacity(rows.len() * (rows.first().map_or(0, |r| r.len()) + 1));
    let zero = vec![0_u8; rows.first().map_or(0, |r| r.len())];
    for (y, row) in rows.iter().enumerate() {
        let prior: &[u8] = if y == 0 { &zero } else { rows[y - 1] };
        let best = allowed(mask, y == 0)
            .into_iter()
            .map(|filter| (filter, filtered(row, prior, filter)))
            .min_by_key(|(filter, bytes)| (cost(bytes), *filter))
            .unwrap_or_else(|| (FILTER_NONE, row.to_vec()));
        raw.push(best.0);
        raw.extend(best.1);
    }
    let data = miniz_oxide::deflate::compress_to_vec_zlib(&raw, compression.min(9) as u8);
    let mut header = Vec::with_capacity(13);
    header.extend(width.to_be_bytes());
    header.extend(height.to_be_bytes());
    // Bit depth 8, colour type 6 (RGBA), deflate, adaptive filtering, not interlaced.
    header.extend([8, 6, 0, 0, 0]);
    let mut png = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
    for (kind, body) in [
        (b"IHDR", &header[..]),
        (b"IDAT", &data[..]),
        (b"IEND", &[][..]),
    ] {
        png.extend(u32::try_from(body.len()).unwrap_or(u32::MAX).to_be_bytes());
        let start = png.len();
        png.extend(kind);
        png.extend(body);
        let crc = crc32fast::hash(&png[start..]);
        png.extend(crc.to_be_bytes());
    }
    png
}

#[cfg(test)]
mod tests {
    use orbistoun_core::GUEST_ARG_REGISTERS;

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

    /// Every measured memory size, and the measured refusals of a create parameter.
    #[test]
    fn the_memory_size_is_the_measured_formula() {
        let query = |size: u32, attribute: u32, width: u32, filters: u32| {
            let param = [size, attribute, width, filters];
            super::query_memory_size(&args(&[at(&param)]))
        };
        for (width, sizes) in [
            (1, [0x45cb0, 0x45cd0, 0x45cf0, 0x45d10, 0x45d30]),
            (4, [0x45cb0, 0x45cd0, 0x45cf0, 0x45d10, 0x45d30]),
            (64, [0x45e70, 0x45f70, 0x46070, 0x46170, 0x46270]),
            (4096, [0x4dc70, 0x51c70, 0x55c70, 0x59c70, 0x5dc70]),
        ] {
            for (filters, size) in (0..).zip(sizes) {
                assert_eq!(query(16, 0, width, filters), size, "w{width} f{filters}");
            }
        }
        for size in [12, 24, 32] {
            assert_eq!(query(size, 0, 64, 4), 0x8069_0102, "size {size}");
        }
        assert_eq!(query(16, 1, 64, 4), 0x8069_0103, "attribute 1");
    }

    /// The measured 4x4 image: bytes `i * 7`.
    fn image() -> [u8; 64] {
        std::array::from_fn(|i| (i * 7) as u8)
    }

    /// An encode block for `image` into `out`.
    fn block(
        image: u64,
        out: &mut [u8],
        pitch: u32,
        (filter, compression): (u16, u16),
    ) -> [u8; 48] {
        let mut b = [0_u8; 48];
        b[0..8].copy_from_slice(&image.to_le_bytes());
        b[8..16].copy_from_slice(&at_mut(&mut out[0]).to_le_bytes());
        b[16..20].copy_from_slice(&64_u32.to_le_bytes());
        b[20..24].copy_from_slice(&u32::try_from(out.len()).expect("small").to_le_bytes());
        b[24..28].copy_from_slice(&4_u32.to_le_bytes());
        b[28..32].copy_from_slice(&4_u32.to_le_bytes());
        b[32..36].copy_from_slice(&pitch.to_le_bytes());
        b[38..40].copy_from_slice(&19_u16.to_le_bytes());
        b[40..42].copy_from_slice(&8_u16.to_le_bytes());
        b[44..46].copy_from_slice(&filter.to_le_bytes());
        b[46..48].copy_from_slice(&compression.to_le_bytes());
        b
    }

    /// Create writes the measured context and handle; an encode writes a PNG of the image whose
    /// header is the measured one and whose first row is filtered as the measured masks chose, and
    /// the output information holds its length and the rows; the measured refusals; a second
    /// delete is refused.
    #[test]
    fn an_encoder_encodes_as_measured() {
        let param = [16_u32, 0, 64, 4];
        let mut memory = vec![0xcc_u8; 0x46270 + 16];
        let base = (at_mut(&mut memory[0]) + 15) & !15;
        let mut handle = 0_u64;
        assert_eq!(
            super::create(&args(&[at(&param), base, 0x46270, at_mut(&mut handle)])),
            0
        );
        assert_eq!(handle, base);
        let offset = usize::try_from(base - at(&memory[0])).expect("small");
        assert_eq!(
            memory[offset + 8..offset + 28],
            [
                16, 0, 0, 0, 0, 0, 0, 0, 64, 0, 0, 0, 4, 0, 0, 0, 0, 0x62, 4, 0
            ]
        );

        let pixels = image();
        let image_at = at(&pixels);
        let mut out = vec![0xcc_u8; 4096];
        let mut info = [0xcc_u8; 8];
        for (filter, first_row_filter) in [(1, 1), (2, 0), (4, 3), (8, 4), (15, 1)] {
            let b = block(image_at, &mut out, 16, (filter, 0));
            let length = super::encode(&args(&[handle, at(&b), at_mut(&mut info[0])]));
            assert!(length < 0x1000, "{filter}: {length:#x}");
            let png = &out[..usize::try_from(length).expect("small")];
            // The measured signature, header and its checksum.
            assert_eq!(
                png[..33],
                [
                    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 0x0d, 0x49, 0x48,
                    0x44, 0x52, 0, 0, 0, 4, 0, 0, 0, 4, 8, 6, 0, 0, 0, 0xa9, 0xf1, 0x9e, 0x7e
                ]
            );
            assert_eq!(
                info[..4],
                u32::try_from(length).expect("small").to_le_bytes()
            );
            assert_eq!(info[4..], 4_u32.to_le_bytes());
            let decoded = inflate_rows(png);
            assert_eq!(decoded[0], first_row_filter, "mask {filter}");
            assert_eq!(unfilter(&decoded), pixels.to_vec(), "mask {filter}");
        }

        let b = block(image_at, &mut out, 8, (1, 0));
        assert_eq!(
            super::encode(&args(&[handle, at(&b), at_mut(&mut info[0])])),
            0x8069_0103
        );
        let b = block(image_at + 1, &mut out, 16, (1, 0));
        assert_eq!(
            super::encode(&args(&[handle, at(&b), at_mut(&mut info[0])])),
            0x8069_0101
        );
        let mut small = [0_u8; 16];
        let b = block(image_at, &mut small, 16, (1, 0));
        assert_eq!(
            super::encode(&args(&[handle, at(&b), at_mut(&mut info[0])])),
            0x8069_0110
        );

        assert_eq!(super::delete(&args(&[handle])), 0);
        assert_eq!(super::delete(&args(&[handle])), 0x8069_0104, "twice");
        drop(memory);
    }

    /// The filtered rows a PNG's data chunk inflates to.
    fn inflate_rows(png: &[u8]) -> Vec<u8> {
        let length = u32::from_be_bytes(png[33..37].try_into().expect("four")) as usize;
        assert_eq!(&png[37..41], b"IDAT");
        miniz_oxide::inflate::decompress_to_vec_zlib(&png[41..41 + length]).expect("inflates")
    }

    /// 4x4 RGBA rows undone from their filters.
    fn unfilter(raw: &[u8]) -> Vec<u8> {
        let mut out: Vec<u8> = Vec::new();
        for (y, line) in raw.chunks_exact(17).enumerate() {
            let (filter, bytes) = (line[0], &line[1..]);
            let mut row = vec![0_u8; 16];
            for i in 0..16 {
                let a = if i >= 4 { row[i - 4] } else { 0 };
                let b = if y > 0 { out[(y - 1) * 16 + i] } else { 0 };
                let c = if i >= 4 && y > 0 {
                    out[(y - 1) * 16 + i - 4]
                } else {
                    0
                };
                row[i] = bytes[i].wrapping_add(match filter {
                    1 => a,
                    2 => b,
                    3 => a.midpoint(b),
                    4 => super::paeth(a, b, c),
                    _ => 0,
                });
            }
            out.extend(row);
        }
        out
    }
}
