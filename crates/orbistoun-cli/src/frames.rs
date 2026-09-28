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

#[cfg(test)]
mod tests {
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
