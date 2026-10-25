//! The images the device keeps for dispatches from one to the next (D985).
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
    /// for at another generation, or let go, is refused (D985).
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
}
