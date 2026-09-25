//! The render backend seam.
//!
//! This module is why `orbistoun-gpu` has no dependency on any graphics API. The
//! translator turns a guest command stream into [`RenderCommand`]s; a backend turns
//! those into whatever its API wants. Adding a second backend is a new crate, not
//! surgery here.
//!
//! # Abstracted at guest semantics, not host API
//!
//! Per CLAUDE.md principle 12, the vocabulary below describes **what the guest asked
//! for**. It deliberately contains no descriptor sets, render passes, or barriers -
//! those are one API's model, and baking them in would make a second backend fit
//! badly while pretending to be abstract.
//!
//! The vocabulary is small on purpose and will grow as the translator learns to
//! recognise more of the command stream. It is not a complete graphics API and is not
//! trying to be.
//!
//! # It pays rent immediately
//!
//! [`RecordingBackend`] captures what the translator emitted, so translation can be
//! tested with no GPU, no window, and no driver - on CI and in a VM. That is the
//! justification for the seam existing now rather than later.

use core::fmt;

/// Opaque handle to a backend-owned resource.
///
/// The translator mints these and never dereferences them; a backend maps them onto
/// whatever it actually stores. Deliberately not a pointer.
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

/// Words in the push-constant block a backend supplies each draw's user data through (worklog 826):
/// sixteen per stage, vertex first. The translator reads the same layout; a test in `pipeline.rs`
/// pins the two numbers equal, because they live in crates that must not import each other's.
pub const USER_DATA_BLOCK_WORDS: usize = 32;

/// Where a stage's words start in the block, vertex then fragment.
pub const USER_DATA_BLOCK_OFFSETS: [usize; 2] = [0, 16];

/// One thing the guest asked the GPU to do.
///
/// Grows as the translator recognises more of the command stream. Each variant should
/// describe an intent the guest expressed, never a step some host API happens to
/// require.
// No `Eq`: clear colours are floats. Deriving `PartialEq` only is correct here rather
// than a limitation - two clear values that differ by a rounding error are not the
// same command, and pretending otherwise would hide a real translation difference.
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
    /// stream's `PA_CL_VPORT_*` registers stand at them (worklog 837). Its y scale is negative for a
    /// GL guest, whose NDC `+y` is up.
    SetViewportTransform(crate::registers::ViewportTransform),
    /// The user-data words a stage's shader starts with, for the draws that follow (worklog 825).
    ///
    /// A guest hands each draw its own values - the GL cube passes its vertex offset this way, a
    /// different one per draw, and its pixel shader the address of its texture descriptors - through
    /// `SPI_SHADER_USER_DATA_*` register writes, and the hardware loads them into scalar registers
    /// before the shader's first instruction. Emitted before a draw whenever a stage's words changed.
    SetUserData {
        /// The stage whose shader receives them.
        stage: ShaderStage,
        /// Every user-data register of that stage, in order; zero where the stream wrote none.
        words: [u32; USER_DATA_WORDS],
    },
    /// Colour target zero's blend state for the draws that follow, as the stream's
    /// `CB_BLEND0_CONTROL` stands at them (`REQ-...2ea9`, worklog 829).
    ///
    /// Emitted before a draw whenever the register's value in force changed. A translucent quad and
    /// the opaque geometry before it are the same kind of draw with different blend state, so this is
    /// per-draw state, not frame state.
    SetBlend(crate::registers::BlendControl),
    /// The texture the draws that follow sample, as linear `Rgba8` texels (worklog 828).
    ///
    /// Read out of guest memory from the image descriptor the draw's pixel shader names - only when
    /// its layout is one read exactly (linear, `8_8_8_8_UNORM`, 2D); otherwise no command is emitted
    /// and the backend's default texture stays bound.
    BindTexture {
        /// Which of the fragment module's textures this is (worklog 840): 0 for the first it
        /// samples, 1 for a second.
        slot: u32,
        /// Texels, row-major and tightly packed, each the guest's four bytes in memory order - shared,
        /// so a texture bound for many draws is read and held once (worklog 844).
        texels: std::sync::Arc<[u32]>,
        /// [`crate::content_hash`] of the texels, taken once as they were read.
        hash: u64,
        /// Width in texels.
        width: u32,
        /// Height in texels.
        height: u32,
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
    /// Draw using the bound index buffer.
    DrawIndexed {
        /// Indices per instance.
        indices: u32,
        /// Number of instances.
        instances: u32,
        /// First index.
        first_index: u32,
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

/// Why a backend could not carry out what it was given.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BackendError {
    /// The backend does not implement this command yet.
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
/// Keyed by a content-addressed [`ResourceId`], so the same resource across frames is recognised
/// and created once - the "translate once" cache extended to the host as "upload once". The guest
/// gives its resources by address; the frontend translates and content-hashes them into ids and
/// extracts their bytes, so the backend is handed plain data and never touches guest memory (which
/// is what keeps this crate free of any graphics API - D029).
///
/// It grows a variant as the decode side learns to extract another kind - a render target's
/// dimensions now, textures next - the same way [`RenderCommand`] grows as intents are recognised. A
/// backend matches exhaustively, so a new kind cannot be silently ignored (D701).
#[derive(Debug, Clone, Copy)]
pub enum Resource<'a> {
    /// A translated shader, as SPIR-V words.
    Shader(&'a [u32]),
    /// A buffer the guest allocated, as its bytes - a vertex, index, constant or storage buffer.
    Buffer(&'a [u8]),
    /// A colour render target, by its pixel dimensions.
    ///
    /// It carries no bytes: a backend that draws into an attachment allocates its own and reads it
    /// back, so what it needs from the frontend is the size to allocate, not the guest's pixels. The
    /// dimensions are decoded from the register the guest sets (`CB_COLOR0_ATTRIB2`), which is why
    /// this crate can hand them over without naming a graphics API.
    RenderTarget {
        /// Width in pixels.
        width: u32,
        /// Height in pixels.
        height: u32,
    },
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
    /// **Idempotent.** A backend that already holds `id` reuses its host object and does not read
    /// `resource` again. Called for every resource a frame references, before the commands that
    /// name it, so a `BindShader` and a draw find a host object already built (D701).
    fn ensure_resident(
        &mut self,
        id: ResourceId,
        resource: Resource<'_>,
    ) -> Result<(), BackendError>;

    /// Carry out one command.
    ///
    /// Returning [`BackendError::Unsupported`] is a legitimate answer and is how a
    /// partially-implemented backend reports a gap honestly (D010) rather than
    /// silently doing nothing.
    fn execute(&mut self, command: &RenderCommand) -> Result<(), BackendError>;

    /// Present whatever has been drawn.
    fn present(&mut self) -> Result<(), BackendError>;

    /// Sets the frame's guest-memory window: the region of guest memory the frame's shaders read
    /// and write, as words.
    ///
    /// Frame-level state, not a command and not a per-slot binding: every translated module reads
    /// guest memory through one fixed window the whole submission is compiled against (worklog 635),
    /// so the region is set once per frame rather than named by each draw. The default ignores it -
    /// a recorder and a backend that binds no guest memory have nothing to do with it - and a
    /// backend that renders a guest's geometry binds it where its shaders expect it (D703).
    fn set_guest_memory(&mut self, _memory: &[u32]) {}

    /// Releases a resource the guest freed.
    ///
    /// The default keeps it: a backend that never evicts is correct until memory pressure, and
    /// eviction is the backend's own concern (D701).
    fn release(&mut self, _id: ResourceId) {}
}

/// A backend that records commands and draws nothing.
///
/// The reason the seam exists now rather than later: it lets command-stream
/// translation be asserted with no GPU, no window, and no driver.
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
        // Order is the property that matters: a command stream reordered is a
        // different frame, so the test double must not normalise anything.
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
        // A frame boundary resets what was drawn, not how many frames have gone by -
        // otherwise the counter cannot be used to detect a stalled presenter.
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
        // The residency call is separate from the command stream: resources come first, in the
        // order a frame references them, so a backend has its host objects before the commands
        // that name them. `clear` resets them like the commands, and `release` is a no-op by
        // default, so a backend that never evicts is still correct.
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
        // Guard against the failure mode CLAUDE.md principle 12 warns about: if this
        // vocabulary ever grows a host-API concept, this test is where it should
        // become awkward to express.
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
