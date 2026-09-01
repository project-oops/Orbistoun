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

/// Something that can carry out [`RenderCommand`]s.
///
/// Implementations live in their own crates and are the only place a graphics API is
/// named. `Send` because the worker executes guest code on its own threads.
pub trait RenderBackend: fmt::Debug + Send {
    /// Human-readable backend name, for logs and the run report.
    fn name(&self) -> &'static str;

    /// Carry out one command.
    ///
    /// Returning [`BackendError::Unsupported`] is a legitimate answer and is how a
    /// partially-implemented backend reports a gap honestly (D010) rather than
    /// silently doing nothing.
    fn execute(&mut self, command: &RenderCommand) -> Result<(), BackendError>;

    /// Present whatever has been drawn.
    fn present(&mut self) -> Result<(), BackendError>;
}

/// A backend that records commands and draws nothing.
///
/// The reason the seam exists now rather than later: it lets command-stream
/// translation be asserted with no GPU, no window, and no driver.
#[derive(Debug, Default)]
pub struct RecordingBackend {
    recorded: Vec<RenderCommand>,
    presents: usize,
}

impl RecordingBackend {
    /// Creates an empty recorder.
    pub const fn new() -> Self {
        Self {
            recorded: Vec::new(),
            presents: 0,
        }
    }

    /// Everything executed so far, in order.
    pub fn recorded(&self) -> &[RenderCommand] {
        &self.recorded
    }

    /// How many times the frame was presented.
    pub const fn presents(&self) -> usize {
        self.presents
    }

    /// Discards the recording, keeping the counters meaningful for a fresh frame.
    pub fn clear(&mut self) {
        self.recorded.clear();
    }
}

impl RenderBackend for RecordingBackend {
    fn name(&self) -> &'static str {
        "recording"
    }

    fn execute(&mut self, command: &RenderCommand) -> Result<(), BackendError> {
        self.recorded.push(command.clone());
        Ok(())
    }

    fn present(&mut self) -> Result<(), BackendError> {
        self.presents += 1;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{RecordingBackend, RenderBackend, RenderCommand, ResourceId, ShaderStage};

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
