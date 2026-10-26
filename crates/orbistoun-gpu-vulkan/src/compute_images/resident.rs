//! The images the device keeps for dispatches from one to the next (D990).
//!
//! The command processor decides which images are kept and when one is current
//! ([`orbistoun_gpu::resident`]); this holds them. A dispatch is handed each image's place - its
//! key, the generation of its contents, and whether its texels came with it - beside the images
//! themselves, for the call that runs it ([`with_hints`]). An image whose texels came is uploaded
//! as before and kept after the dispatch; one whose texels did not is taken as it was left, in the
//! layout it was left in, with nothing uploaded. An image asked for that is not kept, at that
//! generation, refuses the dispatch with [`orbistoun_gpu::resident::NOT_RESIDENT`].

use std::cell::RefCell;
use std::collections::HashMap;

use ash::vk;
use orbistoun_gpu::resident::{NOT_RESIDENT, Use, Uses};

use super::{Created, DispatchImage, DispatchImages};
use crate::compute::DispatchError;

/// A dispatch's images' places, by role, and the keys the device is to let go first.
#[derive(Debug, Clone, Default)]
pub struct Hints {
    /// Each image's place, by role, as [`DispatchImages`] binds them.
    pub uses: Uses,
    /// Keys let go since the last dispatch.
    pub dropped: Vec<u64>,
}

thread_local! {
    /// The places of the images of the dispatch this thread is running.
    static HINTS: RefCell<Option<Hints>> = const { RefCell::new(None) };
}

/// Runs `run` - a dispatch - with its images' places `hints`: the images it binds are taken from
/// those kept, and kept after it, as they say.
pub fn with_hints<T>(hints: Hints, run: impl FnOnce() -> T) -> T {
    let was = HINTS.with(|held| held.replace(Some(hints)));
    let out = run();
    HINTS.with(|held| *held.borrow_mut() = was);
    out
}

/// Each image's place, in the order [`super::wanted_images`] makes them - the first texture slot's,
/// the stored, the further slots', the selected, then the arrays' fillers, which have none - and the
/// keys to let go. Nothing where no places were handed over.
pub(super) fn wanted(images: &DispatchImages<'_>) -> (Vec<Option<Use>>, Vec<u64>) {
    let Some(hints) = HINTS.with(|held| held.borrow_mut().take()) else {
        return (Vec::new(), Vec::new());
    };
    let uses = &hints.uses;
    let mut wanted = vec![uses.fetched];
    wanted.extend((0..images.stored.len()).map(|n| uses.stored.get(n).copied().flatten()));
    wanted.extend(
        (0..images.more_fetched.len()).map(|n| uses.more_fetched.get(n).copied().flatten()),
    );
    if let Some(set) = images.selected {
        wanted.extend((0..set.images.len()).map(|n| uses.selected.get(n).copied().flatten()));
    }
    (wanted, hints.dropped)
}

/// One image kept: its contents' generation, its shape, what was made for it, and the layout it
/// was left in.
struct Kept {
    device: u64,
    generation: u64,
    shape: Shape,
    created: Created,
    layout: vk::ImageLayout,
}

/// What an image kept must match to be taken: its format and extent, and how it is viewed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Shape {
    format: super::DispatchFormat,
    width: u32,
    height: u32,
    depth: Option<u32>,
    layers: Option<u32>,
    cube: bool,
}

impl Shape {
    const fn of(image: &DispatchImage<'_>) -> Self {
        Self {
            format: image.format,
            width: image.width,
            height: image.height,
            depth: image.depth,
            layers: image.layers,
            cube: image.cube,
        }
    }
}

/// The images kept, by key.
fn kept() -> &'static std::sync::Mutex<HashMap<u64, Kept>> {
    static KEPT: std::sync::OnceLock<std::sync::Mutex<HashMap<u64, Kept>>> =
        std::sync::OnceLock::new();
    KEPT.get_or_init(|| std::sync::Mutex::new(HashMap::new()))
}

/// Lets each of `keys` go, on `device`.
pub(super) fn let_go(device: &ash::Device, keys: &[u64]) {
    if keys.is_empty() {
        return;
    }
    let gone: Vec<Kept> = {
        let mut held = kept()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        keys.iter().filter_map(|key| held.remove(key)).collect()
    };
    for image in gone {
        release(device, &image);
    }
}

/// Destroys what was made for an image kept, where it is still this device's.
fn release(device: &ash::Device, image: &Kept) {
    if image.device != vk::Handle::as_raw(device.handle()) {
        return;
    }
    let (handle, memory, view, staging, staging_memory) = image.created;
    super::release_image(device, (handle, memory, Some(view)));
    super::release_buffer(device, staging, staging_memory);
}

/// The usage an image kept is made with: each role a dispatch can bind it in - stored to, fetched
/// from, uploaded and read back - where its format can be stored to on this device; fetched and
/// uploaded alone where it cannot.
pub(super) fn usage(
    (instance, physical): (&ash::Instance, vk::PhysicalDevice),
    format: super::DispatchFormat,
) -> vk::ImageUsageFlags {
    // SAFETY: the physical device came from this instance.
    let features =
        unsafe { instance.get_physical_device_format_properties(physical, format.vulkan()) }
            .optimal_tiling_features;
    let fetched = vk::ImageUsageFlags::SAMPLED | vk::ImageUsageFlags::TRANSFER_DST;
    if features.contains(vk::FormatFeatureFlags::STORAGE_IMAGE) {
        fetched | vk::ImageUsageFlags::STORAGE | vk::ImageUsageFlags::TRANSFER_SRC
    } else {
        fetched
    }
}

/// An image's place in a dispatch, and its shape: what it is kept under after the dispatch.
#[derive(Debug, Clone, Copy)]
pub(super) struct Place {
    used: Use,
    shape: Shape,
}

impl Place {
    /// Whether what a dispatch stores to the image stays on the device, its write-back deferred.
    pub(super) const fn deferred(&self) -> bool {
        self.used.deferred
    }
}

/// The image kept under `key` - at `generation` or one made since by a dispatch storing to it
/// again - read off the device as linear texels, a word or more a texel as a dispatch's are: what
/// its deferred write-back writes (D990). It stays kept. `None` where none is kept under `key` at
/// that generation or later, it holds no buffer to read through, or the device cannot be reached
/// from here - the session held by a dispatch on this thread.
#[must_use]
pub fn read_kept(key: u64, generation: u64) -> Option<Vec<u32>> {
    let session = crate::compute::session().ok()?;
    let session = session.try_lock().ok()?;
    let crate::compute::Session {
        ref device,
        queue,
        family,
        ..
    } = *session;
    let mut held = kept()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let image = held.get_mut(&key)?;
    let (handle, _, _, staging, staging_memory) = image.created;
    if image.device != vk::Handle::as_raw(device.handle())
        || image.generation < generation
        || staging == vk::Buffer::null()
    {
        return None;
    }
    copy_to_staging(device, (queue, family), image).ok()?;
    image.layout = vk::ImageLayout::GENERAL;
    let shape = image.shape;
    let texels = shape.width as usize
        * shape.height as usize
        * shape.depth.unwrap_or(1) as usize
        * shape.layers.unwrap_or(1) as usize;
    let (words, size) = shape.format.words();
    let bytes = texels * words * size;
    // SAFETY: the memory is host-visible and coherent, holds `bytes`, and is not mapped; the copy
    // into it has completed.
    let mapped = unsafe {
        device.map_memory(
            staging_memory,
            0,
            bytes as vk::DeviceSize,
            vk::MemoryMapFlags::empty(),
        )
    }
    .ok()?;
    // SAFETY: the mapping covers `bytes` bytes and nothing writes it while it is read.
    let raw = unsafe { std::slice::from_raw_parts(mapped.cast::<u8>().cast_const(), bytes) };
    // A word a texel's `size` bytes, or several whole words, as a dispatch image's texels are.
    let read = raw
        .chunks_exact(size)
        .map(|part| {
            part.iter()
                .rev()
                .fold(0_u32, |word, &byte| word << 8 | u32::from(byte))
        })
        .collect();
    // SAFETY: mapped just above.
    unsafe { device.unmap_memory(staging_memory) };
    let _ = handle;
    Some(read)
}

/// Copies `image` into the buffer it travels through, after every write to it, and waits.
fn copy_to_staging(
    device: &ash::Device,
    (queue, family): (vk::Queue, u32),
    image: &Kept,
) -> Result<(), DispatchError> {
    let (handle, _, _, staging, _) = image.created;
    let pool_info = vk::CommandPoolCreateInfo::default().queue_family_index(family);
    // SAFETY: the family index is one this device was created with.
    let pool = unsafe { device.create_command_pool(&pool_info, None) }
        .map_err(|e| DispatchError::Vulkan("create_command_pool(kept image)", e))?;
    let copied = (|| {
        let allocate = vk::CommandBufferAllocateInfo::default()
            .command_pool(pool)
            .level(vk::CommandBufferLevel::PRIMARY)
            .command_buffer_count(1);
        // SAFETY: the pool is live.
        let command = unsafe { device.allocate_command_buffers(&allocate) }
            .map_err(|e| DispatchError::Vulkan("allocate_command_buffers(kept image)", e))?[0];
        let begin = vk::CommandBufferBeginInfo::default()
            .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT);
        // SAFETY: the command buffer is fresh and not recording.
        unsafe { device.begin_command_buffer(command, &begin) }
            .map_err(|e| DispatchError::Vulkan("begin_command_buffer(kept image)", e))?;
        super::barrier(
            device,
            command,
            handle,
            (image.layout, vk::ImageLayout::GENERAL),
            (
                vk::AccessFlags::SHADER_WRITE | vk::AccessFlags::TRANSFER_WRITE,
                vk::AccessFlags::TRANSFER_READ,
            ),
            (
                vk::PipelineStageFlags::ALL_COMMANDS,
                vk::PipelineStageFlags::TRANSFER,
            ),
        );
        let shape = image.shape;
        let region = [super::whole(
            shape.width,
            shape.height,
            (shape.depth, shape.layers),
        )];
        // SAFETY: recording is open, the image is in the general layout after the barrier above,
        // and the buffer holds its texels.
        unsafe {
            device.cmd_copy_image_to_buffer(
                command,
                handle,
                vk::ImageLayout::GENERAL,
                staging,
                &region,
            );
        }
        let to_host = [vk::MemoryBarrier::default()
            .src_access_mask(vk::AccessFlags::TRANSFER_WRITE)
            .dst_access_mask(vk::AccessFlags::HOST_READ)];
        // SAFETY: recording is open.
        unsafe {
            device.cmd_pipeline_barrier(
                command,
                vk::PipelineStageFlags::TRANSFER,
                vk::PipelineStageFlags::HOST,
                vk::DependencyFlags::empty(),
                &to_host,
                &[],
                &[],
            );
        }
        // SAFETY: recording is open.
        unsafe { device.end_command_buffer(command) }
            .map_err(|e| DispatchError::Vulkan("end_command_buffer(kept image)", e))?;
        // SAFETY: the device is live and the create info outlives the call.
        let fence = unsafe { device.create_fence(&vk::FenceCreateInfo::default(), None) }
            .map_err(|e| DispatchError::Vulkan("create_fence(kept image)", e))?;
        let commands = [command];
        let submits = [vk::SubmitInfo::default().command_buffers(&commands)];
        let serial = crate::device_lost::submit(crate::device_lost::Kind::Copy);
        // SAFETY: the recording is complete, the queue is this device's, and the fence is fresh.
        let waited = unsafe { device.queue_submit(queue, &submits, fence) }
            .map_err(crate::device_lost::vulkan("queue_submit(kept image)"))
            .and_then(|()| {
                // SAFETY: the fence was submitted with the work just above.
                unsafe { device.wait_for_fences(&[fence], true, u64::MAX) }
                    .map_err(crate::device_lost::vulkan("wait_for_fences(kept image)"))
            });
        if waited.is_ok() {
            crate::device_lost::complete(serial);
        }
        // SAFETY: the submission it fenced has completed, or was never made.
        unsafe { device.destroy_fence(fence, None) };
        waited
    })();
    // SAFETY: nothing recorded from the pool is still running.
    unsafe { device.destroy_command_pool(pool, None) };
    copied
}

/// What a dispatch binding an image at its place takes: what was made for it, the layout its
/// contents were left in where they are kept and nothing is uploaded, and where it is kept after.
pub(super) type Taken = (Created, Option<vk::ImageLayout>, Place);

/// The image `image` at its place `used`: kept and current, taken as it was left; its texels
/// handed over, the image kept under the key made again - or, of its shape, refilled - with
/// them; `None` where it has no place, and it is bound as any other. Refused where it is asked for
/// unchanged and not kept.
pub(super) fn take(
    (instance, physical, device): (&ash::Instance, vk::PhysicalDevice, &ash::Device),
    used: Option<Use>,
    image: DispatchImage<'_>,
) -> Option<Result<Taken, DispatchError>> {
    let used = used?;
    let raw = vk::Handle::as_raw(device.handle());
    let shape = Shape::of(&image);
    let place = Place { used, shape };
    let held = kept()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .remove(&used.key);
    if !used.fresh {
        return Some(match held {
            Some(kept)
                if kept.device == raw
                    && kept.generation == used.generation
                    && kept.shape == shape =>
            {
                Ok((kept.created, Some(kept.layout), place))
            }
            other => {
                if let Some(other) = other {
                    release(device, &other);
                }
                Err(DispatchError::Unsupported(NOT_RESIDENT.to_owned()))
            }
        });
    }
    // Its texels came: refilled where an image of its shape is kept under its key, else made.
    if let Some(kept) = held {
        if kept.device == raw && kept.shape == shape {
            // A block image kept its blocks alone: its texels travel through a buffer made again.
            if kept.created.3 == vk::Buffer::null() {
                let staged = super::staging_buffer((instance, physical, device), image);
                return Some(match staged {
                    Ok((staging, memory)) => {
                        let (image, image_memory, view, _, _) = kept.created;
                        Ok(((image, image_memory, view, staging, memory), None, place))
                    }
                    Err(e) => {
                        release(device, &kept);
                        Err(e)
                    }
                });
            }
            let refilled =
                super::fill_staging(device, kept.created.4, &super::packed_texels(image));
            return Some(match refilled {
                Ok(()) => Ok((kept.created, None, place)),
                Err(e) => {
                    release(device, &kept);
                    Err(DispatchError::Vulkan("map_memory(kept image)", e))
                }
            });
        }
        release(device, &kept);
    }
    Some(
        orbistoun_gpu::perf::span(orbistoun_gpu::perf::Span::DispatchNewImage, || {
            super::create_image(
                (instance, physical, device),
                image,
                false,
                Some(usage((instance, physical), image.format)),
            )
        })
        .map(|created| (created, None, place)),
    )
}

/// Keeps the image a dispatch bound at `place`, left in `layout`, for the next.
pub(super) fn keep(
    device: &ash::Device,
    place: Place,
    (created, layout): (Created, vk::ImageLayout),
) {
    let Place { used, shape } = place;
    let raw = vk::Handle::as_raw(device.handle());
    // A block image is only ever fetched: the buffer its blocks came through is let go, so the
    // images kept for a table of thousands hold their bytes once, on the device.
    let created = if matches!(shape.format, super::DispatchFormat::Blocks { .. }) {
        let (image, memory, view, staging, staging_memory) = created;
        super::release_buffer(device, staging, staging_memory);
        (
            image,
            memory,
            view,
            vk::Buffer::null(),
            vk::DeviceMemory::null(),
        )
    } else {
        created
    };
    let replaced = kept()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .insert(
            used.key,
            Kept {
                device: raw,
                generation: used.generation,
                shape,
                created,
                layout,
            },
        );
    if let Some(replaced) = replaced {
        release(device, &replaced);
    }
}

#[cfg(test)]
mod tests {
    use super::super::{BoundImages, DispatchFormat, DispatchImage, DispatchImages};
    use super::{Hints, with_hints};
    use crate::compute::{Availability, Session, probe, session};
    use orbistoun_gpu::resident::{NOT_RESIDENT, Use, Uses};

    const SIDE: u32 = 19;

    /// Eight by eight `BC1` texels: four blocks of two words.
    static BLOCKS: [u32; 8] = [
        0x0000_ffff,
        0,
        0xffff_0000,
        0,
        0x1234_5678,
        0,
        0x0f0f_f0f0,
        0,
    ];

    fn images(texels: &[u32]) -> DispatchImages<'_> {
        DispatchImages {
            selected: None,
            more_fetched: &[],
            fetched: Some(DispatchImage {
                texels,
                format: DispatchFormat::Rgba8,
                width: SIDE,
                height: SIDE,
                depth: None,
                layers: None,
                sampling: None,
                cube: false,
            }),
            stored: &[],
        }
    }

    fn hints(used: Use, dropped: Vec<u64>) -> Hints {
        Hints {
            uses: Uses {
                fetched: Some(used),
                ..Uses::default()
            },
            dropped,
        }
    }

    /// An image read for a dispatch is kept after it under its key, and the next dispatch taking
    /// it unchanged binds the very image, its contents kept, with no texels handed over; one asked
    /// for at another generation, or let go, is refused (D990).
    #[test]
    fn a_kept_image_is_taken_by_the_next_dispatch() {
        if let Availability::Unavailable { reason } = probe() {
            eprintln!("skipped: {reason}");
            return;
        }
        let session = session().expect("a session");
        let session = session
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let Session {
            ref instance,
            physical,
            ref device,
            ..
        } = *session;
        let context = (instance, physical, device);
        let read = Use {
            key: 0x5eed_0985,
            generation: 1,
            fresh: true,
            deferred: false,
        };
        let texels = vec![7; (SIDE * SIDE) as usize];
        let first = with_hints(hints(read, Vec::new()), || {
            BoundImages::create(context, &images(&texels))
        })
        .expect("created");
        let image = first.images[0].image;
        assert!(first.images[0].kept_layout.is_none(), "uploaded");
        first.destroy(device);

        let taken = Use {
            fresh: false,
            ..read
        };
        let second = with_hints(hints(taken, Vec::new()), || {
            BoundImages::create(context, &images(&[]))
        })
        .expect("taken");
        assert!(second.images[0].image == image, "the image kept");
        assert!(
            second.images[0].kept_layout == Some(ash::vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL),
            "as it was left, nothing uploaded"
        );
        second.destroy(device);

        let later = Use {
            generation: 2,
            ..taken
        };
        let refused = with_hints(hints(later, Vec::new()), || {
            BoundImages::create(context, &images(&[]))
        });
        assert!(
            matches!(&refused, Err(crate::compute::DispatchError::Unsupported(why)) if why == NOT_RESIDENT),
            "another generation is not kept"
        );
        // The refusal let it go as well; one handed over afresh is made again.
        let again = with_hints(hints(read, vec![read.key]), || {
            BoundImages::create(context, &images(&texels))
        })
        .expect("made");
        again.destroy(device);
        super::let_go(device, &[read.key]);
    }

    /// A block image is bound as its blocks, a quarter of a word a texel for `BC1`, and kept with
    /// no buffer of its own: taken again, it is the same image, and handed its blocks afresh it is
    /// given a buffer to travel through again (D990).
    #[test]
    fn a_block_image_is_kept_as_its_blocks() {
        if let Availability::Unavailable { reason } = probe() {
            eprintln!("skipped: {reason}");
            return;
        }
        let session = session().expect("a session");
        let session = session
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let Session {
            ref instance,
            physical,
            ref device,
            ..
        } = *session;
        let context = (instance, physical, device);
        let format = DispatchFormat::Blocks {
            native: orbistoun_gpu::formats::HostFormat::Bc1RgbaUnormBlock,
            block_bytes: 8,
        };
        if !crate::guest_formats::samples(instance, physical, format.vulkan()) {
            eprintln!("skipped: the device does not sample BC1");
            return;
        }
        let blocks: &'static [u32] = &BLOCKS;
        let image = |texels: &'static [u32]| DispatchImages {
            selected: None,
            more_fetched: &[],
            fetched: Some(DispatchImage {
                texels,
                format,
                width: 8,
                height: 8,
                depth: None,
                layers: None,
                sampling: None,
                cube: false,
            }),
            stored: &[],
        };
        let read = Use {
            key: 0xb10c_0985,
            generation: 1,
            fresh: true,
            deferred: false,
        };
        let first = with_hints(hints(read, Vec::new()), || {
            BoundImages::create(context, &image(blocks))
        })
        .expect("made");
        let made = first.images[0].image;
        first.destroy(device);
        let taken = Use {
            fresh: false,
            ..read
        };
        let second = with_hints(hints(taken, Vec::new()), || {
            BoundImages::create(context, &image(&[]))
        })
        .expect("taken");
        assert!(second.images[0].image == made, "the image kept");
        assert!(
            second.images[0].staging == ash::vk::Buffer::null(),
            "its buffer let go"
        );
        second.destroy(device);
        let refilled = Use {
            generation: 2,
            ..read
        };
        let third = with_hints(hints(refilled, Vec::new()), || {
            BoundImages::create(context, &image(blocks))
        })
        .expect("refilled");
        assert!(third.images[0].image == made, "the same image, refilled");
        assert!(
            third.images[0].staging != ash::vk::Buffer::null(),
            "through a buffer of its own again"
        );
        third.destroy(device);
        super::let_go(device, &[read.key]);
    }

    /// An image stored to with its write-back deferred is not read back after its dispatch, and
    /// is read off the device, its texels as they were uploaded, when its write-back is made; a
    /// later generation than the one kept is not read (D990).
    #[test]
    #[expect(
        clippy::too_many_lines,
        reason = "one scenario, set up and checked end to end"
    )]
    fn a_deferred_image_is_read_when_its_write_back_is_made() {
        if let Availability::Unavailable { reason } = probe() {
            eprintln!("skipped: {reason}");
            return;
        }
        let texels: Vec<u32> = (0..SIDE * SIDE)
            .map(|i| i.wrapping_mul(0x9e37_79b9))
            .collect();
        let stored = Use {
            key: 0xdefe_0990,
            generation: 3,
            fresh: true,
            deferred: true,
        };
        {
            let session = session().expect("a session");
            let session = session
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let Session {
                ref instance,
                physical,
                ref device,
                queue,
                family,
                ..
            } = *session;
            let image = DispatchImage {
                texels: &texels,
                format: DispatchFormat::Rgba8,
                width: SIDE,
                height: SIDE,
                depth: None,
                layers: None,
                sampling: None,
                cube: false,
            };
            let images = DispatchImages {
                selected: None,
                more_fetched: &[],
                fetched: None,
                stored: &[image],
            };
            let hints = Hints {
                uses: Uses {
                    stored: vec![Some(stored)],
                    ..Uses::default()
                },
                dropped: Vec::new(),
            };
            let bound = with_hints(hints, || {
                BoundImages::create((instance, physical, device), &images)
            })
            .expect("made");
            assert!(bound.images[0].deferred());
            // The upload alone, as a dispatch's recording begins.
            // SAFETY: the family is the session device's own, and the create info outlives the call.
            let pool = unsafe {
                device.create_command_pool(
                    &ash::vk::CommandPoolCreateInfo::default().queue_family_index(family),
                    None,
                )
            }
            .expect("pool");
            // SAFETY: the pool is live, and the allocate info outlives the call.
            let command = unsafe {
                device.allocate_command_buffers(
                    &ash::vk::CommandBufferAllocateInfo::default()
                        .command_pool(pool)
                        .level(ash::vk::CommandBufferLevel::PRIMARY)
                        .command_buffer_count(1),
                )
            }
            .expect("command")[0];
            // SAFETY: the command buffer is fresh and not recording.
            unsafe {
                device.begin_command_buffer(command, &ash::vk::CommandBufferBeginInfo::default())
            }
            .expect("begin");
            bound.record_upload(device, command);
            bound.record_readback(device, command);
            // SAFETY: recording is open.
            unsafe { device.end_command_buffer(command) }.expect("end");
            let commands = [command];
            let submits = [ash::vk::SubmitInfo::default().command_buffers(&commands)];
            // SAFETY: the recording is complete and the queue is the session device's.
            unsafe { device.queue_submit(queue, &submits, ash::vk::Fence::null()) }
                .expect("submit");
            // SAFETY: the queue is the session device's, held under the session's lock.
            unsafe { device.queue_wait_idle(queue) }.expect("idle");
            // SAFETY: the queue is idle, so nothing recorded from the pool still runs.
            unsafe { device.destroy_command_pool(pool, None) };
            assert_eq!(
                bound.read_stored(device).expect("read"),
                vec![Vec::<u32>::new()],
                "not read back after the dispatch"
            );
            bound.destroy(device);
        }
        assert!(
            super::read_kept(stored.key, stored.generation + 1).is_none(),
            "a later generation is not kept"
        );
        assert_eq!(
            super::read_kept(stored.key, stored.generation).expect("read off the device"),
            texels
        );
        let session = session().expect("a session");
        let session = session
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        super::let_go(&session.device, &[stored.key]);
    }
}
