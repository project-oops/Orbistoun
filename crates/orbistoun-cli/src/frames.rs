//! A run's sheet of frames (D736): the frames `ORBISTOUN_FRAME_EVERY` kept, in flip order, each
//! shrunk to a tile and laid out left to right, top to bottom.

use anyhow::{Context, Result};

/// Tiles to a row.
const COLUMNS: u32 = 9;
/// A tile's size: a sixteenth of a 1920x1080 frame, whatever size the frame was.
const TILE: (u32, u32) = (120, 68);
/// The gap between tiles, and around them.
const GAP: u32 = 2;
/// The gap's colour.
const BACKGROUND: [u8; 3] = [0x20, 0x20, 0x20];

/// One kept frame: the flip it was kept before, its size, and where its `Rgba8` bytes are.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Kept {
    pub(crate) flip: u64,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) path: std::path::PathBuf,
}

/// The frames kept in `traces`, in flip order: every `flip-<n>-<w>x<h>.rgba` the worker wrote.
///
/// # Errors
///
/// When `traces` cannot be read.
pub(crate) fn kept(traces: &std::path::Path) -> Result<Vec<Kept>> {
    let entries = match std::fs::read_dir(traces) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e).with_context(|| format!("reading {}", traces.display())),
    };
    let mut kept: Vec<Kept> = entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            let (flip, size) = name
                .strip_prefix("flip-")?
                .strip_suffix(".rgba")?
                .split_once('-')?;
            let (width, height) = size.split_once('x')?;
            Some(Kept {
                flip: flip.parse().ok()?,
                width: width.parse().ok()?,
                height: height.parse().ok()?,
                path: entry.path(),
            })
        })
        .collect();
    kept.sort_by_key(|frame| frame.flip);
    Ok(kept)
}

/// The most distinct colours [`light`] counts: past it, a frame is plainly a picture.
pub(crate) const MOST_COLOURS: usize = 4096;

/// How much of a kept frame shows anything: its pixels, those whose colour is not black - the
/// alpha aside, since a flipped frame's is often zero - and its distinct colours, counted as far as
/// [`MOST_COLOURS`]; the one colour, where the frame has only one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Light {
    pub(crate) pixels: u64,
    pub(crate) lit: u64,
    pub(crate) colours: usize,
    pub(crate) only: Option<[u8; 3]>,
}

/// Whether a frame shows next to nothing: one colour, or under one pixel in a thousand lit - a black
/// screen with a few stray pixels.
pub(crate) const fn dark(light: &Light) -> bool {
    light.colours <= 1 || light.lit * 1000 < light.pixels
}

/// The flip from which every one of `lights` shows what the last does - as many pixels lit, as many
/// colours: since when a run's picture has stood still. `None` for no frames.
pub(crate) fn same_since(lights: &[(u64, Light)]) -> Option<u64> {
    let (_, last) = lights.last()?;
    lights
        .iter()
        .rev()
        .take_while(|(_, light)| light.lit == last.lit && light.colours == last.colours)
        .last()
        .map(|&(flip, _)| flip)
}

/// The [`Light`] of a frame's `Rgba8` bytes.
pub(crate) fn light(bytes: &[u8]) -> Light {
    let mut colours: std::collections::HashSet<[u8; 3]> = std::collections::HashSet::new();
    let (mut pixels, mut lit) = (0, 0);
    for pixel in bytes.chunks_exact(4) {
        let colour = [pixel[0], pixel[1], pixel[2]];
        pixels += 1;
        if colour != [0; 3] {
            lit += 1;
        }
        if colours.len() < MOST_COLOURS {
            colours.insert(colour);
        }
    }
    let only = (colours.len() == 1)
        .then(|| colours.iter().copied().next())
        .flatten();
    Light {
        pixels,
        lit,
        colours: colours.len(),
        only,
    }
}

/// `count` of `kept`, spaced evenly from the first to the last: the whole run at a glance
/// whatever its length. All of them when there are no more than `count`.
pub(crate) fn spread(kept: &[Kept], count: usize) -> Vec<Kept> {
    if kept.len() <= count || count == 0 {
        return kept.to_vec();
    }
    if count == 1 {
        return kept.first().cloned().into_iter().collect();
    }
    let last = kept.len() - 1;
    (0..count)
        .filter_map(|i| kept.get(i * last / (count - 1)).cloned())
        .collect()
}

/// Where a run of `module` keeps its frames: a directory of its own under `traces`, named by its
/// title. The traces directory is shared by every run on the machine, and a sheet made from
/// it took in frames another title's run, in another session, kept there at the same time.
pub(crate) fn run_dir(traces: &std::path::Path, module: &std::path::Path) -> std::path::PathBuf {
    let title = orbistoun_report::trace::title_stem(&module.to_string_lossy());
    traces.join("frames").join(title)
}

/// The directory a run's frames are read from for its sheet: the one `ORBISTOUN_FRAME_DIR` names,
/// which [`crate::run`] points at the run's own ([`run_dir`]), or else `traces`.
pub(crate) fn kept_dir(traces: &std::path::Path) -> std::path::PathBuf {
    orbistoun_env::FRAME_DIR
        .get()
        .map_or_else(|| traces.to_path_buf(), std::path::PathBuf::from)
}

/// Removes every kept frame from `traces`, so a run's sheet holds that run's frames alone.
///
/// # Errors
///
/// When a kept frame cannot be removed.
pub(crate) fn clear(traces: &std::path::Path) -> Result<()> {
    for frame in kept(traces)? {
        std::fs::remove_file(&frame.path)
            .with_context(|| format!("removing {}", frame.path.display()))?;
    }
    Ok(())
}

/// Lays `frames` out as a sheet: each an `Rgba8` frame of its own size, shrunk to a tile by
/// nearest sampling, in order. `None` when there are none.
pub(crate) fn sheet(frames: &[(u32, u32, &[u8])]) -> Option<image::RgbImage> {
    if frames.is_empty() {
        return None;
    }
    let count = u32::try_from(frames.len()).ok()?;
    let rows = count.div_ceil(COLUMNS);
    let columns = count.min(COLUMNS);
    let width = GAP + columns * (TILE.0 + GAP);
    let height = GAP + rows * (TILE.1 + GAP);
    let mut sheet = image::RgbImage::from_pixel(width, height, image::Rgb(BACKGROUND));
    for (index, &(frame_width, frame_height, bytes)) in (0u32..).zip(frames) {
        let left = GAP + (index % COLUMNS) * (TILE.0 + GAP);
        let top = GAP + (index / COLUMNS) * (TILE.1 + GAP);
        for y in 0..TILE.1 {
            for x in 0..TILE.0 {
                let from_x = u64::from(x) * u64::from(frame_width) / u64::from(TILE.0);
                let from_y = u64::from(y) * u64::from(frame_height) / u64::from(TILE.1);
                let at = usize::try_from((from_y * u64::from(frame_width) + from_x) * 4).ok()?;
                let pixel = bytes.get(at..at + 3).unwrap_or(&BACKGROUND);
                sheet.put_pixel(
                    left + x,
                    top + y,
                    image::Rgb([pixel[0], pixel[1], pixel[2]]),
                );
            }
        }
    }
    Some(sheet)
}

/// Writes the sheet of `kept` to `out` as a PNG, answering how many frames it holds.
///
/// # Errors
///
/// When a frame cannot be read or the sheet cannot be written.
pub(crate) fn write_sheet(kept: &[Kept], out: &std::path::Path) -> Result<usize> {
    let bytes: Vec<(u32, u32, Vec<u8>)> = kept
        .iter()
        .map(|frame| {
            std::fs::read(&frame.path)
                .with_context(|| format!("reading {}", frame.path.display()))
                .map(|bytes| (frame.width, frame.height, bytes))
        })
        .collect::<Result<_>>()?;
    let frames: Vec<(u32, u32, &[u8])> = bytes
        .iter()
        .map(|(width, height, bytes)| (*width, *height, bytes.as_slice()))
        .collect();
    let Some(sheet) = sheet(&frames) else {
        anyhow::bail!("the run kept no frames, so there is no sheet to write");
    };
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
    }
    sheet
        .save_with_format(out, image::ImageFormat::Png)
        .with_context(|| format!("writing {}", out.display()))?;
    Ok(frames.len())
}

/// A frame the console flipped, by the guest's flip index: what a kept frame of the same index is
/// compared with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Reference {
    pub(crate) flip: u64,
    pub(crate) width: u32,
    pub(crate) height: u32,
    /// `Rgba8`, rows top to bottom, no padding.
    pub(crate) rgba: Vec<u8>,
}

/// The reference frames in `dir`: every `flip-<n>-<w>x<h>.rgba` (the kept-frame layout) and every
/// `flip-<n>.png`, in flip order. Two references for one flip are refused, since which one a
/// comparison used could not be told afterwards.
///
/// # Errors
///
/// When `dir` or a frame cannot be read, a raw frame's length is not its extent's, or a flip has
/// two references.
pub(crate) fn references(dir: &std::path::Path) -> Result<Vec<Reference>> {
    let mut references = Vec::new();
    for frame in kept(dir)? {
        let rgba = std::fs::read(&frame.path)
            .with_context(|| format!("reading {}", frame.path.display()))?;
        let expected = u64::from(frame.width) * u64::from(frame.height) * 4;
        anyhow::ensure!(
            u64::try_from(rgba.len())? == expected,
            "{} holds {} bytes, and a {}x{} Rgba8 frame is {expected}",
            frame.path.display(),
            rgba.len(),
            frame.width,
            frame.height
        );
        references.push(Reference {
            flip: frame.flip,
            width: frame.width,
            height: frame.height,
            rgba,
        });
    }
    let entries = std::fs::read_dir(dir).with_context(|| format!("reading {}", dir.display()))?;
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let Some(flip) = name
            .strip_prefix("flip-")
            .and_then(|rest| rest.strip_suffix(".png"))
            .and_then(|flip| flip.parse::<u64>().ok())
        else {
            continue;
        };
        let image = image::open(entry.path())
            .with_context(|| format!("reading {}", entry.path().display()))?
            .into_rgba8();
        references.push(Reference {
            flip,
            width: image.width(),
            height: image.height(),
            rgba: image.into_raw(),
        });
    }
    references.sort_by_key(|reference| reference.flip);
    if let Some(pair) = references
        .windows(2)
        .find(|pair| pair[0].flip == pair[1].flip)
    {
        anyhow::bail!(
            "{} holds two references for flip {}",
            dir.display(),
            pair[0].flip
        );
    }
    Ok(references)
}

/// How a kept frame compares with the console's frame of the same flip.
///
/// Colour only: a presented frame is shown opaque (D796), so alpha never reaches the picture and
/// is not compared.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Comparison {
    pub(crate) pixels: u64,
    /// Pixels whose every colour channel is within the tolerance of the reference's.
    pub(crate) matched: u64,
    /// The mean absolute difference over every colour channel of every pixel, in 8-bit steps.
    pub(crate) mean_error: f64,
    /// The largest absolute difference in any colour channel.
    pub(crate) max_error: u8,
}

impl Comparison {
    /// Whether every pixel matched.
    pub(crate) fn exact(&self) -> bool {
        self.matched == self.pixels
    }
}

/// Compares two `Rgba8` frames of one extent, pixel by pixel, counting a pixel as matched when no
/// colour channel differs by more than `tolerance`. `None` when the lengths differ or are not
/// whole pixels, since frames of different extents have no pixel-for-pixel answer.
pub(crate) fn compare(kept: &[u8], reference: &[u8], tolerance: u8) -> Option<Comparison> {
    if kept.len() != reference.len() || kept.len() % 4 != 0 {
        return None;
    }
    let (mut matched, mut total, mut max_error) = (0u64, 0u64, 0u8);
    for (ours, theirs) in kept.chunks_exact(4).zip(reference.chunks_exact(4)) {
        let worst = ours[..3]
            .iter()
            .zip(&theirs[..3])
            .map(|(a, b)| {
                let difference = a.abs_diff(*b);
                total += u64::from(difference);
                difference
            })
            .max()
            .unwrap_or(0);
        max_error = max_error.max(worst);
        if worst <= tolerance {
            matched += 1;
        }
    }
    let pixels = u64::try_from(kept.len() / 4).ok()?;
    #[expect(
        clippy::cast_precision_loss,
        reason = "a mean over at most a few hundred million channels, reported to two places"
    )]
    let mean_error = if pixels == 0 {
        0.0
    } else {
        total as f64 / (pixels * 3) as f64
    };
    Some(Comparison {
        pixels,
        matched,
        mean_error,
        max_error,
    })
}

/// The per-channel absolute difference between two `Rgba8` frames of one extent, as a picture:
/// black where they agree. `None` when the lengths are not `width * height` pixels each.
pub(crate) fn difference(
    width: u32,
    height: u32,
    kept: &[u8],
    reference: &[u8],
) -> Option<image::RgbImage> {
    let expected = usize::try_from(u64::from(width) * u64::from(height) * 4).ok()?;
    if kept.len() != expected || reference.len() != expected {
        return None;
    }
    let pixels: Vec<u8> = kept
        .chunks_exact(4)
        .zip(reference.chunks_exact(4))
        .flat_map(|(ours, theirs)| {
            [
                ours[0].abs_diff(theirs[0]),
                ours[1].abs_diff(theirs[1]),
                ours[2].abs_diff(theirs[2]),
            ]
        })
        .collect();
    image::RgbImage::from_raw(width, height, pixels)
}

#[cfg(test)]
mod tests {
    /// Matching counts a pixel only when every colour channel is within the tolerance, alpha is
    /// ignored, and the error figures are over colour channels alone.
    #[test]
    fn a_comparison_counts_colour_within_the_tolerance_and_ignores_alpha() {
        let reference = [10, 20, 30, 255, 0, 0, 0, 0, 100, 100, 100, 7];
        let kept = [10, 20, 30, 0, 2, 0, 0, 255, 100, 100, 110, 7];
        let exact = super::compare(&kept, &reference, 0).expect("same extent");
        assert_eq!(
            (exact.pixels, exact.matched),
            (3, 1),
            "alpha alone differs in pixel 0"
        );
        assert_eq!(exact.max_error, 10);
        assert!(
            (exact.mean_error - 12.0 / 9.0).abs() < 1e-9,
            "{}",
            exact.mean_error
        );
        assert!(!exact.exact());
        let loose = super::compare(&kept, &reference, 2).expect("same extent");
        assert_eq!(
            loose.matched, 2,
            "a difference of 2 is within a tolerance of 2"
        );
        assert!(super::compare(&kept, &reference, 10).expect("same").exact());
        assert!(
            super::compare(&kept[..8], &reference, 0).is_none(),
            "extents differ"
        );
        assert!(
            super::compare(&kept[..7], &reference[..7], 0).is_none(),
            "not whole pixels"
        );
    }

    /// A frame's light counts the pixels whose colour is not black, its alpha aside - a flipped
    /// frame's alpha is often zero - and the distinct colours, as far as the cap; a frame of one
    /// colour says so, black or not.
    #[test]
    fn a_frames_light_counts_lit_pixels_and_colours() {
        use super::{Light, light};
        let pixel = |rgba: [u8; 4]| rgba.to_vec();
        let mut bytes = Vec::new();
        bytes.extend(pixel([0, 0, 0, 0]));
        bytes.extend(pixel([0, 0, 0, 0xff]));
        bytes.extend(pixel([0xff, 0xff, 0, 0]));
        bytes.extend(pixel([0xff, 0xff, 0, 0xff]));
        assert_eq!(
            light(&bytes),
            Light {
                pixels: 4,
                lit: 2,
                colours: 2,
                only: None,
            }
        );
        let grey: Vec<u8> = [0x40, 0x40, 0x40, 0x12].repeat(16);
        assert_eq!(light(&grey).only, Some([0x40, 0x40, 0x40]));
        assert_eq!(light(&grey).colours, 1);
        let many: Vec<u8> = (0..=u16::MAX)
            .flat_map(|i| {
                let [low, high] = i.to_le_bytes();
                [low, high, 1, 0]
            })
            .collect();
        assert_eq!(light(&many).colours, super::MOST_COLOURS);
        // Dark: one colour, or under one pixel in a thousand lit.
        assert!(super::dark(&light(&grey)));
        assert!(!super::dark(&light(&bytes)));
        let mut sliver = [0, 0, 0, 0xff].repeat(2000);
        sliver[0] = 0xff;
        assert!(super::dark(&light(&sliver)));
    }

    /// A run's picture is unchanged from the first of its last frames that light as the last one
    /// does: a run that moves past black says when it stopped moving, and one that never moves
    /// says its first frame.
    #[test]
    fn a_run_says_since_when_its_picture_is_unchanged() {
        let shows = |lit, colours| super::Light {
            pixels: 100,
            lit,
            colours,
            only: None,
        };
        let moving = [
            (1, shows(0, 1)),
            (40, shows(5, 3)),
            (80, shows(9, 7)),
            (120, shows(9, 7)),
        ];
        assert_eq!(super::same_since(&moving), Some(80));
        let still = [(1, shows(0, 1)), (40, shows(0, 1))];
        assert_eq!(super::same_since(&still), Some(1));
        assert_eq!(super::same_since(&[]), None);
    }

    /// The difference picture is black where the frames agree and holds the per-channel distance
    /// where they do not.
    #[test]
    fn the_difference_picture_is_black_where_frames_agree() {
        let reference = [10, 20, 30, 255, 0, 0, 0, 0];
        let kept = [10, 20, 30, 0, 5, 0, 9, 255];
        let picture = super::difference(2, 1, &kept, &reference).expect("2x1");
        assert_eq!(picture.get_pixel(0, 0).0, [0, 0, 0]);
        assert_eq!(picture.get_pixel(1, 0).0, [5, 0, 9]);
        assert!(super::difference(3, 1, &kept, &reference).is_none());
    }

    /// Two titles' runs keep their frames apart: a frame another title's run kept at the same time
    /// is not among this run's, as PPSA25872's publisher logo was in Neverputt's sheet when both
    /// kept their frames in the shared traces directory.
    #[test]
    fn another_titles_run_keeps_its_frames_apart() {
        let traces = tempfile::tempdir().expect("a temporary directory");
        let ours = super::run_dir(
            traces.path(),
            std::path::Path::new("homebrew/NVPT00001/eboot.bin"),
        );
        let theirs = super::run_dir(
            traces.path(),
            std::path::Path::new("titles/PPSA25872-app0/eboot.bin"),
        );
        for dir in [&ours, &theirs] {
            std::fs::create_dir_all(dir).expect("creates");
        }
        std::fs::write(theirs.join("flip-000031-2x1.rgba"), [1u8; 8]).expect("writes");
        super::clear(&ours).expect("clears");
        assert!(
            super::kept(&ours).expect("reads").is_empty(),
            "none of theirs"
        );
        assert_eq!(super::kept(&theirs).expect("reads").len(), 1, "theirs kept");
    }

    /// References are read from raw kept-frame files and from PNGs, ordered by flip, and a flip
    /// with two references, or a raw file of the wrong length, is refused.
    #[test]
    fn references_are_read_from_raw_frames_and_pngs() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        std::fs::write(dir.path().join("flip-000030-2x1.rgba"), [1u8; 8]).expect("writes");
        image::RgbaImage::from_raw(1, 1, vec![9, 8, 7, 6])
            .expect("1x1")
            .save(dir.path().join("flip-1.png"))
            .expect("writes a png");
        std::fs::write(dir.path().join("notes.txt"), b"ignored").expect("writes");
        let references = super::references(dir.path()).expect("reads");
        let summary: Vec<(u64, u32, u32, usize)> = references
            .iter()
            .map(|r| (r.flip, r.width, r.height, r.rgba.len()))
            .collect();
        assert_eq!(summary, [(1, 1, 1, 4), (30, 2, 1, 8)]);
        assert_eq!(references[0].rgba, [9, 8, 7, 6]);

        std::fs::write(dir.path().join("flip-30.png"), b"").expect("writes");
        assert!(
            super::references(dir.path()).is_err(),
            "an unreadable png is an error"
        );
        std::fs::remove_file(dir.path().join("flip-30.png")).expect("removes");
        image::RgbaImage::from_raw(1, 1, vec![0; 4])
            .expect("1x1")
            .save(dir.path().join("flip-30.png"))
            .expect("writes a png");
        let error = super::references(dir.path()).expect_err("two references for flip 30");
        assert!(error.to_string().contains("flip 30"), "{error}");

        let short = tempfile::tempdir().expect("a temporary directory");
        std::fs::write(short.path().join("flip-000001-2x2.rgba"), [0u8; 8]).expect("writes");
        assert!(
            super::references(short.path()).is_err(),
            "8 bytes is not a 2x2 frame"
        );
    }

    /// Frames are tiled in order, nine to a row, each shrunk to a tile whatever its size.
    #[test]
    fn frames_are_tiled_in_order_nine_to_a_row() {
        let solid = |width: u32, height: u32, shade: u8| {
            (width, height, vec![shade; (width * height * 4) as usize])
        };
        let frames: Vec<(u32, u32, Vec<u8>)> = (0..10u8)
            .map(|i| {
                if i == 3 {
                    solid(780, 438, i * 20)
                } else {
                    solid(1920, 1080, i * 20)
                }
            })
            .collect();
        let borrowed: Vec<(u32, u32, &[u8])> = frames
            .iter()
            .map(|(w, h, b)| (*w, *h, b.as_slice()))
            .collect();
        let sheet = super::sheet(&borrowed).expect("a sheet");
        let (tile, gap) = (super::TILE, super::GAP);
        assert_eq!(
            sheet.dimensions(),
            (gap + 9 * (tile.0 + gap), gap + 2 * (tile.1 + gap)),
            "nine columns, two rows"
        );
        let at = |index: u32| {
            let left = gap + (index % 9) * (tile.0 + gap);
            let top = gap + (index / 9) * (tile.1 + gap);
            sheet.get_pixel(left + tile.0 / 2, top + tile.1 / 2).0[0]
        };
        for index in 0..10u8 {
            assert_eq!(at(u32::from(index)), index * 20, "tile {index} in order");
        }
        assert!(super::sheet(&[]).is_none(), "no frames, no sheet");
    }

    /// A spread keeps the first and the last and spaces the rest evenly between them.
    #[test]
    fn a_spread_covers_the_run_from_first_to_last() {
        let kept: Vec<super::Kept> = (0..34u64)
            .map(|i| super::Kept {
                flip: i * 60,
                width: 1,
                height: 1,
                path: std::path::PathBuf::new(),
            })
            .collect();
        let flips = |shown: Vec<super::Kept>| shown.iter().map(|k| k.flip).collect::<Vec<_>>();
        let ten = flips(super::spread(&kept, 10));
        assert_eq!(ten.len(), 10);
        assert_eq!((ten[0], ten[9]), (0, 33 * 60), "first and last");
        assert!(
            ten.windows(2).all(|pair| pair[0] < pair[1]),
            "in order: {ten:?}"
        );
        assert_eq!(
            flips(super::spread(&kept[..4], 10)).len(),
            4,
            "fewer than asked: all"
        );
    }

    /// The kept frames are found by name and ordered by flip, not by name.
    #[test]
    fn kept_frames_are_ordered_by_flip() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        for name in [
            "flip-000060-1920x1080.rgba",
            "flip-000000-780x438.rgba",
            "latest-drawn-frame.rgba",
            "flip-bad-1x1.rgba",
        ] {
            std::fs::write(dir.path().join(name), b"").expect("writes");
        }
        let kept = super::kept(dir.path()).expect("reads");
        let order: Vec<(u64, u32)> = kept.iter().map(|k| (k.flip, k.width)).collect();
        assert_eq!(order, [(0, 780), (60, 1920)]);
        super::clear(dir.path()).expect("clears");
        assert!(super::kept(dir.path()).expect("reads").is_empty());
        assert!(
            dir.path().join("latest-drawn-frame.rgba").exists(),
            "only kept frames go"
        );
    }
}
