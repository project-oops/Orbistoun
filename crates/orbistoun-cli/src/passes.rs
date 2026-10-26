//! `orbistoun-cli passes`: a frame's pass dump (`ORBISTOUN_PASS_DUMP`, D986) as pictures and a
//! table, each pass flagged where what it wrote is implausible.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::Path;

use anyhow::{Context, Result};
use orbistoun_gpu::pass_dump::{self, Numeric};

/// One pass, as `passes.tsv` lists it.
#[derive(Debug, Clone, Default)]
struct Pass {
    number: u32,
    kind: String,
    program: String,
    work: String,
    inputs: Vec<u64>,
    note: String,
}

/// One image a pass wrote, as `outputs.tsv` lists it.
#[derive(Debug, Clone, Default)]
struct Output {
    pass: u32,
    slot: u32,
    address: u64,
    format: String,
    width: u32,
    height: u32,
    layers: u32,
    file: String,
}

fn hex(value: &str) -> Option<u64> {
    u64::from_str_radix(value.trim().trim_start_matches("0x"), 16).ok()
}

/// The rows of a listing, its header aside, each split at its tabs.
fn rows(path: &Path) -> Result<Vec<Vec<String>>> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    Ok(text
        .lines()
        .skip(1)
        .filter(|line| !line.is_empty())
        .map(|line| line.split('\t').map(str::to_owned).collect())
        .collect())
}

fn read_passes(dir: &Path) -> Result<Vec<Pass>> {
    Ok(rows(&dir.join(pass_dump::PASSES))?
        .into_iter()
        .filter_map(|row| {
            let field = |at: usize| row.get(at).cloned().unwrap_or_default();
            Some(Pass {
                number: field(0).parse().ok()?,
                kind: field(1),
                program: field(2),
                work: field(3),
                inputs: field(4).split(',').filter_map(hex).collect(),
                note: field(5),
            })
        })
        .collect())
}

fn read_outputs(dir: &Path) -> Result<Vec<Output>> {
    Ok(rows(&dir.join(pass_dump::OUTPUTS))?
        .into_iter()
        .filter_map(|row| {
            let field = |at: usize| row.get(at).cloned().unwrap_or_default();
            Some(Output {
                pass: field(0).parse().ok()?,
                slot: field(1).parse().ok()?,
                address: hex(&field(2))?,
                format: field(3),
                width: field(4).parse().ok()?,
                height: field(5).parse().ok()?,
                layers: field(6).parse().unwrap_or(1),
                file: field(7),
            })
        })
        .collect())
}

/// The flags a pass earns beyond its outputs' own: `refused`, `stale` where it reads an image a
/// refused pass of the frame was to write, and `unchanged` where everything it wrote is as it was.
fn pass_flags(pass: &Pass, refused_writes: &BTreeSet<u64>, outputs: &[&Output]) -> Vec<String> {
    let mut flags = Vec::new();
    if pass.kind.starts_with("refused") {
        flags.push("refused".to_owned());
    }
    let stale: Vec<String> = pass
        .inputs
        .iter()
        .filter(|at| refused_writes.contains(at))
        .map(|at| format!("{at:#x}"))
        .collect();
    if !stale.is_empty() {
        flags.push(format!("stale({})", stale.join(",")));
    }
    if !outputs.is_empty() && outputs.iter().all(|output| output.file.starts_with('=')) {
        flags.push("unchanged".to_owned());
    }
    flags
}

/// An image's texels as an `Rgb8` picture, every `step`th texel of every `step`th row of its first
/// layer: a normalised channel as it is, a float one over the image's peak, an integer one over
/// its largest; NaN magenta, infinity white. A single channel is grey.
fn picture(output: &Output, bytes: &[u8], peak: f32, step: u32) -> Option<image::RgbImage> {
    let (stride, channels, numeric) = pass_dump::layout(&output.format)?;
    let step = step.max(1);
    let (width, height) = (output.width.div_ceil(step), output.height.div_ceil(step));
    let scale = match numeric {
        Numeric::Normalised => 1.0,
        Numeric::Float | Numeric::Integer => peak.max(f32::MIN_POSITIVE),
    };
    let mut picture = image::RgbImage::new(width.max(1), height.max(1));
    for y in 0..height {
        for x in 0..width {
            let index = (y * step * output.width + x * step) as usize;
            let Some(texel) = bytes
                .get(index * stride..index * stride + stride)
                .and_then(|texel| pass_dump::texel(&output.format, texel))
            else {
                continue;
            };
            let byte = |value: f32| -> u8 {
                if value.is_nan() {
                    0
                } else {
                    ((value / scale).clamp(0.0, 1.0) * 255.0).round() as u8
                }
            };
            let pixel = if texel.iter().take(channels).any(|v| v.is_nan()) {
                [255, 0, 255]
            } else if texel.iter().take(channels).any(|v| v.is_infinite()) {
                [255, 255, 255]
            } else if channels == 1 {
                [byte(texel[0]); 3]
            } else {
                [byte(texel[0]), byte(texel[1]), byte(texel[2])]
            };
            picture.put_pixel(x, y, image::Rgb(pixel));
        }
    }
    Some(picture)
}

/// Judges each image a pass wrote, read from `dir`, and writes its picture into `out` (every
/// `step`th texel): its table lines and `passes.html` cells, its flags added to the pass's `flags`.
fn judge_outputs(
    written: &[&Output],
    (dir, out, step): (&Path, &Path, u32),
    flags: &mut Vec<String>,
) -> Result<(Vec<String>, String)> {
    let mut lines = Vec::new();
    let mut cells = String::new();
    for output in written {
        let file = output.file.trim_start_matches('=');
        let judged = if file == "-" || file.is_empty() {
            None
        } else {
            let bytes = std::fs::read(dir.join(file)).with_context(|| format!("reading {file}"))?;
            let judged = pass_dump::judge(
                &output.format,
                (output.width, output.height, output.layers),
                &bytes,
            );
            let picture_name = format!("{file}.png");
            if !output.file.starts_with('=')
                && !out.join(&picture_name).exists()
                && let Some(picture) = picture(
                    output,
                    &bytes,
                    judged.map_or(1.0, |judged| judged.peak),
                    step,
                )
            {
                picture
                    .save(out.join(&picture_name))
                    .with_context(|| format!("writing {picture_name}"))?;
            }
            let _ = write!(
                cells,
                "<td><img src=\"{picture_name}\"><br>o{} {:#x} {} {}x{}</td>",
                output.slot, output.address, output.format, output.width, output.height
            );
            judged
        };
        let own: Vec<&str> = judged.map(|judged| judged.flags()).unwrap_or_default();
        for flag in &own {
            if !flags.iter().any(|seen| seen == flag) {
                flags.push((*flag).to_owned());
            }
        }
        lines.push(format!(
            "{:<16} {:<20} {:>7} {:>6}  {}",
            format!("{:#x}", output.address),
            output.format,
            judged.map_or_else(|| "-".to_owned(), |judged| format!("{:.3}", judged.noise)),
            judged.map_or_else(
                || "-".to_owned(),
                |judged| (judged.zero * 100 / judged.texels.max(1)).to_string()
            ),
            own.join(",")
        ));
    }
    Ok((lines, cells))
}

/// Reads the dump in `dir`, writes a picture of each image written beside it (or into `out`) and
/// `passes.html`, and prints the pass table with its flags.
pub(crate) fn cmd_passes(dir: &Path, out: Option<&Path>, step: u32) -> Result<()> {
    let passes = read_passes(dir)?;
    let outputs = read_outputs(dir)?;
    let out = out.unwrap_or(dir);
    std::fs::create_dir_all(out).with_context(|| format!("creating {}", out.display()))?;
    let mut by_pass: BTreeMap<u32, Vec<&Output>> = BTreeMap::new();
    for output in &outputs {
        by_pass.entry(output.pass).or_default().push(output);
    }
    // What the refused passes were to write: a later pass that reads it reads what was there.
    let mut refused_writes = BTreeSet::new();
    let mut table = String::new();
    let mut html = String::from(concat!(
        "<!doctype html><meta charset=utf-8><title>Passes</title><style>",
        "body{font:13px monospace;background:#111;color:#ddd}td{vertical-align:top;padding:2px 6px}",
        "img{max-width:320px;image-rendering:pixelated}.f{color:#f66}</style><table>\n"
    ));
    let _ = writeln!(
        table,
        "{:>5}  {:<17} {:<16} {:<22} {:<16} {:<20} {:>7} {:>6}  flags",
        "pass", "kind", "program", "work", "address", "format", "noise", "zero%"
    );
    let mut first_flagged = None;
    for pass in &passes {
        let written = by_pass.get(&pass.number).cloned().unwrap_or_default();
        let mut flags = pass_flags(pass, &refused_writes, &written);
        if pass.kind.starts_with("refused") {
            refused_writes.extend(written.iter().map(|output| output.address));
        }
        let (lines, cells) = judge_outputs(&written, (dir, out, step), &mut flags)?;
        if first_flagged.is_none()
            && flags
                .iter()
                .any(|flag| matches!(flag.as_str(), "noise" | "nan" | "inf"))
        {
            first_flagged = Some(pass.number);
        }
        let head = format!(
            "{:>5}  {:<17} {:<16} {:<22}",
            pass.number,
            pass.kind,
            shorten(&pass.program, 16),
            shorten(&pass.work, 22)
        );
        if lines.is_empty() {
            let _ = writeln!(table, "{head} {:<52}  {}", "", flags.join(","));
        }
        for (at, line) in lines.iter().enumerate() {
            let lead = if at == 0 {
                head.clone()
            } else {
                " ".repeat(head.len())
            };
            let pass_flags = if at == 0 {
                flags.join(",")
            } else {
                String::new()
            };
            let _ = writeln!(table, "{lead} {line}  {pass_flags}");
        }
        if !pass.note.is_empty() {
            let _ = writeln!(table, "{:>7}{}", "", shorten(&pass.note, 160));
        }
        let _ = writeln!(
            html,
            "<tr><td>{}</td><td>{}<br>{}<br>{}<br><span class=f>{}</span><br>{}</td>{cells}</tr>",
            pass.number,
            pass.kind,
            pass.program,
            pass.work,
            flags.join(" "),
            pass.note
        );
    }
    html.push_str("</table>\n");
    std::fs::write(out.join("passes.html"), html).context("writing passes.html")?;
    print!("{table}");
    match first_flagged {
        Some(number) => println!(
            "the first pass whose output is non-finite or noise is {number}; pictures and passes.html are in {}",
            out.display()
        ),
        None => println!(
            "no pass's output is non-finite or noise; pictures and passes.html are in {}",
            out.display()
        ),
    }
    Ok(())
}

/// `text` cut to `most` characters.
fn shorten(text: &str, most: usize) -> String {
    if text.chars().count() <= most {
        return text.to_owned();
    }
    let mut cut: String = text.chars().take(most.saturating_sub(1)).collect();
    cut.push('~');
    cut
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pass_reading_what_a_refused_pass_was_to_write_is_stale() {
        let refused: BTreeSet<u64> = [0x1000].into_iter().collect();
        let pass = Pass {
            kind: "dispatch".to_owned(),
            inputs: vec![0x2000, 0x1000],
            ..Pass::default()
        };
        let same = Output {
            file: "=p0001-o0-1000.bin".to_owned(),
            ..Output::default()
        };
        assert_eq!(
            pass_flags(&pass, &refused, &[&same]),
            ["stale(0x1000)", "unchanged"]
        );
        let refused_pass = Pass {
            kind: "refused-dispatch".to_owned(),
            ..Pass::default()
        };
        assert_eq!(
            pass_flags(&refused_pass, &BTreeSet::new(), &[]),
            ["refused"]
        );
    }

    #[test]
    fn a_dump_reads_back_as_its_table() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join(pass_dump::PASSES),
            "pass\tkind\tprogram\twork\tinputs\tnote\n0\tdispatch\t0x10\t[1, 1, 1] groups\t\t\n1\trefused-dispatch\t0x20\t[1, 1, 1] groups\t0x30\twaits\n",
        )
        .unwrap();
        // Two by two Rgba8: static would need more texels; this one is a flat colour.
        std::fs::write(
            dir.path().join(pass_dump::OUTPUTS),
            "pass\tslot\taddress\tformat\twidth\theight\tlayers\tfile\n0\t0\t0x40\timage:Rgba8\t2\t2\t1\tp0000-o0-40.bin\n1\t0\t0x50\timage:Rgba8\t2\t2\t1\t-\n",
        )
        .unwrap();
        std::fs::write(dir.path().join("p0000-o0-40.bin"), [9_u8; 16]).unwrap();
        cmd_passes(dir.path(), None, 1).unwrap();
        assert!(dir.path().join("p0000-o0-40.bin.png").exists());
        assert!(dir.path().join("passes.html").exists());
        assert_eq!(read_passes(dir.path()).unwrap()[1].inputs, [0x30]);
        assert_eq!(read_outputs(dir.path()).unwrap()[1].file, "-");
    }
}
