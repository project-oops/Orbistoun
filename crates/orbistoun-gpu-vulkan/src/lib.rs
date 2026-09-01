//! Vulkan implementation of [`orbistoun_gpu::RenderBackend`].
//!
//! **This is the only crate in the workspace that names a graphics API.** The
//! translator in `orbistoun-gpu` has no dependency on `ash`, so host-API concepts
//! cannot leak into it - `cargo` enforces that, not code review (CLAUDE.md principle
//! 12).
//!
//! # Status
//!
//! **The rendering backend creates no device.** Every command is refused with
//! [`orbistoun_gpu::BackendError::Unsupported`], which is the honest answer (D010) -
//! a backend that silently accepted commands and drew nothing would look like a
//! rendering bug rather than an unimplemented layer. Landing it properly is roadmap
//! phase 6.
//!
//! [`compute`] does create one. Dispatching a translated shader against known inputs and
//! reading the buffer back is the only way to check that a translation computes the right
//! thing rather than merely validating as well-formed, so `ash` is a real dependency now -
//! having deliberately not been one while nothing used Vulkan (D019).
//!
//! The architectural boundary is unaffected either way: it is enforced by `orbistoun-gpu`
//! having **no** path to a graphics API, not by this crate having one.

pub mod compute;
pub use compute::{Availability, DispatchError, Output, dispatch, probe};

use orbistoun_gpu::{BackendError, RenderBackend, RenderCommand};

/// A Vulkan render backend.
///
/// Constructed without a device today; [`VulkanBackend::new`] does not fail because
/// there is nothing yet to fail at. When device creation lands it becomes fallible.
#[derive(Debug, Default)]
pub struct VulkanBackend {
    refused: usize,
}

impl VulkanBackend {
    /// Creates a backend that implements nothing yet.
    pub const fn new() -> Self {
        Self { refused: 0 }
    }

    /// How many commands have been refused.
    ///
    /// Useful before the backend does anything real: it distinguishes "the translator
    /// emitted nothing" from "the translator emitted plenty and none of it landed",
    /// which look identical from a black screen.
    pub const fn refused(&self) -> usize {
        self.refused
    }
}

/// Name of the command variant, for the honest-refusal error.
const fn command_name(command: &RenderCommand) -> &'static str {
    match command {
        RenderCommand::SetRenderTargets { .. } => "SetRenderTargets",
        RenderCommand::BindShader { .. } => "BindShader",
        RenderCommand::BindBuffer { .. } => "BindBuffer",
        RenderCommand::SetViewport(_) => "SetViewport",
        RenderCommand::ClearColour { .. } => "ClearColour",
        RenderCommand::Draw { .. } => "Draw",
        RenderCommand::DrawIndexed { .. } => "DrawIndexed",
        RenderCommand::Dispatch { .. } => "Dispatch",
        RenderCommand::Fence { .. } => "Fence",
    }
}

impl RenderBackend for VulkanBackend {
    fn name(&self) -> &'static str {
        "vulkan"
    }

    fn execute(&mut self, command: &RenderCommand) -> Result<(), BackendError> {
        self.refused += 1;
        Err(BackendError::Unsupported {
            command: command_name(command),
        })
    }

    fn present(&mut self) -> Result<(), BackendError> {
        Err(BackendError::Unsupported { command: "present" })
    }
}

#[cfg(test)]
mod tests {
    use super::VulkanBackend;
    use orbistoun_gpu::{BackendError, RenderBackend, RenderCommand};

    #[test]
    fn every_command_is_refused_by_name_not_silently_dropped() {
        // The failure this guards against: a backend that returns Ok and draws
        // nothing is indistinguishable from a rendering bug. Refusal must name the
        // command so a report says which capability is missing.
        let mut b = VulkanBackend::new();
        let err = b
            .execute(&RenderCommand::Draw {
                vertices: 3,
                instances: 1,
                first_vertex: 0,
            })
            .expect_err("nothing is implemented yet");
        assert_eq!(err, BackendError::Unsupported { command: "Draw" });
    }

    #[test]
    fn refusals_are_counted() {
        // Distinguishes "translator emitted nothing" from "translator emitted plenty
        // and none of it landed" - identical from a black screen otherwise.
        let mut b = VulkanBackend::new();
        for _ in 0..3 {
            let _ = b.execute(&RenderCommand::Fence { label: 1 });
        }
        assert_eq!(b.refused(), 3);
    }

    #[test]
    fn present_is_refused_too() {
        let mut b = VulkanBackend::new();
        assert!(b.present().is_err());
        assert_eq!(b.name(), "vulkan");
    }
}
