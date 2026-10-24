//! A guest compute dispatch written out whole, so it runs again alone ([`crate::replay`]).
//!
//! A capture is a directory: [`MANIFEST`] names everything the dispatch was handed, a line each,
//! and each array of words is a file of its own, little-endian. It holds what
//! [`crate::dispatch_guest_hashed`] was handed rather than the device objects made from it, so a
//! replay goes through the same binding code, and a change to that code is replayed too. Only what
//! is bound is written: the guest-memory window, each image's texels and each buffer the program
//! reads. A buffer it only stores to is its length, and an alias is the buffer it shares.
//!
//! `ORBISTOUN_CAPTURE_DISPATCH` names programs whose first dispatch, or each of their first
//! `EVERY_DISPATCH_MOST` with `:all`, is written before it is submitted. A dispatch that loses the
//! device is written after it fails, from the inputs it still borrows, while the run's ending is
//! held back (`device_lost::hold_ending`): nothing is copied or kept for the dispatches that
//! succeed.

use std::borrow::Cow;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use orbistoun_gpu::bound_ranges::BoundRanges;
use orbistoun_gpu::{BorderColour, MipFilter, TextureFilter, TextureSampling, TextureWrap};

use crate::compute::DispatchError;
use crate::compute_buffers::BufferIn;
use crate::compute_images::{DispatchFormat, DispatchImage, DispatchImages};

/// The file in a capture directory that names the rest.
pub const MANIFEST: &str = "dispatch.txt";

/// The manifest's first line, which says what the directory is.
const HEADER: &str = "# a guest compute dispatch: orbistoun-cli replay-dispatch <this directory>";

// Word arrays are written as the host holds them, which is the little-endian order the format
// names.
const _: () = assert!(cfg!(target_endian = "little"));

/// Why a dispatch was written out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    /// `ORBISTOUN_CAPTURE_DISPATCH` named its program; written before it was submitted.
    Asked,
    /// It lost the device; written after it failed.
    DeviceLost,
    /// It changed words in a range `ORBISTOUN_WATCH_GPU` names; written after it ran, from the
    /// inputs it was handed, so a replay makes the same change.
    Watched,
    /// It binds a range `ORBISTOUN_WATCH_GPU` names `:bound`; written before it was submitted.
    Bound,
    /// It ran shortly before a dispatch that lost the device; kept from its inputs when it ran
    /// (`ORBISTOUN_CAPTURE_RECENT`) and written at the loss.
    Recent,
}

impl Reason {
    const ALL: [Self; 5] = [
        Self::Asked,
        Self::DeviceLost,
        Self::Watched,
        Self::Bound,
        Self::Recent,
    ];

    const fn name(self) -> &'static str {
        match self {
            Self::Asked => "asked",
            Self::DeviceLost => "device-lost",
            Self::Watched => "watched",
            Self::Bound => "bound",
            Self::Recent => "recent",
        }
    }
}

/// Which of a dispatch's image bindings an image is at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageRole {
    /// The first texture slot's ([`DispatchImages::fetched`]).
    Fetched,
    /// A further texture slot's, in order ([`DispatchImages::more_fetched`]).
    MoreFetched,
    /// A stored image's, in order ([`DispatchImages::stored`]).
    Stored,
}

impl ImageRole {
    const ALL: [Self; 3] = [Self::Fetched, Self::MoreFetched, Self::Stored];

    const fn name(self) -> &'static str {
        match self {
            Self::Fetched => "fetched",
            Self::MoreFetched => "more-fetched",
            Self::Stored => "stored",
        }
    }
}

/// One image a captured dispatch bound, as [`DispatchImage`] holds it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapturedImage<'a> {
    /// Which binding it is at.
    pub role: ImageRole,
    /// Its texels before the dispatch.
    pub texels: Cow<'a, [u32]>,
    /// What a texel is.
    pub format: DispatchFormat,
    /// Width in texels.
    pub width: u32,
    /// Height in texels.
    pub height: u32,
    /// Slices of a three-dimensional image.
    pub depth: Option<u32>,
    /// Layers of an array the program addresses as one.
    pub layers: Option<u32>,
    /// The sampler it is sampled through, where its program names one.
    pub sampling: Option<TextureSampling>,
    /// Whether it is sampled as a cube.
    pub cube: bool,
}

/// One buffer a captured dispatch bound, as [`BufferIn`] holds it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CapturedBuffer<'a> {
    /// Its words before the dispatch.
    Words(Cow<'a, [u32]>),
    /// A buffer of this many words the program only stores to.
    Unread(usize),
    /// The buffer at this other index, from this many bytes into it.
    Alias(usize, u64),
}

/// Everything a guest compute dispatch was handed: enough to run it again alone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Capture<'a> {
    /// The guest program's address, where the dispatch was noted with one.
    pub program: Option<u64>,
    /// Why it was written out.
    pub reason: Reason,
    /// The SPIR-V module, as translated, before any change for the device.
    pub module: Cow<'a, [u32]>,
    /// The guest-memory window at binding 1, before.
    pub memory: Cow<'a, [u32]>,
    /// The user-data block.
    pub push: Cow<'a, [u32]>,
    /// Workgroups.
    pub groups: [u32; 3],
    /// Its images, the fetched first, then the further fetched, then the stored, each in order.
    pub images: Vec<CapturedImage<'a>>,
    /// Its buffers, by slot.
    pub buffers: Vec<CapturedBuffer<'a>>,
    /// Where the window and each buffer lie in guest memory, where the command processor noted
    /// it: the buffers' either one per slot or none.
    pub ranges: BoundRanges,
    /// Which of the run's guest dispatches it was, counting from one, so captures of several
    /// programs are put in the order they ran.
    pub ordinal: Option<u64>,
}

impl<'a> Capture<'a> {
    /// A capture borrowing what a dispatch was handed.
    pub fn of(
        (program, reason): (Option<u64>, Reason),
        module: &'a [u32],
        memory: &'a [u32],
        (push, groups): (&'a [u32], [u32; 3]),
        images: &DispatchImages<'a>,
        buffers: &[BufferIn<'a>],
    ) -> Self {
        let image = |role, image: &DispatchImage<'a>| CapturedImage {
            role,
            texels: Cow::Borrowed(image.texels),
            format: image.format,
            width: image.width,
            height: image.height,
            depth: image.depth,
            layers: image.layers,
            sampling: image.sampling,
            cube: image.cube,
        };
        let images = images
            .fetched
            .iter()
            .map(|fetched| image(ImageRole::Fetched, fetched))
            .chain(
                images
                    .more_fetched
                    .iter()
                    .map(|more| image(ImageRole::MoreFetched, more)),
            )
            .chain(
                images
                    .stored
                    .iter()
                    .map(|stored| image(ImageRole::Stored, stored)),
            )
            .collect();
        let buffers = buffers
            .iter()
            .map(|buffer| match *buffer {
                BufferIn::Words(words) => CapturedBuffer::Words(Cow::Borrowed(words)),
                BufferIn::Unread(words) => CapturedBuffer::Unread(words),
                BufferIn::Alias(holder, offset) => CapturedBuffer::Alias(holder, offset),
            })
            .collect();
        Self {
            program,
            reason,
            module: Cow::Borrowed(module),
            memory: Cow::Borrowed(memory),
            push: Cow::Borrowed(push),
            groups,
            images,
            buffers,
            ranges: BoundRanges::default(),
            ordinal: None,
        }
    }

    /// Its images as a dispatch takes them: the fetched one, the further fetched and the stored.
    pub fn dispatch_images(
        &self,
    ) -> (
        Option<DispatchImage<'_>>,
        Vec<DispatchImage<'_>>,
        Vec<DispatchImage<'_>>,
    ) {
        let of = |role: ImageRole| {
            self.images
                .iter()
                .filter(move |image| image.role == role)
                .map(|image| DispatchImage {
                    texels: &image.texels,
                    format: image.format,
                    width: image.width,
                    height: image.height,
                    depth: image.depth,
                    layers: image.layers,
                    sampling: image.sampling,
                    cube: image.cube,
                })
        };
        (
            of(ImageRole::Fetched).next(),
            of(ImageRole::MoreFetched).collect(),
            of(ImageRole::Stored).collect(),
        )
    }

    /// Its buffers as a dispatch takes them.
    pub fn buffers_in(&self) -> Vec<BufferIn<'_>> {
        self.buffers
            .iter()
            .map(|buffer| match buffer {
                CapturedBuffer::Words(words) => BufferIn::Words(words),
                CapturedBuffer::Unread(words) => BufferIn::Unread(*words),
                CapturedBuffer::Alias(holder, offset) => BufferIn::Alias(*holder, *offset),
            })
            .collect()
    }

    /// An owned copy of everything but the module, which is left empty: a keeper of many captures
    /// holds each module once.
    pub(crate) fn owned_without_module(&self) -> Capture<'static> {
        let owned = |words: &Cow<'_, [u32]>| Cow::Owned(words.to_vec());
        Capture {
            program: self.program,
            reason: self.reason,
            module: Cow::Owned(Vec::new()),
            memory: owned(&self.memory),
            push: owned(&self.push),
            groups: self.groups,
            images: self
                .images
                .iter()
                .map(|image| CapturedImage {
                    role: image.role,
                    texels: owned(&image.texels),
                    format: image.format,
                    width: image.width,
                    height: image.height,
                    depth: image.depth,
                    layers: image.layers,
                    sampling: image.sampling,
                    cube: image.cube,
                })
                .collect(),
            buffers: self
                .buffers
                .iter()
                .map(|buffer| match buffer {
                    CapturedBuffer::Words(words) => CapturedBuffer::Words(owned(words)),
                    CapturedBuffer::Unread(words) => CapturedBuffer::Unread(*words),
                    CapturedBuffer::Alias(holder, offset) => {
                        CapturedBuffer::Alias(*holder, *offset)
                    }
                })
                .collect(),
            ranges: self.ranges.clone(),
            ordinal: self.ordinal,
        }
    }

    /// The bytes its files hold together.
    pub fn bytes(&self) -> u64 {
        let words = self.module.len()
            + self.memory.len()
            + self.images.iter().map(|i| i.texels.len()).sum::<usize>()
            + self
                .buffers
                .iter()
                .map(|b| match b {
                    CapturedBuffer::Words(words) => words.len(),
                    _ => 0,
                })
                .sum::<usize>();
        words as u64 * 4
    }

    /// The manifest and each file of words, by name, as `put` is to store them.
    ///
    /// # Errors
    ///
    /// Whatever `put` answers.
    pub fn save(
        &self,
        put: &mut dyn FnMut(&str, &[u8]) -> std::io::Result<()>,
    ) -> std::io::Result<()> {
        let mut manifest = String::new();
        let mut files: Vec<(String, &[u32])> = Vec::new();
        let _ = writeln!(manifest, "{HEADER}");
        let _ = writeln!(
            manifest,
            "program {}",
            self.program
                .map_or_else(|| "none".to_owned(), |p| format!("{p:#x}"))
        );
        let _ = writeln!(manifest, "reason {}", self.reason.name());
        if let Some(ordinal) = self.ordinal {
            let _ = writeln!(manifest, "ordinal {ordinal}");
        }
        let [x, y, z] = self.groups;
        let _ = writeln!(manifest, "groups {x} {y} {z}");
        let _ = write!(manifest, "push");
        for word in self.push.iter() {
            let _ = write!(manifest, " {word:#x}");
        }
        let _ = writeln!(manifest);
        let _ = writeln!(manifest, "module module.spv");
        files.push(("module.spv".to_owned(), &self.module));
        let _ = writeln!(
            manifest,
            "memory memory.bin{}",
            range_text(self.ranges.window)
        );
        files.push(("memory.bin".to_owned(), &self.memory));
        for (index, image) in self.images.iter().enumerate() {
            let name = format!("image-{index}.bin");
            let _ = writeln!(manifest, "image {name} {}", image_fields(image));
            files.push((name, &image.texels));
        }
        for (index, buffer) in self.buffers.iter().enumerate() {
            let range = range_text(self.ranges.buffers.get(index).copied());
            match buffer {
                CapturedBuffer::Words(words) => {
                    let name = format!("buffer-{index}.bin");
                    let _ = writeln!(manifest, "buffer words {name}{range}");
                    files.push((name, words));
                }
                CapturedBuffer::Unread(words) => {
                    let _ = writeln!(manifest, "buffer unread {words}{range}");
                }
                CapturedBuffer::Alias(holder, offset) => {
                    let _ = writeln!(manifest, "buffer alias {holder} {offset:#x}{range}");
                }
            }
        }
        for (name, words) in files {
            put(&name, zerocopy::IntoBytes::as_bytes(words))?;
        }
        // Last, so a directory with a manifest holds every file it names.
        put(MANIFEST, manifest.as_bytes())
    }

    /// A capture from the manifest and the files `get` answers by name.
    ///
    /// # Errors
    ///
    /// A file `get` cannot answer, or a manifest line this does not write.
    pub fn load(
        get: &mut dyn FnMut(&str) -> std::io::Result<Vec<u8>>,
    ) -> Result<Capture<'static>, DispatchError> {
        let manifest = get(MANIFEST).map_err(|e| bad(format!("{MANIFEST}: {e}")))?;
        let manifest = String::from_utf8(manifest).map_err(|_| bad("not text".to_owned()))?;
        let mut words = |name: &str| read_words(get, name);
        let mut capture = Capture::of(
            (None, Reason::Asked),
            &[],
            &[],
            (&[], [0; 3]),
            &DispatchImages::default(),
            &[],
        );
        let mut seen_module = false;
        for line in manifest.lines().filter(|line| !line.starts_with('#')) {
            let fields: Vec<&str> = line.split_whitespace().collect();
            let Some((&key, rest)) = fields.split_first() else {
                continue;
            };
            let unread = || bad(format!("unreadable line: {line}"));
            let (rest, range) = split_range(rest).ok_or_else(unread)?;
            match (key, range) {
                ("memory", _) => capture.ranges.window = range,
                ("buffer", Some(range)) => capture.ranges.buffers.push(range),
                (_, None) => {}
                _ => return Err(unread()),
            }
            match (key, rest) {
                ("ordinal", [ordinal]) => {
                    capture.ordinal = Some(ordinal.parse().map_err(|_| unread())?);
                }
                ("program", ["none"]) => capture.program = None,
                ("program", [at]) => capture.program = Some(number(at).ok_or_else(unread)?),
                ("reason", [reason]) => {
                    capture.reason = Reason::ALL
                        .into_iter()
                        .find(|r| r.name() == *reason)
                        .ok_or_else(unread)?;
                }
                ("groups", [x, y, z]) => {
                    let group = |s: &str| s.parse().ok();
                    capture.groups = [
                        group(x).ok_or_else(unread)?,
                        group(y).ok_or_else(unread)?,
                        group(z).ok_or_else(unread)?,
                    ];
                }
                ("push", pushed) => {
                    capture.push = Cow::Owned(
                        pushed
                            .iter()
                            .map(|w| number(w).and_then(|w| u32::try_from(w).ok()))
                            .collect::<Option<_>>()
                            .ok_or_else(unread)?,
                    );
                }
                ("module", [name]) => {
                    capture.module = words(name)?;
                    seen_module = true;
                }
                ("memory", [name]) => capture.memory = words(name)?,
                ("image", [name, fields @ ..]) => {
                    let texels = words(name)?;
                    capture
                        .images
                        .push(parse_image(fields, texels).ok_or_else(unread)?);
                }
                ("buffer", ["words", name]) => {
                    capture.buffers.push(CapturedBuffer::Words(words(name)?));
                }
                ("buffer", ["unread", count]) => capture
                    .buffers
                    .push(CapturedBuffer::Unread(count.parse().map_err(|_| unread())?)),
                ("buffer", ["alias", holder, offset]) => {
                    capture.buffers.push(CapturedBuffer::Alias(
                        holder.parse().map_err(|_| unread())?,
                        number(offset).ok_or_else(unread)?,
                    ));
                }
                _ => return Err(unread()),
            }
        }
        if !seen_module {
            return Err(bad(format!("{MANIFEST} names no module")));
        }
        if !capture.ranges.buffers.is_empty()
            && capture.ranges.buffers.len() != capture.buffers.len()
        {
            return Err(bad(format!(
                "{MANIFEST} places some buffers in guest memory and not others"
            )));
        }
        Ok(capture)
    }

    /// Writes it into `dir`, made if missing.
    ///
    /// # Errors
    ///
    /// The directory or a file could not be written.
    pub fn write_to(&self, dir: &Path) -> std::io::Result<()> {
        std::fs::create_dir_all(dir)?;
        self.save(&mut |name, bytes| std::fs::write(dir.join(name), bytes))
    }

    /// The capture written into `dir`.
    ///
    /// # Errors
    ///
    /// As [`Self::load`].
    pub fn read_from(dir: &Path) -> Result<Capture<'static>, DispatchError> {
        Self::load(&mut |name| std::fs::read(dir.join(name)))
    }
}

/// The words of the file `get` answers for `name`, which must lie in the capture's directory.
fn read_words(
    get: &mut dyn FnMut(&str) -> std::io::Result<Vec<u8>>,
    name: &str,
) -> Result<Cow<'static, [u32]>, DispatchError> {
    if !plain_name(name) {
        return Err(bad(format!("{name} is not a file of this directory")));
    }
    let bytes = get(name).map_err(|e| bad(format!("{name}: {e}")))?;
    if bytes.len() % 4 != 0 {
        return Err(bad(format!("{name} is not whole words")));
    }
    Ok(Cow::Owned(
        bytes
            .chunks_exact(4)
            .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
            .collect(),
    ))
}

/// A manifest line's fields.
type Fields<'f> = &'f [&'f str];

/// A binding's guest range as the end of its manifest line, ` at <first byte> bytes <length>`, or
/// nothing where it is not known.
fn range_text(range: Option<(u64, u64)>) -> String {
    range.map_or_else(String::new, |(at, bytes)| {
        format!(" at {at:#x} bytes {bytes:#x}")
    })
}

/// A manifest line's fields after its key without the guest range [`range_text`] ends it with,
/// and that range; `None` for a range that is not numbers.
fn split_range(fields: Fields<'_>) -> Option<(Fields<'_>, Option<(u64, u64)>)> {
    match fields {
        [rest @ .., "at", at, "bytes", bytes] => Some((rest, Some((number(at)?, number(bytes)?)))),
        _ => Some((fields, None)),
    }
}

/// The error for a capture that cannot be read.
fn bad(why: String) -> DispatchError {
    DispatchError::Capture(why)
}

/// Whether `name` is a file directly inside the capture directory.
fn plain_name(name: &str) -> bool {
    !name.is_empty() && name != ".." && !name.contains(['/', '\\', ':'])
}

/// A number written in hexadecimal with `0x`, or in decimal.
fn number(text: &str) -> Option<u64> {
    text.strip_prefix("0x").map_or_else(
        || text.parse().ok(),
        |hex| u64::from_str_radix(hex, 16).ok(),
    )
}

/// An optional count, `-` for none.
fn optional(value: Option<u32>) -> String {
    value.map_or_else(|| "-".to_owned(), |v| v.to_string())
}

/// An image's manifest fields after its file: role, format, width, height, depth, layers, cube or
/// flat, and its sampler or `-`.
fn image_fields(image: &CapturedImage<'_>) -> String {
    format!(
        "{} {} {} {} {} {} {} {}",
        image.role.name(),
        format_name(image.format),
        image.width,
        image.height,
        optional(image.depth),
        optional(image.layers),
        if image.cube { "cube" } else { "flat" },
        image
            .sampling
            .as_ref()
            .map_or_else(|| "-".to_owned(), sampling_text),
    )
}

/// An image from its manifest fields after its file, and its texels.
fn parse_image(fields: &[&str], texels: Cow<'static, [u32]>) -> Option<CapturedImage<'static>> {
    let [role, format, width, height, depth, layers, cube, sampling] = fields else {
        return None;
    };
    let optional = |s: &str| {
        if s == "-" {
            Some(None)
        } else {
            s.parse().ok().map(Some)
        }
    };
    Some(CapturedImage {
        role: ImageRole::ALL.into_iter().find(|r| r.name() == *role)?,
        texels,
        format: FORMATS.into_iter().find(|f| format_name(*f) == *format)?,
        width: width.parse().ok()?,
        height: height.parse().ok()?,
        depth: optional(depth)?,
        layers: optional(layers)?,
        cube: match *cube {
            "cube" => true,
            "flat" => false,
            _ => return None,
        },
        sampling: if *sampling == "-" {
            None
        } else {
            Some(parse_sampling(sampling)?)
        },
    })
}

/// Every image format, for reading one back by its name.
const FORMATS: [DispatchFormat; 17] = [
    DispatchFormat::Rgba8,
    DispatchFormat::R8,
    DispatchFormat::R8Uint,
    DispatchFormat::Rgba8Uint,
    DispatchFormat::Rgba32Uint,
    DispatchFormat::R32Uint,
    DispatchFormat::R16Uint,
    DispatchFormat::R16Unorm,
    DispatchFormat::R16Float,
    DispatchFormat::Rg16Float,
    DispatchFormat::Rg8Unorm,
    DispatchFormat::Rg32Uint,
    DispatchFormat::R32Float,
    DispatchFormat::Rgb10A2,
    DispatchFormat::Rgba16Float,
    DispatchFormat::Rgba16Unorm,
    DispatchFormat::Float11_11_10,
];

/// A format's name in a manifest.
const fn format_name(format: DispatchFormat) -> &'static str {
    match format {
        DispatchFormat::Rgba8 => "rgba8",
        DispatchFormat::R8 => "r8",
        DispatchFormat::R8Uint => "r8-uint",
        DispatchFormat::Rgba8Uint => "rgba8-uint",
        DispatchFormat::Rgba32Uint => "rgba32-uint",
        DispatchFormat::R32Uint => "r32-uint",
        DispatchFormat::R16Uint => "r16-uint",
        DispatchFormat::R16Unorm => "r16-unorm",
        DispatchFormat::R16Float => "r16-float",
        DispatchFormat::Rg16Float => "rg16-float",
        DispatchFormat::Rg8Unorm => "rg8-unorm",
        DispatchFormat::Rg32Uint => "rg32-uint",
        DispatchFormat::R32Float => "r32-float",
        DispatchFormat::Rgb10A2 => "rgb10a2",
        DispatchFormat::Rgba16Float => "rgba16-float",
        DispatchFormat::Rgba16Unorm => "rgba16-unorm",
        DispatchFormat::Float11_11_10 => "float11-11-10",
    }
}

const WRAPS: [(TextureWrap, &str); 5] = [
    (TextureWrap::Repeat, "repeat"),
    (TextureWrap::Mirror, "mirror"),
    (TextureWrap::ClampToEdge, "clamp-edge"),
    (TextureWrap::ClampHalfBorder, "clamp-half-border"),
    (TextureWrap::ClampBorder, "clamp-border"),
];

const FILTERS: [(TextureFilter, &str); 2] = [
    (TextureFilter::Nearest, "nearest"),
    (TextureFilter::Linear, "linear"),
];

const MIP_FILTERS: [(MipFilter, &str); 3] = [
    (MipFilter::None, "none"),
    (MipFilter::Nearest, "nearest"),
    (MipFilter::Linear, "linear"),
];

/// The name `table` gives `value`.
fn name_of<T: PartialEq + Copy>(table: &[(T, &'static str)], value: T) -> &'static str {
    table
        .iter()
        .find(|(v, _)| *v == value)
        .map_or("?", |(_, name)| name)
}

/// The value `table` names `name`.
fn value_of<T: Copy>(table: &[(T, &str)], name: &str) -> Option<T> {
    table.iter().find(|(_, n)| *n == name).map(|(v, _)| *v)
}

/// A sampler as one comma-separated field: wrap across and down, magnify, minify, mip, border,
/// anisotropy, depth compare, least and most level, level bias, mirror-once across and down.
fn sampling_text(sampling: &TextureSampling) -> String {
    let border = match sampling.border {
        BorderColour::TransparentBlack => "transparent-black".to_owned(),
        BorderColour::OpaqueBlack => "opaque-black".to_owned(),
        BorderColour::OpaqueWhite => "opaque-white".to_owned(),
        BorderColour::Table(entry) => format!("table:{entry}"),
        BorderColour::Custom([r, g, b, a]) => format!("custom:{r:#x}:{g:#x}:{b:#x}:{a:#x}"),
    };
    format!(
        "{},{},{},{},{},{border},{},{},{},{},{},{},{}",
        name_of(&WRAPS, sampling.wrap[0]),
        name_of(&WRAPS, sampling.wrap[1]),
        name_of(&FILTERS, sampling.magnify),
        name_of(&FILTERS, sampling.minify),
        name_of(&MIP_FILTERS, sampling.mip),
        sampling.anisotropy,
        sampling.depth_compare,
        sampling.min_lod,
        sampling.max_lod,
        sampling.lod_bias,
        u8::from(sampling.mirror_once[0]),
        u8::from(sampling.mirror_once[1]),
    )
}

/// A sampler from [`sampling_text`]'s field.
fn parse_sampling(text: &str) -> Option<TextureSampling> {
    let fields: Vec<&str> = text.split(',').collect();
    let [
        wrap_s,
        wrap_t,
        magnify,
        minify,
        mip,
        border,
        anisotropy,
        compare,
        min_lod,
        max_lod,
        bias,
        mirror_s,
        mirror_t,
    ] = fields.as_slice()
    else {
        return None;
    };
    let border = match *border {
        "transparent-black" => BorderColour::TransparentBlack,
        "opaque-black" => BorderColour::OpaqueBlack,
        "opaque-white" => BorderColour::OpaqueWhite,
        other => {
            if let Some(entry) = other.strip_prefix("table:") {
                BorderColour::Table(entry.parse().ok()?)
            } else {
                let parts: Vec<u32> = other
                    .strip_prefix("custom:")?
                    .split(':')
                    .map(|p| number(p).and_then(|p| u32::try_from(p).ok()))
                    .collect::<Option<_>>()?;
                BorderColour::Custom(parts.try_into().ok()?)
            }
        }
    };
    let flag = |s: &str| match s {
        "0" => Some(false),
        "1" => Some(true),
        _ => None,
    };
    Some(TextureSampling {
        wrap: [value_of(&WRAPS, wrap_s)?, value_of(&WRAPS, wrap_t)?],
        magnify: value_of(&FILTERS, magnify)?,
        minify: value_of(&FILTERS, minify)?,
        mip: value_of(&MIP_FILTERS, mip)?,
        border,
        anisotropy: anisotropy.parse().ok()?,
        depth_compare: compare.parse().ok()?,
        min_lod: min_lod.parse().ok()?,
        max_lod: max_lod.parse().ok()?,
        lod_bias: bias.parse().ok()?,
        mirror_once: [flag(mirror_s)?, flag(mirror_t)?],
    })
}

/// The most dispatches of one program `<addr>:all` captures: a program a frame runs several times
/// would otherwise fill the disk.
pub(crate) const EVERY_DISPATCH_MOST: u32 = 32;

/// Whether a dispatch is written before it is submitted, and under what name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Wanted {
    /// Not written.
    No,
    /// Written as its program's one capture, `dispatch-<addr>`.
    Once,
    /// Written as its program's capture with this number, from one, `dispatch-<addr>-<n>`.
    Numbered(u32),
    /// Not written: its program has had [`EVERY_DISPATCH_MOST`] written. Answered once, for the
    /// first dispatch past them, so the run says so.
    Capped,
}

impl Wanted {
    /// Whether the dispatch is written.
    pub(crate) const fn written(self) -> bool {
        matches!(self, Self::Once | Self::Numbered(_))
    }
}

/// The programs `ORBISTOUN_CAPTURE_DISPATCH` names, each with whether every dispatch of it is
/// wanted, and how many of each were written.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Selection {
    asked: Vec<(u64, bool)>,
    taken: Vec<(u64, u32)>,
}

impl Selection {
    /// The programs a value of `ORBISTOUN_CAPTURE_DISPATCH` names: addresses, comma-separated, in
    /// hexadecimal with `0x` or decimal, each followed by `:all` where every dispatch of it is
    /// wanted rather than its first.
    ///
    /// # Errors
    ///
    /// The entry that is not an address.
    pub(crate) fn parse(value: Option<&str>) -> Result<Self, String> {
        let asked = value
            .unwrap_or_default()
            .split(',')
            .map(str::trim)
            .filter(|entry| !entry.is_empty())
            .map(|entry| {
                let (address, every) = entry
                    .strip_suffix(":all")
                    .map_or((entry, false), |address| (address, true));
                number(address)
                    .map(|address| (address, every))
                    .ok_or_else(|| entry.to_owned())
            })
            .collect::<Result<_, _>>()?;
        Ok(Self {
            asked,
            taken: Vec::new(),
        })
    }

    /// Whether a dispatch of `program` is written before it is submitted: its program is named,
    /// and none of its dispatches was written yet, or every one is wanted and fewer than
    /// [`EVERY_DISPATCH_MOST`] were. Answering that it is written counts it.
    pub(crate) fn wants(&mut self, program: Option<u64>) -> Wanted {
        let Some(&(program, every)) =
            program.and_then(|program| self.asked.iter().find(|(asked, _)| *asked == program))
        else {
            return Wanted::No;
        };
        let at = self
            .taken
            .iter()
            .position(|(taken, _)| *taken == program)
            .unwrap_or_else(|| {
                self.taken.push((program, 0));
                self.taken.len() - 1
            });
        let taken = &mut self.taken[at].1;
        let most = if every { EVERY_DISPATCH_MOST } else { 1 };
        if *taken > most {
            return Wanted::No;
        }
        *taken += 1;
        match (*taken > most, every) {
            (true, false) => Wanted::No,
            (true, true) => Wanted::Capped,
            (false, false) => Wanted::Once,
            (false, true) => Wanted::Numbered(*taken),
        }
    }
}

/// Whether a dispatch that failed is written out for losing the device: the device was not lost
/// before it (`lost_before`), it is after (`lost_after`), and it was not written already
/// (`written`). A dispatch on a device another submission lost fails too, and is not the cause.
pub(crate) const fn written_for_loss(lost_before: bool, lost_after: bool, written: bool) -> bool {
    !lost_before && lost_after && !written
}

/// Whether `error` is the device's loss.
pub(crate) fn is_loss(error: &DispatchError) -> bool {
    matches!(error, DispatchError::Vulkan(_, result) if *result == ash::vk::Result::ERROR_DEVICE_LOST)
}

/// The directory a capture is written to: `dispatch-<program>`, or `dispatch-module-<hash>` for a
/// dispatch noted with none, then its number where it has one - `-w<n>` for a watched change,
/// `-b<n>` for a watched binding, `-r<ordinal>` for one kept from before a loss - and `-lost` for
/// a loss.
pub(crate) fn directory_name(
    program: Option<u64>,
    module_hash: u64,
    (reason, numbered): (Reason, Option<u64>),
) -> String {
    let named = program.map_or_else(
        || format!("module-{module_hash:016x}"),
        |p| format!("{p:x}"),
    );
    let mark = match reason {
        Reason::Watched => "w",
        Reason::Bound => "b",
        Reason::Recent => "r",
        Reason::Asked | Reason::DeviceLost => "",
    };
    let number = numbered.map(|n| format!("-{mark}{n}")).unwrap_or_default();
    let suffix = if reason == Reason::DeviceLost {
        "-lost"
    } else {
        ""
    };
    format!("dispatch-{named}{number}{suffix}")
}

/// The most dispatches a run captures for changing a watched range.
pub(crate) const WATCHED_MOST: u32 = 16;

/// Counts a capture in `taken`: its number from one up to `most`, then [`Taken::Capped`] once, so
/// the caller says the cap was reached, then [`Taken::Past`].
pub(crate) fn take_number(taken: &std::sync::atomic::AtomicU32, most: u32) -> Taken {
    let n = taken
        .fetch_update(
            std::sync::atomic::Ordering::Relaxed,
            std::sync::atomic::Ordering::Relaxed,
            |n| (n <= most).then_some(n + 1),
        )
        .map_or(most + 2, |before| before + 1);
    match n {
        n if n <= most => Taken::Number(n),
        n if n == most + 1 => Taken::Capped,
        _ => Taken::Past,
    }
}

/// What [`take_number`] answers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Taken {
    /// Captured under this number.
    Number(u32),
    /// Not captured: the first past the most.
    Capped,
    /// Not captured, and said before.
    Past,
}

/// The number the next capture for `reason` - a watched change or a watched binding, each counted
/// apart - takes, saying once when the run has had [`WATCHED_MOST`] of them.
pub(crate) fn next_watched(reason: Reason) -> Option<u32> {
    static WATCHED: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    static BOUND: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let (taken, what) = if reason == Reason::Bound {
        (&BOUND, "bound")
    } else {
        (&WATCHED, "changed")
    };
    match take_number(taken, WATCHED_MOST) {
        Taken::Number(n) => Some(n),
        Taken::Capped => {
            tracing::info!(
                "captured {WATCHED_MOST} dispatches that {what} a watched range, the most a run keeps; later ones are logged only"
            );
            None
        }
        Taken::Past => None,
    }
}

/// The next guest dispatch's place among the run's, from one.
pub(crate) fn next_ordinal() -> u64 {
    static DISPATCHES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    DISPATCHES.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1
}

/// The run's selection, read from the environment once. A value that names no address is said
/// once and selects nothing.
fn selection() -> &'static std::sync::Mutex<Selection> {
    static SELECTION: std::sync::OnceLock<std::sync::Mutex<Selection>> = std::sync::OnceLock::new();
    SELECTION.get_or_init(|| {
        let value = orbistoun_env::CAPTURE_DISPATCH.get();
        let selection = Selection::parse(value.as_deref()).unwrap_or_else(|entry| {
            tracing::warn!(
                "{} names {entry:?}, which is not an address; no dispatch is captured",
                orbistoun_env::CAPTURE_DISPATCH.name
            );
            Selection::default()
        });
        std::sync::Mutex::new(selection)
    })
}

/// Whether a dispatch of `program` is written before it is submitted ([`Selection::wants`]); the
/// first dispatch past a program's [`EVERY_DISPATCH_MOST`] is said.
pub(crate) fn asked(program: Option<u64>) -> Wanted {
    let wanted = selection()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .wants(program);
    if wanted == Wanted::Capped
        && let Some(program) = program
    {
        tracing::info!(
            "captured {EVERY_DISPATCH_MOST} dispatches of the program at {program:#x}, the most {} keeps of one; its later dispatches are not captured",
            orbistoun_env::CAPTURE_DISPATCH.name
        );
    }
    wanted
}

/// Where the run keeps its frames, and so its captures.
pub(crate) fn frame_dir() -> PathBuf {
    orbistoun_env::FRAME_DIR
        .get()
        .map_or_else(std::env::temp_dir, PathBuf::from)
}

/// Writes `capture` of the module hashed `module_hash`, numbered where its directory takes a
/// number, where the run keeps its frames, and says where, or why not.
pub(crate) fn write(capture: &Capture<'_>, module_hash: u64, numbered: Option<u64>) {
    let dir = frame_dir().join(directory_name(
        capture.program,
        module_hash,
        (capture.reason, numbered),
    ));
    let named = capture.program.map_or_else(
        || "a module".to_owned(),
        |p| format!("the program at {p:#x}"),
    );
    match capture.write_to(&dir) {
        Ok(()) => tracing::info!(
            "captured the run's dispatch {} of {named} over {:?} groups ({}, {} bytes) at {}; orbistoun-cli replay-dispatch runs it alone",
            capture.ordinal.unwrap_or_default(),
            capture.groups,
            capture.reason.name(),
            capture.bytes(),
            dir.display()
        ),
        Err(e) => tracing::warn!(
            "the dispatch of {named} could not be captured at {}: {e}",
            dir.display()
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        Capture, CapturedBuffer, CapturedImage, EVERY_DISPATCH_MOST, ImageRole, Reason, Selection,
        Taken, Wanted, directory_name, take_number, written_for_loss,
    };
    use crate::compute_images::DispatchFormat;
    use orbistoun_gpu::bound_ranges::BoundRanges;
    use orbistoun_gpu::{BorderColour, MipFilter, TextureFilter, TextureSampling, TextureWrap};
    use std::borrow::Cow;
    use std::collections::HashMap;

    fn image(role: ImageRole, format: DispatchFormat) -> CapturedImage<'static> {
        CapturedImage {
            role,
            texels: Cow::Owned(vec![1, 2, 3, 4, 5, 6]),
            format,
            width: 3,
            height: 2,
            depth: None,
            layers: None,
            sampling: None,
            cube: false,
        }
    }

    /// A capture holding one of everything a dispatch binds.
    fn everything() -> Capture<'static> {
        let sampled = TextureSampling {
            wrap: [TextureWrap::Mirror, TextureWrap::ClampHalfBorder],
            magnify: TextureFilter::Linear,
            minify: TextureFilter::Nearest,
            mip: MipFilter::None,
            border: BorderColour::Custom([0x3f80_0000, 0, 0xbf80_0000, 0x7fc0_0000]),
            anisotropy: 16,
            depth_compare: 3,
            min_lod: 0x100,
            max_lod: 0xf00,
            lod_bias: -0x80,
            mirror_once: [true, false],
        };
        Capture {
            program: Some(0x3dc_3400),
            reason: Reason::DeviceLost,
            module: Cow::Owned(vec![0x0723_0203, 0x0001_0500, 7]),
            memory: Cow::Owned((0..1000).collect()),
            push: Cow::Owned(vec![0xdead_beef, 0, u32::MAX]),
            groups: [64, 3, 1],
            images: vec![
                CapturedImage {
                    sampling: Some(sampled),
                    cube: true,
                    layers: Some(6),
                    texels: Cow::Owned(vec![9; 3 * 2 * 6]),
                    ..image(ImageRole::Fetched, DispatchFormat::Rgba16Float)
                },
                CapturedImage {
                    sampling: Some(TextureSampling {
                        border: BorderColour::Table(12),
                        ..TextureSampling::CLAMPED_POINT
                    }),
                    depth: Some(4),
                    ..image(ImageRole::MoreFetched, DispatchFormat::Float11_11_10)
                },
                image(ImageRole::MoreFetched, DispatchFormat::R8),
                image(ImageRole::Stored, DispatchFormat::Rgba8Uint),
            ],
            buffers: vec![
                CapturedBuffer::Words(Cow::Owned(vec![5, 6, 7])),
                CapturedBuffer::Unread(1 << 20),
                CapturedBuffer::Alias(0, 0x100),
            ],
            ranges: BoundRanges {
                window: Some((0x4000_0187_0000, 4000)),
                buffers: vec![
                    (0x4000_0200_0000, 12),
                    (0x4000_0300_0000, 4 << 20),
                    (0x4000_0200_0100, 4),
                ],
            },
            ordinal: Some(4127),
        }
    }

    fn saved(capture: &Capture<'_>) -> HashMap<String, Vec<u8>> {
        let mut files = HashMap::new();
        capture
            .save(&mut |name, bytes| {
                files.insert(name.to_owned(), bytes.to_vec());
                Ok(())
            })
            .expect("saved");
        files
    }

    fn loaded(files: &HashMap<String, Vec<u8>>) -> Result<Capture<'static>, String> {
        Capture::load(&mut |name| {
            files
                .get(name)
                .cloned()
                .ok_or_else(|| std::io::ErrorKind::NotFound.into())
        })
        .map_err(|e| e.to_string())
    }

    /// Everything a capture holds comes back from its files as it was.
    #[test]
    fn a_capture_reads_back_as_it_was_written() {
        let capture = everything();
        assert_eq!(loaded(&saved(&capture)), Ok(capture));
    }

    /// A dispatch noted with no program, with no images, buffers or user data, reads back too.
    #[test]
    fn a_bare_capture_reads_back_as_it_was_written() {
        let capture = Capture {
            program: None,
            reason: Reason::Asked,
            push: Cow::Owned(Vec::new()),
            images: Vec::new(),
            buffers: Vec::new(),
            ranges: BoundRanges::default(),
            ordinal: None,
            ..everything()
        };
        assert_eq!(loaded(&saved(&capture)), Ok(capture));
    }

    /// The manifest says where each binding lies in guest memory, in a form a reader can take.
    #[test]
    fn the_manifest_places_each_binding_in_guest_memory() {
        let files = saved(&everything());
        let manifest = String::from_utf8(files[super::MANIFEST].clone()).expect("text");
        for line in [
            "ordinal 4127",
            "memory memory.bin at 0x400001870000 bytes 0xfa0",
            "buffer words buffer-0.bin at 0x400002000000 bytes 0xc",
            "buffer unread 1048576 at 0x400003000000 bytes 0x400000",
            "buffer alias 0 0x100 at 0x400002000100 bytes 0x4",
        ] {
            assert!(manifest.lines().any(|l| l == line), "{line} in {manifest}");
        }
    }

    /// Only what is bound is written: an unread buffer and an alias have no file, and the bytes
    /// counted are the files'.
    #[test]
    fn only_what_is_bound_is_written() {
        let capture = everything();
        let files = saved(&capture);
        let mut names: Vec<&str> = files.keys().map(String::as_str).collect();
        names.sort_unstable();
        assert_eq!(
            names,
            [
                "buffer-0.bin",
                "dispatch.txt",
                "image-0.bin",
                "image-1.bin",
                "image-2.bin",
                "image-3.bin",
                "memory.bin",
                "module.spv",
            ]
        );
        let written: usize = files
            .iter()
            .filter(|(name, _)| *name != super::MANIFEST)
            .map(|(_, bytes)| bytes.len())
            .sum();
        assert_eq!(capture.bytes(), written as u64);
        assert_eq!(
            files["memory.bin"][4..8],
            1u32.to_le_bytes(),
            "little-endian"
        );
    }

    /// A manifest naming a file outside its directory, a file of part-words, an unknown line or no
    /// module is refused rather than read.
    #[test]
    fn a_damaged_capture_is_refused() {
        let files = saved(&everything());
        let with_manifest = |manifest: &str| {
            let mut files = files.clone();
            files.insert(super::MANIFEST.to_owned(), manifest.as_bytes().to_vec());
            loaded(&files)
        };
        assert!(with_manifest("module ../module.spv").is_err());
        assert!(with_manifest("module module.spv\nwhatever 1").is_err());
        assert!(with_manifest("memory memory.bin").is_err(), "no module");
        assert!(with_manifest("module module.spv\ngroups 1 1").is_err());
        assert!(
            with_manifest("module module.spv\nbuffer unread 4 at 0x10 bytes many").is_err(),
            "a range that is not numbers"
        );
        assert!(
            with_manifest("module module.spv\nbuffer unread 4 at 0x10 bytes 16\nbuffer unread 4")
                .is_err(),
            "some buffers placed and not others"
        );
        assert!(with_manifest("module module.spv at 0x10 bytes 16").is_err());
        let mut torn = files.clone();
        torn.get_mut("memory.bin").expect("written").pop();
        assert!(loaded(&torn).is_err(), "part of a word");
    }

    /// A named program's first dispatch is captured and its later ones are not; a program not
    /// named, and a dispatch noted with none, never are.
    #[test]
    fn a_named_program_is_captured_once() {
        let mut selection = Selection::parse(Some(" 0x3dc3400, 4096 ,")).expect("two addresses");
        assert_eq!(selection.wants(None), Wanted::No);
        assert_eq!(selection.wants(Some(0x1234)), Wanted::No);
        assert_eq!(selection.wants(Some(0x3dc_3400)), Wanted::Once);
        assert_eq!(
            selection.wants(Some(0x3dc_3400)),
            Wanted::No,
            "only the first"
        );
        assert_eq!(selection.wants(Some(0x3dc_3400)), Wanted::No);
        assert_eq!(selection.wants(Some(4096)), Wanted::Once);
        assert_eq!(Selection::parse(None), Ok(Selection::default()));
        assert_eq!(
            Selection::parse(Some("0x10,later")),
            Err("later".to_owned())
        );
        assert_eq!(
            Selection::parse(Some("0x10:every")),
            Err("0x10:every".to_owned())
        );
    }

    /// A program named with `:all` has each dispatch captured, numbered from one, up to the most;
    /// the first past them is answered capped, once, and the rest not at all. Another program
    /// named beside it keeps its own count.
    #[test]
    fn every_dispatch_of_a_program_is_captured_up_to_the_most() {
        let mut selection =
            Selection::parse(Some("0x400003ce6000:all,0x400003dc3400")).expect("two programs");
        for n in 1..=EVERY_DISPATCH_MOST {
            assert_eq!(selection.wants(Some(0x4000_03ce_6000)), Wanted::Numbered(n));
            if n == 2 {
                assert_eq!(selection.wants(Some(0x4000_03dc_3400)), Wanted::Once);
            }
        }
        assert_eq!(selection.wants(Some(0x4000_03ce_6000)), Wanted::Capped);
        assert_eq!(selection.wants(Some(0x4000_03ce_6000)), Wanted::No);
        assert_eq!(selection.wants(Some(0x4000_03ce_6000)), Wanted::No);
        assert!(Wanted::Numbered(1).written() && Wanted::Once.written());
        assert!(!Wanted::Capped.written() && !Wanted::No.written());
    }

    /// The dispatch that lost the device is captured; one that failed on a device already lost,
    /// one that failed otherwise, and one already captured are not.
    #[test]
    fn only_the_dispatch_that_lost_the_device_is_captured_for_it() {
        assert!(written_for_loss(false, true, false));
        assert!(!written_for_loss(true, true, false), "lost before it");
        assert!(!written_for_loss(false, false, false), "not a loss");
        assert!(!written_for_loss(false, true, true), "already captured");
    }

    /// A capture's directory names its program, or its module where it has none, and a loss.
    #[test]
    fn a_capture_directory_names_what_it_holds() {
        assert_eq!(
            directory_name(Some(0x3dc_3400), 7, (Reason::Asked, None)),
            "dispatch-3dc3400"
        );
        assert_eq!(
            directory_name(Some(0x3dc_3400), 7, (Reason::Asked, Some(12))),
            "dispatch-3dc3400-12"
        );
        assert_eq!(
            directory_name(None, 0xab, (Reason::DeviceLost, None)),
            "dispatch-module-00000000000000ab-lost"
        );
        assert_eq!(
            directory_name(Some(0x3ce_6000), 7, (Reason::Watched, Some(3))),
            "dispatch-3ce6000-w3"
        );
        assert_eq!(
            directory_name(Some(0x3ce_6000), 7, (Reason::Bound, Some(16))),
            "dispatch-3ce6000-b16"
        );
    }

    /// Captures are numbered from one up to the most, the first past them is answered capped once,
    /// and the rest past.
    #[test]
    fn numbered_captures_stop_at_the_most() {
        let taken = std::sync::atomic::AtomicU32::new(0);
        assert_eq!(take_number(&taken, 2), Taken::Number(1));
        assert_eq!(take_number(&taken, 2), Taken::Number(2));
        assert_eq!(take_number(&taken, 2), Taken::Capped);
        assert_eq!(take_number(&taken, 2), Taken::Past);
        assert_eq!(take_number(&taken, 2), Taken::Past);
    }

    /// A capture for a watch reads back with its reason.
    #[test]
    fn a_watched_capture_reads_back_with_its_reason() {
        for reason in [Reason::Watched, Reason::Bound] {
            let capture = Capture {
                reason,
                ..everything()
            };
            assert_eq!(loaded(&saved(&capture)), Ok(capture));
        }
    }
}
