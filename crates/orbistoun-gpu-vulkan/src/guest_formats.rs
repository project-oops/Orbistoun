//! A guest image format on the device (D923): the Vulkan format that holds its bits as they are
//! where the device samples it, and otherwise the format `orbistoun_gpu::formats::decode` widens it
//! to on the CPU.
//!
//! Every format named here is the Vulkan format of the same name (Vulkan section 49, "Formats");
//! which guest code each stands for, and where the guest's channels lie in it, is
//! `orbistoun_gpu::formats`'s to say. Whether the device samples a format is asked of it
//! (`vkGetPhysicalDeviceFormatProperties`, `SAMPLED_IMAGE` with optimal tiling), once a format.

use ash::vk;
use orbistoun_gpu::formats::{self, HostChannel, HostFormat, Swizzle, Widened};

use crate::compute::DispatchError;

/// The Vulkan format `format` names.
#[allow(clippy::too_many_lines, reason = "one table of formats, a line each")]
pub(crate) const fn vk_format(format: HostFormat) -> vk::Format {
    use HostFormat as H;
    match format {
        H::R8Unorm => vk::Format::R8_UNORM,
        H::R8Snorm => vk::Format::R8_SNORM,
        H::R8Uint => vk::Format::R8_UINT,
        H::R8Sint => vk::Format::R8_SINT,
        H::R8Srgb => vk::Format::R8_SRGB,
        H::R8G8Unorm => vk::Format::R8G8_UNORM,
        H::R8G8Snorm => vk::Format::R8G8_SNORM,
        H::R8G8Uint => vk::Format::R8G8_UINT,
        H::R8G8Sint => vk::Format::R8G8_SINT,
        H::R8G8Srgb => vk::Format::R8G8_SRGB,
        H::R8G8B8A8Unorm => vk::Format::R8G8B8A8_UNORM,
        H::R8G8B8A8Snorm => vk::Format::R8G8B8A8_SNORM,
        H::R8G8B8A8Uint => vk::Format::R8G8B8A8_UINT,
        H::R8G8B8A8Sint => vk::Format::R8G8B8A8_SINT,
        H::R8G8B8A8Srgb => vk::Format::R8G8B8A8_SRGB,
        H::R16Unorm => vk::Format::R16_UNORM,
        H::R16Snorm => vk::Format::R16_SNORM,
        H::R16Uint => vk::Format::R16_UINT,
        H::R16Sint => vk::Format::R16_SINT,
        H::R16Sfloat => vk::Format::R16_SFLOAT,
        H::R16G16Unorm => vk::Format::R16G16_UNORM,
        H::R16G16Snorm => vk::Format::R16G16_SNORM,
        H::R16G16Uint => vk::Format::R16G16_UINT,
        H::R16G16Sint => vk::Format::R16G16_SINT,
        H::R16G16Sfloat => vk::Format::R16G16_SFLOAT,
        H::R16G16B16A16Unorm => vk::Format::R16G16B16A16_UNORM,
        H::R16G16B16A16Snorm => vk::Format::R16G16B16A16_SNORM,
        H::R16G16B16A16Uint => vk::Format::R16G16B16A16_UINT,
        H::R16G16B16A16Sint => vk::Format::R16G16B16A16_SINT,
        H::R16G16B16A16Sfloat => vk::Format::R16G16B16A16_SFLOAT,
        H::R32Uint => vk::Format::R32_UINT,
        H::R32Sint => vk::Format::R32_SINT,
        H::R32Sfloat => vk::Format::R32_SFLOAT,
        H::R32G32Uint => vk::Format::R32G32_UINT,
        H::R32G32Sint => vk::Format::R32G32_SINT,
        H::R32G32Sfloat => vk::Format::R32G32_SFLOAT,
        H::R32G32B32A32Uint => vk::Format::R32G32B32A32_UINT,
        H::R32G32B32A32Sint => vk::Format::R32G32B32A32_SINT,
        H::R32G32B32A32Sfloat => vk::Format::R32G32B32A32_SFLOAT,
        H::A2B10G10R10UnormPack32 => vk::Format::A2B10G10R10_UNORM_PACK32,
        H::A2B10G10R10SnormPack32 => vk::Format::A2B10G10R10_SNORM_PACK32,
        H::A2B10G10R10UintPack32 => vk::Format::A2B10G10R10_UINT_PACK32,
        H::A2B10G10R10SintPack32 => vk::Format::A2B10G10R10_SINT_PACK32,
        H::B10G11R11UfloatPack32 => vk::Format::B10G11R11_UFLOAT_PACK32,
        H::E5B9G9R9UfloatPack32 => vk::Format::E5B9G9R9_UFLOAT_PACK32,
        H::R5G6B5UnormPack16 => vk::Format::R5G6B5_UNORM_PACK16,
        H::A1R5G5B5UnormPack16 => vk::Format::A1R5G5B5_UNORM_PACK16,
        H::R5G5B5A1UnormPack16 => vk::Format::R5G5B5A1_UNORM_PACK16,
        H::B4G4R4A4UnormPack16 => vk::Format::B4G4R4A4_UNORM_PACK16,
        H::R4G4UnormPack8 => vk::Format::R4G4_UNORM_PACK8,
        H::Bc1RgbaUnormBlock => vk::Format::BC1_RGBA_UNORM_BLOCK,
        H::Bc1RgbaSrgbBlock => vk::Format::BC1_RGBA_SRGB_BLOCK,
        H::Bc2UnormBlock => vk::Format::BC2_UNORM_BLOCK,
        H::Bc2SrgbBlock => vk::Format::BC2_SRGB_BLOCK,
        H::Bc3UnormBlock => vk::Format::BC3_UNORM_BLOCK,
        H::Bc3SrgbBlock => vk::Format::BC3_SRGB_BLOCK,
        H::Bc4UnormBlock => vk::Format::BC4_UNORM_BLOCK,
        H::Bc4SnormBlock => vk::Format::BC4_SNORM_BLOCK,
        H::Bc5UnormBlock => vk::Format::BC5_UNORM_BLOCK,
        H::Bc5SnormBlock => vk::Format::BC5_SNORM_BLOCK,
        H::Bc6hUfloatBlock => vk::Format::BC6H_UFLOAT_BLOCK,
        H::Bc6hSfloatBlock => vk::Format::BC6H_SFLOAT_BLOCK,
        H::Bc7UnormBlock => vk::Format::BC7_UNORM_BLOCK,
        H::Bc7SrgbBlock => vk::Format::BC7_SRGB_BLOCK,
    }
}

/// The Vulkan format a CPU decode's words are.
const fn vk_widened(widened: Widened) -> vk::Format {
    match widened {
        Widened::Rgba8Unorm => vk::Format::R8G8B8A8_UNORM,
        Widened::Rgba8Srgb => vk::Format::R8G8B8A8_SRGB,
        Widened::Rgba16Float => vk::Format::R16G16B16A16_SFLOAT,
        Widened::Rgba32Float => vk::Format::R32G32B32A32_SFLOAT,
        Widened::Rgba32Uint => vk::Format::R32G32B32A32_UINT,
        Widened::Rgba32Sint => vk::Format::R32G32B32A32_SINT,
    }
}

/// A view's component swizzle for one channel of a sample.
const fn component(swizzle: Swizzle) -> vk::ComponentSwizzle {
    match swizzle {
        Swizzle::Zero => vk::ComponentSwizzle::ZERO,
        Swizzle::One => vk::ComponentSwizzle::ONE,
        Swizzle::Channel(HostChannel::R) => vk::ComponentSwizzle::R,
        Swizzle::Channel(HostChannel::G) => vk::ComponentSwizzle::G,
        Swizzle::Channel(HostChannel::B) => vk::ComponentSwizzle::B,
        // `view` never answers a missing channel.
        Swizzle::Channel(HostChannel::A | HostChannel::Missing) => vk::ComponentSwizzle::A,
    }
}

std::thread_local! {
    /// Whether this thread's guest-format textures are widened whatever the device samples.
    static WIDEN: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Runs `run` with every guest-format texture this thread makes widened on the CPU, as on a device
/// that samples none of them natively: how a test reaches the fallback on a device that has them.
pub fn widening<T>(run: impl FnOnce() -> T) -> T {
    let before = WIDEN.with(|widen| widen.replace(true));
    let out = run();
    WIDEN.with(|widen| widen.set(before));
    out
}

/// Whether the device samples `format` from an optimally tiled image, asked once a format.
fn samples(instance: &ash::Instance, physical: vk::PhysicalDevice, format: vk::Format) -> bool {
    static ASKED: std::sync::OnceLock<std::sync::Mutex<std::collections::HashMap<i32, bool>>> =
        std::sync::OnceLock::new();
    let mut asked = ASKED
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    *asked.entry(format.as_raw()).or_insert_with(|| {
        // SAFETY: the physical device belongs to this instance, which is live.
        let properties =
            unsafe { instance.get_physical_device_format_properties(physical, format) };
        properties
            .optimal_tiling_features
            .contains(vk::FormatFeatureFlags::SAMPLED_IMAGE | vk::FormatFeatureFlags::TRANSFER_DST)
    })
}

/// Words a `width` x `height` image of `layers` layers of guest format `code` takes, its texels or
/// blocks tightly packed, each layer's after the one before; `None` for a code not sampled.
#[must_use]
pub fn guest_words(code: u16, (width, height, layers): (u32, u32, u32)) -> Option<usize> {
    let layout = formats::sampled(u32::from(code)).ok()?.layout;
    Some((layer_bytes(layout, width, height) * layers as usize).div_ceil(4))
}

/// Bytes one layer of `layout` takes, its elements tightly packed.
fn layer_bytes(layout: formats::Layout, width: u32, height: u32) -> usize {
    let extent = layout.element_extent();
    width.div_ceil(extent) as usize
        * height.div_ceil(extent) as usize
        * layout.element_bytes() as usize
}

/// A guest-format texture as made on this device: its Vulkan format, its words - the guest's own,
/// or widened - and its view's components under the guest's selects `selects`.
pub(crate) fn staged<'a>(
    (instance, physical): (&ash::Instance, vk::PhysicalDevice),
    words: &'a [u32],
    (width, height, layers): (u32, u32, u32),
    (code, selects): (u16, [u8; 4]),
) -> Result<
    (
        vk::Format,
        std::borrow::Cow<'a, [u32]>,
        vk::ComponentMapping,
    ),
    DispatchError,
> {
    let sampled = formats::sampled(u32::from(code))
        .map_err(|why| DispatchError::Unsupported(why.to_string()))?;
    let widen = WIDEN.with(std::cell::Cell::get);
    let native = sampled
        .native
        .map(vk_format)
        .filter(|&format| !widen && samples(instance, physical, format));
    let selects = selects.map(u32::from);
    let view = sampled.view(selects, native.is_some()).ok_or_else(|| {
        DispatchError::Unsupported(format!(
            concat!(
                "image format {} under selects {:?}: a reserved select, or one naming a channel ",
                "the format does not hold"
            ),
            formats::name(u32::from(code)).unwrap_or("unnamed"),
            selects
        ))
    })?;
    let [r, g, b, a] = view.map(component);
    let components = vk::ComponentMapping { r, g, b, a };
    if let Some(format) = native {
        return Ok((format, std::borrow::Cow::Borrowed(words), components));
    }
    let bytes: Vec<u8> = words.iter().flat_map(|word| word.to_le_bytes()).collect();
    let each = layer_bytes(sampled.layout, width, height);
    let mut widened = Vec::new();
    for layer in 0..layers as usize {
        let from = bytes.get(layer * each..).unwrap_or(&[]);
        let decoded = formats::decode(&sampled, from, (width, height)).ok_or_else(|| {
            DispatchError::Unsupported(format!(
                "a guest-format texture of {} words, short of its {width} x {height} x {layers}",
                words.len()
            ))
        })?;
        widened.extend(decoded);
    }
    Ok((
        vk_widened(sampled.widened),
        std::borrow::Cow::Owned(widened),
        components,
    ))
}
