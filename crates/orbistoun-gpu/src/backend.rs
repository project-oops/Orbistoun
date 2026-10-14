//! The render backend seam.
//!
//! The translator turns a guest command stream into [`RenderCommand`]s; a backend crate turns
//! those into whatever its graphics API wants, so `orbistoun-gpu` depends on none. The vocabulary
//! describes what the guest asked for, with no descriptor sets, render passes or barriers, which
//! are one API's model (D029). [`RecordingBackend`] captures what the translator emitted, so
//! translation is tested with no GPU, window or driver.

use core::fmt;

/// Opaque handle to a backend-owned resource.
///
/// The translator mints these and never dereferences them; a backend maps them onto whatever it
/// stores.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ResourceId(pub u64);

/// Which pipeline stage a shader binds to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ShaderStage {
    /// Per-vertex processing.
    Vertex,
    /// Per-fragment processing.
    Fragment,
    /// Compute.
    Compute,
}

/// The index buffer an indexed draw's primitive shader reads its vertex ids from (D740).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndexBuffer {
    /// The guest's indices: their address and bytes per index.
    Guest {
        /// The first index's address.
        address: u64,
        /// Bytes one index takes: two or four.
        bytes: u32,
    },
    /// Thirty-two-bit ids counting up from `first`: a non-indexed draw split into chunks (D741),
    /// each chunk's vertex ids its own run of the draw's.
    Counting {
        /// The first vertex id.
        first: u32,
    },
}

/// A colour attachment's output state past the first one's
/// ([`RenderCommand::SetMoreColourOutputs`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct ColourOutput {
    /// Its blend state, as its `CB_BLENDn_CONTROL` stands; `None` where the stream set none.
    pub blend: Option<crate::registers::BlendControl>,
    /// The channels written, red in bit 0 to alpha in bit 3: its four bits of `CB_TARGET_MASK`,
    /// limited to the channels its format holds.
    pub write_mask: u8,
}

/// A rectangle in render-target space.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    /// Left edge.
    pub x: i32,
    /// Top edge.
    pub y: i32,
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
}

/// How many user-data registers a shader stage has: `SPI_SHADER_USER_DATA_*_0` through `_31`
/// (`src/amd/registers/gfx103.json` in the collection's Mesa tree).
pub const USER_DATA_WORDS: usize = 32;

/// Words in the push-constant block a backend supplies each draw's user data through: thirty-two
/// per stage, vertex first. The translator reads the same layout; a test in `pipeline.rs` pins the two
/// numbers equal, because they live in crates that cannot import each other.
pub const USER_DATA_BLOCK_WORDS: usize = 64;

/// Where a stage's words start in the block, vertex then fragment.
pub const USER_DATA_BLOCK_OFFSETS: [usize; 2] = [0, 32];

/// One thing the guest asked the GPU to do.
///
/// Each variant describes an intent the guest expressed, never a step a host API requires.
// No `Eq`: clear colours are floats, and two clear values that differ by a rounding error are
// different commands.
#[derive(Debug, Clone, PartialEq)]
pub enum RenderCommand {
    /// Direct subsequent drawing at these targets.
    SetRenderTargets {
        /// Colour targets, in slot order.
        colour: Vec<ResourceId>,
        /// Depth/stencil target, if one is bound.
        depth: Option<ResourceId>,
    },
    /// Bind a shader to a stage.
    BindShader {
        /// Stage the shader runs at.
        stage: ShaderStage,
        /// The translated shader.
        shader: ResourceId,
    },
    /// Bind a buffer to a numbered slot for a stage.
    BindBuffer {
        /// Stage that reads the buffer.
        stage: ShaderStage,
        /// Slot index as the guest numbered it.
        slot: u32,
        /// The buffer.
        buffer: ResourceId,
        /// Byte offset into the buffer.
        offset: u64,
        /// Byte length of the bound range.
        length: u64,
    },
    /// Restrict rasterisation to a rectangle.
    SetViewport(Rect),
    /// The transform from clip space to the target's pixels for the draws that follow, as the
    /// stream's `PA_CL_VPORT_*` registers stand at them. Its y scale is negative for a GL guest,
    /// whose NDC `+y` is up.
    SetViewportTransform(crate::registers::ViewportTransform),
    /// The user-data words a stage's shader starts with, for the draws that follow.
    ///
    /// A guest hands each draw its own values through `SPI_SHADER_USER_DATA_*` register writes, and
    /// the hardware loads them into scalar registers before the shader's first instruction. Emitted
    /// before a draw whenever a stage's words changed.
    SetUserData {
        /// The stage whose shader receives them.
        stage: ShaderStage,
        /// Every user-data register of that stage, in order; zero where the stream wrote none.
        words: [u32; USER_DATA_WORDS],
    },
    /// Colour target zero's blend state for the draws that follow, as the stream's
    /// `CB_BLEND0_CONTROL` stands at them.
    ///
    /// Emitted before a draw whenever the value in force changed: blend is per-draw state, not
    /// frame state.
    SetBlend(crate::registers::BlendControl),
    /// The blend constants a constant-colour blend factor reads for the draws that follow, as the
    /// stream's `CB_BLEND_RED`, `_GREEN`, `_BLUE` and `_ALPHA` stand at them: each an `f32`'s bits,
    /// in that order. Emitted before a draw whenever any of them changed.
    SetBlendConstants([u32; 4]),
    /// The raster operation colour target zero's draws that follow write through, as the stream's
    /// `CB_COLOR_CONTROL` `ROP3` stands at them ([`crate::registers::raster_op`]); without one
    /// a draw writes its colour unchanged (`0xCC`). Emitted before a draw whenever it changed.
    SetRasterOp(u8),
    /// The width in pixels of the lines the draws that follow rasterize, as the stream's
    /// `PA_SU_LINE_CNTL` stands at them ([`crate::registers::line_width`]); without one a line is
    /// one pixel wide. Emitted before a draw whenever the register changed. Raw bits, so the
    /// command stays hashable: `WIDTH` as the register holds it.
    SetLineWidth(u16),
    /// The layer the draws that follow render (D859): their primitive shaders keep only the
    /// primitives whose exported layer is this one. `None` keeps every primitive whatever layer it
    /// exports - a draw whose `PA_CL_VS_OUT_CNTL` does not use the layer, and every draw before
    /// the command is first given.
    SetDrawLayer(Option<u32>),
    /// Which channels of colour target zero the draws that follow write, red in bit 0 to alpha in
    /// bit 3: `CB_TARGET_MASK` as it stands at them, limited to the channels the target's format
    /// holds ([`crate::registers::colour_write_mask`]). Emitted before a draw whenever it changed;
    /// without one every channel is written.
    SetWriteMask(u8),
    /// The output state of each colour attachment after the first, in the order
    /// [`Self::SetRenderTargets`] lists them from its second: what [`Self::SetBlend`] and
    /// [`Self::SetWriteMask`] are for the first. Emitted before a draw whenever any changed.
    SetMoreColourOutputs(Vec<ColourOutput>),
    /// Whether the draws that follow count the samples that pass their depth and stencil tests,
    /// as `DB_COUNT_CONTROL` stands at them: `ZPASS_ENABLE` set and `ZPASS_INCREMENT_DISABLE`
    /// clear. A `ZPASS_DONE` reads the count (D827). Emitted before a draw whenever it changed,
    /// and only once a stream turns counting on.
    SetSampleCounting(bool),
    /// The user clip planes the draws that follow are clipped against, as the stream's
    /// `PA_CL_UCP_0` to `_5` stand at them, each four `f32`s' bits; a plane `PA_CL_CLIP_CNTL` does
    /// not enable is zero, which clips nothing (D830). Emitted before a submission's first draw,
    /// and before any draw where they changed.
    SetClipPlanes([[u32; 4]; 6]),
    /// The depth and stencil tests the draws that follow run under, as the stream's
    /// `DB_DEPTH_CONTROL`, `DB_STENCIL_CONTROL` and `DB_STENCILREFMASK` pair stand at them.
    ///
    /// Emitted before a draw whenever the state in force changed, as [`Self::SetBlend`] is. It
    /// applies to the depth target [`Self::SetRenderTargets`] bound; with none bound no test runs.
    SetDepthStencil(crate::depth::DepthStencilState),
    /// Which faces the draws that follow discard and which winding is front, as the stream's
    /// `PA_SU_SC_MODE_CNTL` stands at them. Emitted before a draw whenever it changed.
    SetCull(crate::depth::CullState),
    /// The polygon offset the draws that follow are rasterized under, as the stream's
    /// `PA_SU_SC_MODE_CNTL` and `PA_SU_POLY_OFFSET_*` stand at them. Emitted before a draw whenever
    /// it changed, and only once a stream turns an offset on.
    SetDepthBias(crate::depth::DepthBias),
    /// Set a whole depth target to a value, as the guest cleared it: by a clear draw
    /// (`DB_RENDER_CONTROL`) or by filling its surface.
    ClearDepthStencil {
        /// The depth target, as [`Self::SetRenderTargets`] names it.
        target: ResourceId,
        /// The depth every sample takes, when depth is cleared.
        depth: Option<f32>,
        /// The stencil every sample takes, when stencil is cleared.
        stencil: Option<u8>,
    },
    /// The texture the draws that follow sample, as linear `Rgba8` texels.
    ///
    /// Read out of guest memory from the image descriptor the draw's pixel shader names, only when
    /// its layout is one read exactly (linear, `8_8_8_8_UNORM`, 2D); otherwise no command is
    /// emitted and the backend's default texture stays bound.
    BindTexture {
        /// Which of the fragment module's textures this is: 0 for the first it samples, 1 for a
        /// second, up to four.
        slot: u32,
        /// Texels, row-major and tightly packed, each the guest's four bytes in memory order.
        /// Shared, so a texture bound for many draws is read and held once.
        texels: std::sync::Arc<[u32]>,
        /// [`crate::content_hash`] of the texels, taken once as they were read.
        hash: u64,
        /// Width in texels.
        width: u32,
        /// Height in texels.
        height: u32,
        /// How the draws sample it, from the sampler descriptor the pixel shader names.
        sampling: crate::registers::TextureSampling,
        /// What the words are: texels, or blocks of compressed texels.
        encoding: TextureEncoding,
        /// How many faces the words hold one after another: 1 for a flat image, 6 for a cube,
        /// +x, -x, +y, -y, +z, -z, which the host samples through a cube view (D811).
        faces: u32,
        /// How many mip levels the words hold, finest first: each after the level before, at that
        /// level's side halved and never below one (D833). One for a texture bound as its base
        /// level alone.
        levels: u32,
    },
    /// A three-dimensional texture the draws that follow sample, at a texture slot (D823): `depth`
    /// slices of `width` x `height` texels, one after another, each the guest's four bytes in memory
    /// order, which the host samples as a volume.
    BindVolumeTexture {
        /// Which of the fragment module's textures this is.
        slot: u32,
        /// Texels, row-major, slice after slice, tightly packed.
        texels: std::sync::Arc<[u32]>,
        /// [`crate::content_hash`] of the texels, taken once as they were read.
        hash: u64,
        /// Width in texels.
        width: u32,
        /// Height in texels.
        height: u32,
        /// Slices.
        depth: u32,
        /// How the draws sample it, from the sampler descriptor the pixel shader names.
        sampling: crate::registers::TextureSampling,
        /// What a texel is: `Rgba8`, or `Rgb10A2`.
        encoding: TextureEncoding,
    },
    /// A depth target the draws that follow sample, at a texture slot (D817): the attachment the
    /// backend keeps under `target`, as the draws before left it - compared through `sampling`'s
    /// `depth_compare` where `compare` says, its depth read as it is otherwise. Never read from
    /// guest memory, where a depth target is never written.
    BindDepthTexture {
        /// Which of the fragment module's textures this is.
        slot: u32,
        /// The depth target, [`crate::depth::depth_target_id_at`] its base.
        target: ResourceId,
        /// How the draws sample it, from the sampler descriptor the pixel shader names.
        sampling: crate::registers::TextureSampling,
        /// Whether the draws compare a reference against it, or read its depth.
        compare: bool,
    },
    /// The buffers the draws that follow read through at one stage, in the slots its module
    /// numbered them (D733).
    ///
    /// Each is the guest's bytes over the range the draw's descriptor or base names, read when the
    /// draw was prepared. Emitted before a draw whenever a stage's buffers changed; a stage with
    /// none is sent as an empty list.
    BindDrawBuffers {
        /// The stage whose module reads them.
        stage: ShaderStage,
        /// Each slot's buffer.
        buffers: Vec<DrawBuffer>,
    },
    /// Clear a colour target.
    ClearColour {
        /// Target to clear.
        target: ResourceId,
        /// Clear value, RGBA, linear.
        value: [f32; 4],
    },
    /// Draw without an index buffer.
    Draw {
        /// Vertices per instance.
        vertices: u32,
        /// Number of instances.
        instances: u32,
        /// First vertex.
        first_vertex: u32,
    },
    /// A non-indexed draw split into chunks one wave holds (D741), carried as one command (D787):
    /// chunk `i` is the [`RenderCommand::Draw`] of `vertices` vertices (`last` for the final one)
    /// from `first_vertex + i * stride`. [`RenderCommand::chunk_draws`] gives those draws.
    DrawChunks {
        /// How many chunks.
        chunks: u32,
        /// The vertices each chunk but the last draws.
        vertices: u32,
        /// The vertices the last chunk draws.
        last: u32,
        /// Number of instances, every chunk alike.
        instances: u32,
        /// The first chunk's first vertex.
        first_vertex: u32,
        /// How far each chunk's first vertex lies past the one before.
        stride: u32,
    },
    /// Draw using the bound index buffer.
    DrawIndexed {
        /// Indices per instance.
        indices: u32,
        /// Number of instances.
        instances: u32,
        /// First index.
        first_index: u32,
        /// Where a primitive shader that reads its vertex ids from an index buffer finds them
        /// (D740); `None` where the stream names an index size this does not read.
        index_buffer: Option<IndexBuffer>,
    },
    /// Run a compute workload.
    Dispatch {
        /// Workgroups on X.
        x: u32,
        /// Workgroups on Y.
        y: u32,
        /// Workgroups on Z.
        z: u32,
    },
    /// A guest-visible synchronisation point, carrying the guest's own label so a
    /// trace can be correlated against the command stream that produced it.
    Fence {
        /// Value the guest associated with this point.
        label: u64,
    },
}

impl RenderCommand {
    /// The draws a [`RenderCommand::DrawChunks`] stands for, in order (D787); a command that is not
    /// one yields nothing.
    pub fn chunk_draws(&self) -> impl Iterator<Item = Self> + '_ {
        let (chunks, vertices, last, instances, first_vertex, stride) = match *self {
            Self::DrawChunks {
                chunks,
                vertices,
                last,
                instances,
                first_vertex,
                stride,
            } => (chunks, vertices, last, instances, first_vertex, stride),
            _ => (0, 0, 0, 0, 0, 0),
        };
        (0..chunks).map(move |i| Self::Draw {
            vertices: if i + 1 == chunks { last } else { vertices },
            instances,
            first_vertex: first_vertex.wrapping_add(i.wrapping_mul(stride)),
        })
    }
}

/// One buffer a draw reads through (D733): the guest's bytes from where its descriptor or base
/// starts, as many as the draw may reach.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DrawBuffer {
    /// The bytes, shared so a buffer bound for many draws is read and held once.
    pub bytes: std::sync::Arc<[u8]>,
    /// [`crate::content_hash`] of the bytes, taken once as they were read.
    pub hash: u64,
    /// The guest address the first byte was read from, which a global load's address is read
    /// against (D758); zero for bytes no guest range holds, such as a chunk's counting indices.
    pub base: u64,
}

/// Why a backend could not carry out what it was given.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BackendError {
    /// The backend does not implement this command.
    Unsupported {
        /// Human-readable name of the command that was refused.
        command: &'static str,
    },
    /// A resource id did not refer to anything the backend owns.
    UnknownResource(ResourceId),
    /// The backend's underlying API refused.
    Device(String),
}

impl fmt::Display for BackendError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported { command } => write!(f, "backend does not support {command}"),
            Self::UnknownResource(ResourceId(id)) => write!(f, "unknown resource {id:#x}"),
            Self::Device(msg) => write!(f, "device error: {msg}"),
        }
    }
}

impl std::error::Error for BackendError {}

/// A resource a submission references, handed to the backend to make resident.
///
/// Keyed by a content-addressed [`ResourceId`], so the same resource across frames is created
/// once. The frontend translates and hashes guest resources and extracts their bytes, so the
/// backend is handed plain data and never touches guest memory. A backend matches exhaustively,
/// so a new kind cannot be silently ignored (D701).
#[derive(Debug, Clone, Copy)]
pub enum Resource<'a> {
    /// A translated shader, as SPIR-V words.
    Shader(&'a [u32]),
    /// A buffer the guest allocated, as its bytes - a vertex, index, constant or storage buffer.
    Buffer(&'a [u8]),
    /// A colour render target, by its pixel dimensions.
    ///
    /// It carries no bytes: a backend allocates its own attachment and reads it back, so it needs
    /// only the size. The dimensions are decoded from the register the guest sets
    /// (`CB_COLOR0_ATTRIB2`).
    RenderTarget {
        /// Width in pixels.
        width: u32,
        /// Height in pixels.
        height: u32,
        /// How its texels hold colour, which decides the attachment a draw writes through.
        encoding: TargetEncoding,
    },
}

/// How a colour target's texels hold colour: what a backend draws it through, and what its bytes
/// are when read back. Each is four bytes a texel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum TargetEncoding {
    /// Eight bits a channel, `UNORM`.
    #[default]
    Unorm8,
    /// Eight bits a channel holding sRGB-encoded colour (`NUMBER_SRGB`): a draw blends in linear and
    /// writes the encoding.
    Srgb8,
    /// `COLOR_10_11_11` `FLOAT` (D773): red and green as eleven-bit and blue as ten-bit unsigned
    /// floats, red in the low bits - the packing Vulkan's `B10G11R11_UFLOAT_PACK32` holds.
    Float11_11_10,
    /// `COLOR_32` `FLOAT` (D844): one 32-bit float a texel, red - the bits Vulkan's `R32_SFLOAT`
    /// holds.
    Float32,
    /// `COLOR_16_16_16_16` `FLOAT` (D845): four half floats a texel, red first - the bits
    /// Vulkan's `R16G16B16A16_SFLOAT` holds. Eight bytes a texel.
    Float16x4,
    /// `COLOR_16_16` `FLOAT` (D847): two half floats a texel, red then green - the bits Vulkan's
    /// `R16G16_SFLOAT` holds. Four bytes a texel.
    Float16x2,
    /// `COLOR_2_10_10_10` `UNORM` (D848): red, green and blue in ten bits each from the low end,
    /// alpha in the top two - the bits Vulkan's `A2B10G10R10_UNORM_PACK32` holds.
    Unorm10x3A2,
    /// `COLOR_16_16` `UNORM` (D871): two normalised 16-bit channels a texel, red then green - the
    /// bits Vulkan's `R16G16_UNORM` holds.
    Unorm16x2,
    /// `COLOR_32_32` `FLOAT` (D872): two 32-bit floats a texel, red then green - the bits Vulkan's
    /// `R32G32_SFLOAT` holds. Eight bytes a texel.
    Float32x2,
    /// `COLOR_32_32` `UINT` (D870): two 32-bit unsigned integers a texel, red then green - the bits
    /// Vulkan's `R32G32_UINT` holds. Eight bytes a texel.
    Uint32x2,
    /// `COLOR_8_8` `UNORM` (D869): red and green bytes a texel, which a host carries in each word's
    /// low half and a device draws through Vulkan's `R8G8B8A8_UNORM`, blue zero and alpha one - as
    /// a format without them reads - and written red and green alone.
    Unorm8x2,
    /// `COLOR_16` `FLOAT` (D868): one half float a texel, red, which a host carries in each word's
    /// low half - as [`Self::Uint16`] carries its integer - and a device draws as the 32-bit float
    /// of Vulkan's `R32_SFLOAT`, widened on the way in and rounded back on the way out.
    Float16,
    /// `COLOR_8_8_8_8` `SNORM` (D866): four signed-normalised bytes a texel, red first - the bits
    /// Vulkan's `R8G8B8A8_SNORM` holds.
    Snorm8,
    /// `COLOR_16` `UINT` (D852): one 16-bit unsigned integer a texel, drawn through Vulkan's
    /// `R32_UINT`, each texel a word holding the integer, which write-back stores as its low half.
    Uint16,
    /// `COLOR_32_32_32_32` `FLOAT` (D853): four 32-bit floats a texel, red first - the bits
    /// Vulkan's `R32G32B32A32_SFLOAT` holds. Sixteen bytes a texel.
    Float32x4,
}

/// What a bound texture's words are.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum TextureEncoding {
    /// One word a texel, the guest's four bytes in memory order.
    #[default]
    Rgba8,
    /// `BC3_UNORM` blocks, four words each, a block for every four-by-four texels, rows of blocks
    /// row-major: the guest's compressed bytes as they are, which the host samples natively.
    Bc3,
    /// `BC1_SRGB` blocks, two words each, a block for every four-by-four texels, rows of blocks
    /// row-major: the guest's compressed bytes as they are, which the host samples natively with
    /// the colours decoded from sRGB, as the guest's sampler decodes them.
    Bc1Srgb,
    /// One word a texel, `10_11_11_FLOAT`'s packed unsigned floats as the guest holds them, which
    /// the host samples natively as `B10G11R11_UFLOAT_PACK32` (D774).
    Float11_11_10,
    /// `BC3_SRGB` blocks, four words each, a block for every four-by-four texels, rows of blocks
    /// row-major: the guest's compressed bytes as they are, which the host samples natively with the
    /// colours decoded from sRGB, as the guest's sampler decodes them.
    Bc3Srgb,
    /// `BC6_UFLOAT` blocks, four words each, a block for every four-by-four texels, rows of blocks
    /// row-major: the guest's compressed half floats as they are, which the host samples natively
    /// as `BC6H_UFLOAT_BLOCK`.
    Bc6hUfloat,
    /// One word a texel, `8_8_8_8_SRGB`'s bytes as the guest holds them, which the host samples as
    /// `R8G8B8A8_SRGB`: the colour channels decoded from sRGB to linear as they are read, as the
    /// guest's own sampler decodes them.
    Rgba8Srgb,
    /// One word a texel, the guest's `2_10_10_10_UNORM` as it holds it - red, green and blue ten bits
    /// each from the low end, alpha the top two - which the host samples as
    /// `A2B10G10R10_UNORM_PACK32`. A tiled volume's texels so (D823).
    Rgb10A2,
    /// One word a texel, a depth as an `f32`'s bits, which the host samples as `D32_SFLOAT`: a depth
    /// target read back from the backend's own attachment, its depth read as it is (D817).
    DepthFloat,
    /// [`Self::DepthFloat`] sampled through a compare by the sampler's function (D817).
    DepthCompare,
}

/// Something that can carry out [`RenderCommand`]s.
///
/// Implementations live in their own crates and are the only place a graphics API is
/// named. `Send` because the worker executes guest code on its own threads.
pub trait RenderBackend: fmt::Debug + Send {
    /// Human-readable backend name, for logs and the run report.
    fn name(&self) -> &'static str;

    /// Makes a resource resident, keyed by its content [`ResourceId`].
    ///
    /// Idempotent: a backend that already holds `id` reuses its host object and does not read
    /// `resource` again. Called for every resource a frame references, before the commands that
    /// name it (D701).
    fn ensure_resident(
        &mut self,
        id: ResourceId,
        resource: Resource<'_>,
    ) -> Result<(), BackendError>;

    /// Carry out one command.
    ///
    /// Returning [`BackendError::Unsupported`] is a legitimate answer: it is how a partial backend
    /// reports a gap rather than silently doing nothing (D010).
    fn execute(&mut self, command: &RenderCommand) -> Result<(), BackendError>;

    /// Carry out a [`RenderCommand::DrawChunks`]'s draws (D787), each as
    /// [`RenderCommand::chunk_draws`] gives it. A backend that can take a run of them at once
    /// overrides this: a split draw may be thousands of chunks.
    fn execute_chunks(&mut self, command: &RenderCommand) -> Result<(), BackendError> {
        command
            .chunk_draws()
            .try_for_each(|draw| self.execute(&draw))
    }

    /// Present whatever has been drawn.
    fn present(&mut self) -> Result<(), BackendError>;

    /// Sets the frame's guest-memory window: the region of guest memory the frame's shaders read
    /// and write, as words.
    ///
    /// Frame-level state, set once per frame: every translated module reads guest memory through
    /// one fixed window the submission is compiled against (D703). The default ignores it, for
    /// backends that bind no guest memory.
    fn set_guest_memory(&mut self, _memory: &[u32]) {}

    /// Releases a resource the guest freed.
    ///
    /// The default keeps it: a backend that never evicts is correct until memory pressure, and
    /// eviction is the backend's own concern.
    fn release(&mut self, _id: ResourceId) {}
}

/// A backend that records commands and draws nothing, so command-stream translation can be
/// asserted with no GPU, window or driver.
#[derive(Debug, Default)]
pub struct RecordingBackend {
    recorded: Vec<RenderCommand>,
    presents: usize,
    resident: Vec<ResourceId>,
    guest_memory: Vec<u32>,
}

impl RecordingBackend {
    /// Creates an empty recorder.
    pub const fn new() -> Self {
        Self {
            recorded: Vec::new(),
            presents: 0,
            resident: Vec::new(),
            guest_memory: Vec::new(),
        }
    }

    /// Everything executed so far, in order.
    pub fn recorded(&self) -> &[RenderCommand] {
        &self.recorded
    }

    /// Every resource made resident so far, in order - so a test can assert a frame's resources
    /// reached the backend before the commands that name them.
    pub fn resident(&self) -> &[ResourceId] {
        &self.resident
    }

    /// The guest-memory window the frame set, so a test can assert the driver handed it over.
    pub fn guest_memory(&self) -> &[u32] {
        &self.guest_memory
    }

    /// How many times the frame was presented.
    pub const fn presents(&self) -> usize {
        self.presents
    }

    /// Discards the recording, keeping the counters meaningful for a fresh frame.
    pub fn clear(&mut self) {
        self.recorded.clear();
        self.resident.clear();
        self.guest_memory.clear();
    }
}

impl RenderBackend for RecordingBackend {
    fn name(&self) -> &'static str {
        "recording"
    }

    fn ensure_resident(
        &mut self,
        id: ResourceId,
        _resource: Resource<'_>,
    ) -> Result<(), BackendError> {
        self.resident.push(id);
        Ok(())
    }

    fn execute(&mut self, command: &RenderCommand) -> Result<(), BackendError> {
        self.recorded.push(command.clone());
        Ok(())
    }

    fn present(&mut self) -> Result<(), BackendError> {
        self.presents += 1;
        Ok(())
    }

    fn set_guest_memory(&mut self, memory: &[u32]) {
        self.guest_memory = memory.to_vec();
    }
}

#[cfg(test)]
mod tests {
    use super::{
        RecordingBackend, RenderBackend, RenderCommand, Resource, ResourceId, ShaderStage,
    };

    #[test]
    fn recording_backend_preserves_order() {
        // Order matters: a reordered command stream is a different frame, so the recorder
        // normalises nothing.
        let mut b = RecordingBackend::new();
        b.execute(&RenderCommand::SetViewport(super::Rect {
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
        }))
        .expect("recording never fails");
        b.execute(&RenderCommand::Draw {
            vertices: 3,
            instances: 1,
            first_vertex: 0,
        })
        .expect("recording never fails");

        assert_eq!(b.recorded().len(), 2);
        assert!(matches!(b.recorded()[0], RenderCommand::SetViewport(_)));
        assert!(matches!(b.recorded()[1], RenderCommand::Draw { .. }));
    }

    #[test]
    fn present_is_counted_separately_from_commands() {
        let mut b = RecordingBackend::new();
        b.present().expect("recording never fails");
        b.present().expect("recording never fails");
        assert_eq!(b.presents(), 2);
        assert!(b.recorded().is_empty(), "present is not a command");
    }

    #[test]
    fn clear_drops_commands_but_not_the_present_count() {
        // A frame boundary resets what was drawn, not the frame count, so the counter can detect a
        // stalled presenter.
        let mut b = RecordingBackend::new();
        b.execute(&RenderCommand::Fence { label: 7 })
            .expect("recording never fails");
        b.present().expect("recording never fails");
        b.clear();
        assert!(b.recorded().is_empty());
        assert_eq!(b.presents(), 1);
    }

    #[test]
    fn resources_are_made_resident_in_order_and_cleared_with_the_frame() {
        // Residency is separate from the command stream: resources come first, in the order a frame
        // references them. `clear` resets them like the commands, and `release` is a no-op by
        // default.
        let words = [0x0723_0203_u32];
        let mut b = RecordingBackend::new();
        b.ensure_resident(ResourceId(0x11), Resource::Shader(&words))
            .expect("recording never fails");
        b.ensure_resident(ResourceId(0x22), Resource::Shader(&words))
            .expect("recording never fails");
        b.release(ResourceId(0x11));

        assert_eq!(b.resident(), &[ResourceId(0x11), ResourceId(0x22)]);
        assert!(
            b.recorded().is_empty(),
            "making a resource resident is not a command"
        );

        b.clear();
        assert!(
            b.resident().is_empty(),
            "a fresh frame starts with nothing resident"
        );
    }

    #[test]
    fn commands_carry_guest_intent_not_host_concepts() {
        // The vocabulary carries no host-API concept (D029); this is where one would become
        // awkward.
        let cmd = RenderCommand::BindBuffer {
            stage: ShaderStage::Vertex,
            slot: 3,
            buffer: ResourceId(0xdead_beef),
            offset: 256,
            length: 1024,
        };
        // Slot numbering is the guest's, carried through untranslated.
        match cmd {
            RenderCommand::BindBuffer { slot, offset, .. } => {
                assert_eq!(slot, 3);
                assert_eq!(offset, 256);
            }
            _ => panic!("wrong variant"),
        }
    }
}
