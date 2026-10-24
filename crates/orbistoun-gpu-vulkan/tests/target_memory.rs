//! What a resident colour target costs the host, and that what it holds survives the cuts: a
//! target holds no staging buffer of its own, only the frame read last keeps a host copy, spare
//! snapshots are bounded by bytes, and an idle target let go of is drawn on again from what guest
//! memory holds, byte for byte.

use orbistoun_gpu::{
    Rect, RenderBackend, RenderCommand, Resource, ResourceId, ShaderStage, TargetEncoding,
};
use orbistoun_gpu_vulkan::VulkanBackend;
use orbistoun_gpu_vulkan::compute::{Availability, probe};
use orbistoun_gpu_vulkan::framebuffer::Pixels;
use orbistoun_spirv::{constant_colour_fragment_module, fullscreen_triangle_vertex_module};

const VERTEX: ResourceId = ResourceId(1);
const GREEN: ResourceId = ResourceId(2);
const RED: ResourceId = ResourceId(3);
const BLUE: ResourceId = ResourceId(4);

/// A backend with the modules made resident, or `None` where there is no device or it cannot count
/// samples - a counting draw is one drawn on the resident attachment (D827).
fn backend() -> Option<VulkanBackend> {
    if let Availability::Unavailable { reason } = probe() {
        println!("SKIPPED - no device: {reason}");
        return None;
    }
    let mut backend = VulkanBackend::new();
    let vertex = fullscreen_triangle_vertex_module();
    backend
        .ensure_resident(VERTEX, Resource::Shader(&vertex))
        .expect("vertex");
    for (id, colour) in [
        (GREEN, [0.0, 1.0, 0.0, 1.0]),
        (RED, [1.0, 0.0, 0.0, 1.0]),
        (BLUE, [0.0, 0.0, 1.0, 1.0]),
    ] {
        let module = constant_colour_fragment_module(colour);
        backend
            .ensure_resident(id, Resource::Shader(&module))
            .expect("fragment");
    }
    if backend.samples_passed().is_none() {
        println!("SKIPPED - the device does not count samples exactly");
        return None;
    }
    Some(backend)
}

fn target(backend: &mut VulkanBackend, id: u64, (width, height): (u32, u32)) -> ResourceId {
    target_of(backend, id, (width, height), TargetEncoding::Unorm8)
}

fn target_of(
    backend: &mut VulkanBackend,
    id: u64,
    (width, height): (u32, u32),
    encoding: TargetEncoding,
) -> ResourceId {
    let id = ResourceId(id);
    backend
        .ensure_resident(
            id,
            Resource::RenderTarget {
                width,
                height,
                encoding,
            },
        )
        .expect("target");
    id
}

/// Draws `fragment`'s colour over `rect` of `target`, on its resident attachment.
fn draw(backend: &mut VulkanBackend, target: ResourceId, fragment: ResourceId, rect: Rect) {
    for command in [
        RenderCommand::SetRenderTargets {
            colour: vec![target],
            depth: None,
        },
        RenderCommand::SetSampleCounting(true),
        RenderCommand::BindShader {
            stage: ShaderStage::Vertex,
            shader: VERTEX,
        },
        RenderCommand::BindShader {
            stage: ShaderStage::Fragment,
            shader: fragment,
        },
        RenderCommand::SetViewport(rect),
        RenderCommand::Draw {
            vertices: 3,
            instances: 1,
            first_vertex: 0,
        },
    ] {
        backend.execute(&command).expect("drawn");
    }
}

fn rect(x: i32, width: u32, height: u32) -> Rect {
    Rect {
        x,
        y: 0,
        width,
        height,
    }
}

fn frame(backend: &mut VulkanBackend, target: ResourceId) -> Vec<u8> {
    backend
        .target_frame(target)
        .expect("the target's frame")
        .bytes
        .clone()
}

fn held_on_host(backend: &VulkanBackend) -> u64 {
    backend
        .host_held()
        .iter()
        .find(|(name, _)| *name == "targets held on the host")
        .map_or(0, |(_, bytes)| *bytes)
}

/// A resident target holds its image and one row of it: no host-visible buffer of its own the size
/// of the image, which every target held for its whole life.
#[test]
fn a_resident_target_holds_its_image_and_one_row() {
    let Some(mut backend) = backend() else {
        return;
    };
    let (width, height) = (64, 32);
    let id = target(&mut backend, 0x8000_0000_0000_0010, (width, height));
    draw(&mut backend, id, GREEN, rect(0, width, height));
    let image = u64::from(width * height * 4);
    let held = backend.resident_target_bytes();
    assert!(
        held >= image && held < 2 * image,
        "{held} bytes held for a {image}-byte image"
    );
}

/// Pixels handed in, a clear filled on the device and rows patched in all come back exactly, through
/// staging lent for each copy: a seed, a uniform fill copied row by row, and a band of rows.
#[test]
fn what_goes_through_staging_comes_back_byte_identical() {
    let Some(mut backend) = backend() else {
        return;
    };
    for (index, (encoding, texel)) in [
        (TargetEncoding::Unorm8, 4usize),
        (TargetEncoding::Unorm16x2, 4),
        (TargetEncoding::Float32x2, 8),
    ]
    .into_iter()
    .enumerate()
    {
        let (width, height) = (24u32, 10u32);
        let id = target_of(
            &mut backend,
            0x8000_0000_0000_0100 | index as u64,
            (width, height),
            encoding,
        );
        draw(&mut backend, id, GREEN, rect(0, width, height));
        let pixels: Vec<u8> = (0..width as usize * height as usize * texel)
            .map(|i| (i * 7 % 251) as u8)
            .collect();
        backend.seed_target(
            id,
            Pixels {
                width,
                height,
                bytes: pixels.clone(),
            },
        );
        assert_eq!(
            frame(&mut backend, id),
            pixels,
            "{encoding:?}: the seed comes back"
        );

        let word = 0x8040_2010u32;
        backend.seed_target_uniform(id, (width, height), word);
        let filled = frame(&mut backend, id);
        assert_eq!(
            filled.len(),
            width as usize * height as usize * texel,
            "{encoding:?}"
        );
        assert!(
            filled
                .chunks_exact(4)
                .all(|w| u32::from_le_bytes([w[0], w[1], w[2], w[3]]) == word),
            "{encoding:?}: every word of every row is the fill's"
        );
    }

    // Rows of the frame drawn last, read and patched in place (D864).
    let (width, height) = (16u32, 12u32);
    let id = target(&mut backend, 0x8000_0000_0000_0200, (width, height));
    draw(&mut backend, id, RED, rect(0, width, height));
    let band: Vec<u8> = (0..width as usize * 3 * 4).map(|i| i as u8).collect();
    assert!(backend.patch_last_frame_rows((4, 3), &band));
    let rows = backend.last_frame_rows((4, 3)).expect("rows");
    assert_eq!(rows.bytes, band, "the rows patched in come back");
    let whole = frame(&mut backend, id);
    let row = width as usize * 4;
    assert_eq!(&whole[4 * row..7 * row], band.as_slice());
    assert!(
        whole[..4 * row]
            .chunks_exact(4)
            .all(|p| p == [255, 0, 0, 255]),
        "the rest is as drawn"
    );
}

/// Of the targets resident on the device, only the one read back last keeps a copy on the host; one
/// read again comes off the device as it was.
#[test]
fn only_the_frame_read_last_is_held_on_the_host() {
    let Some(mut backend) = backend() else {
        return;
    };
    let (width, height) = (32, 16);
    let a = target(&mut backend, 0x8000_0000_0000_0300, (width, height));
    let b = target(&mut backend, 0x8000_0000_0000_0301, (width, height));
    draw(&mut backend, a, GREEN, rect(0, width / 2, height));
    draw(&mut backend, b, BLUE, rect(0, width, height));
    let first_a = frame(&mut backend, a);
    let first_b = frame(&mut backend, b);
    let one_frame = u64::from(width * height * 4);
    assert_eq!(
        held_on_host(&backend),
        one_frame,
        "only the last read is held"
    );
    assert!(backend.holds_target(a, (width, height)));
    assert!(backend.holds_target(b, (width, height)));
    assert_eq!(frame(&mut backend, a), first_a, "read again off the device");
    assert_eq!(frame(&mut backend, b), first_b);
    assert_eq!(held_on_host(&backend), one_frame);
}

/// Spare snapshot buffers are kept up to their bytes, the oldest let go of first, and a spare of
/// the size asked for is still taken again.
#[test]
fn spare_snapshots_are_bounded_by_bytes() {
    let Some(mut backend) = backend() else {
        return;
    };
    // 36 MiB a frame: eight are 288 MiB, past the 256 MiB the spares keep.
    let (width, height) = (3072, 3072);
    let id = target(&mut backend, 0x8000_0000_0000_0400, (width, height));
    let mut kept = Vec::new();
    for _ in 0..8 {
        draw(&mut backend, id, GREEN, rect(0, 8, 8));
        kept.push(backend.snapshot_last_frame().expect("a snapshot"));
    }
    for id in kept {
        backend.drop_snapshot(id);
    }
    let (count, bytes) = backend.spare_snapshots();
    let frame = u64::from(width * height * 4);
    assert!(bytes <= 256 << 20, "{bytes} bytes of spares");
    assert_eq!(count, 7, "as many as fit");
    assert_eq!(bytes, 7 * frame);
    draw(&mut backend, id, RED, rect(0, 8, 8));
    let again = backend.snapshot_last_frame().expect("a snapshot");
    assert_eq!(backend.spare_snapshots().0, 6, "a spare taken again");
    backend.drop_snapshot(again);
}

/// Draws `a`'s two halves - green, then after the target was read back and maybe let go of and
/// handed back what was read, red - and answers its frame.
fn drawn_across_a_let_go(let_go: bool) -> Option<Vec<u8>> {
    let mut backend = backend()?;
    backend.set_target_budget(0, 2);
    let (width, height) = (40, 20);
    let a = target(&mut backend, 0x8000_0000_0000_0500, (width, height));
    let b = target(&mut backend, 0x8000_0000_0000_0501, (width, height));
    draw(&mut backend, a, GREEN, rect(0, width / 2, height));
    // Written back: what guest memory now holds.
    let memory = frame(&mut backend, a);
    draw(&mut backend, b, BLUE, rect(0, width, height));
    backend.set_frame(3);
    if let_go {
        let gone = backend.evict_idle_targets(&[]);
        assert_eq!(gone, vec![a], "the idle target read back goes");
        assert!(!backend.holds_target(a, (width, height)));
        assert!(
            backend.holds_target(b, (width, height)),
            "the target drawn last stays"
        );
        backend.seed_target(
            a,
            Pixels {
                width,
                height,
                bytes: memory,
            },
        );
    }
    let half = i32::try_from(width / 2).expect("half");
    draw(&mut backend, a, RED, rect(half, width / 2, height));
    Some(frame(&mut backend, a))
}

/// A target let go of for being idle and handed back what guest memory holds draws exactly the
/// frame it would have had it been kept.
#[test]
fn a_target_let_go_of_draws_on_byte_identical() {
    let Some(kept) = drawn_across_a_let_go(false) else {
        return;
    };
    let let_go = drawn_across_a_let_go(true).expect("a device");
    assert_eq!(let_go, kept);
}

/// A target whose draws are on the device alone, drawn and never read back nor kept in a snapshot,
/// is never let go of, nor one drawn within the idle frames, nor one kept; one kept whole in a
/// snapshot, whose write-back the snapshot carries (D719), is.
#[test]
fn only_a_target_whose_frame_left_the_device_is_let_go_of() {
    let Some(mut backend) = backend() else {
        return;
    };
    backend.set_target_budget(0, 4);
    let (width, height) = (16, 16);
    let a = target(&mut backend, 0x8000_0000_0000_0600, (width, height));
    let b = target(&mut backend, 0x8000_0000_0000_0601, (width, height));
    let c = target(&mut backend, 0x8000_0000_0000_0602, (width, height));
    draw(&mut backend, a, GREEN, rect(0, width, height));
    draw(&mut backend, c, GREEN, rect(0, width, height));
    let _ = frame(&mut backend, c);
    draw(&mut backend, b, RED, rect(0, width, height));
    backend.set_frame(2);
    assert!(
        backend.evict_idle_targets(&[]).is_empty(),
        "nothing idle for long enough"
    );
    backend.set_frame(10);
    assert_eq!(
        backend.evict_idle_targets(&[c]),
        Vec::<ResourceId>::new(),
        "a drawn and unread, b drawn last, c kept"
    );

    // a's frame kept whole in a snapshot, as a switch of target keeps it (D885).
    draw(&mut backend, a, BLUE, rect(0, width, height));
    let snapshot = backend.snapshot_last_frame().expect("a snapshot");
    draw(&mut backend, b, RED, rect(0, width, height));
    backend.set_frame(12);
    assert_eq!(
        backend.evict_idle_targets(&[]),
        vec![c],
        "a was drawn within the idle frames"
    );
    backend.set_frame(20);
    assert_eq!(backend.evict_idle_targets(&[]), vec![a]);
    let kept = backend.take_snapshot(snapshot).expect("the snapshot");
    assert!(
        kept.bytes.chunks_exact(4).all(|p| p == [0, 0, 255, 255]),
        "the snapshot still holds the frame let go of"
    );
    assert!(backend.holds_target(b, (width, height)));
}
