//! The thin driver: one submission through one backend.
//!
//! A [`crate::pipeline::Submission`] carries a frame's resources and the ordered commands that
//! reference them. This runs it against a [`RenderBackend`]: make each resource resident, carry
//! out each command in order, then present. It is deliberately small - the ordering is the
//! frontend's, decoded from the guest's own stream, so a backend stays a recorder and the loop
//! lives here (D701) rather than inside every backend.

use crate::backend::{BackendError, RenderBackend, Resource};
use crate::pipeline::Submission;

/// The result of driving one submission through a backend.
///
/// A frame is legible from this alone: how many resources were made resident, how many commands
/// the backend carried out, and how many it refused - which is a gap it named honestly (D010),
/// not a failure. A frame where `refused` is high and `executed` low is a backend that has not
/// grown the arm the guest needs yet, and the count says so rather than a black screen.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct FrameOutcome {
    /// Resources made resident for this frame.
    pub resident: usize,
    /// Commands the backend carried out.
    pub executed: usize,
    /// Commands the backend refused as unsupported - a named gap, not an error.
    pub refused: usize,
    /// Whether the frame was presented.
    pub presented: bool,
}

/// Drives one submission through a backend: resident, then executed, then presented.
///
/// A refused command is **counted, not fatal**: a partially-implemented backend reports its gaps
/// and the frame continues, which is what lets the executor grow arm by arm while a real guest
/// keeps submitting. A **residency error is fatal** to the frame - a resource that could not be
/// made resident is one the commands cannot reference, so continuing would refuse everything for
/// a reason the caller already has - and a **device error from a command** is fatal too, because
/// it is a real failure rather than a gap the backend chose to name.
pub fn drive(
    backend: &mut dyn RenderBackend,
    submission: &Submission,
) -> Result<FrameOutcome, BackendError> {
    let mut outcome = FrameOutcome::default();

    for (id, spirv) in &submission.modules {
        backend.ensure_resident(*id, Resource::Shader(spirv))?;
        outcome.resident += 1;
    }

    for (id, extent) in &submission.targets {
        backend.ensure_resident(
            *id,
            Resource::RenderTarget {
                width: extent.width,
                height: extent.height,
            },
        )?;
        outcome.resident += 1;
    }

    // The frame's guest-memory window, before the commands that read it. Frame-level state, set once
    // (D703): a shader fetches its vertices from it, so it has to be in place before a draw runs.
    backend.set_guest_memory(&submission.guest_memory);

    for command in &submission.commands {
        match backend.execute(command) {
            Ok(()) => outcome.executed += 1,
            Err(BackendError::Unsupported { .. }) => outcome.refused += 1,
            Err(other) => return Err(other),
        }
    }

    outcome.presented = backend.present().is_ok();
    Ok(outcome)
}

#[cfg(test)]
mod tests {
    use super::{FrameOutcome, drive};
    use crate::backend::{
        BackendError, RecordingBackend, RenderBackend, RenderCommand, Resource, ResourceId,
        ShaderStage,
    };
    use crate::pipeline::Submission;

    /// A submission with one module and a couple of commands.
    fn submission() -> Submission {
        Submission {
            commands: vec![
                RenderCommand::BindShader {
                    stage: ShaderStage::Compute,
                    shader: ResourceId(1),
                },
                RenderCommand::Dispatch { x: 1, y: 1, z: 1 },
            ],
            modules: std::collections::BTreeMap::from([(ResourceId(1), vec![0x0723_0203])]),
            ..Submission::default()
        }
    }

    #[test]
    fn a_recording_backend_takes_the_whole_frame() {
        // The recorder accepts everything, so the outcome is the frame's own shape: one resource,
        // two commands, presented.
        let mut backend = RecordingBackend::new();
        let outcome = drive(&mut backend, &submission()).expect("recording never fails");
        assert_eq!(
            outcome,
            FrameOutcome {
                resident: 1,
                executed: 2,
                refused: 0,
                presented: true,
            }
        );
        assert_eq!(backend.resident(), &[ResourceId(1)]);
        assert_eq!(backend.recorded().len(), 2);
    }

    /// **A render target is made resident alongside the modules, so a `SetRenderTargets` finds it.**
    ///
    /// The target carries no bytes, only its size, and rides in a separate map - but the driver must
    /// still make it resident before the commands run, exactly as it does a shader module (D701).
    /// Made to fail against a driver that made only modules resident: the target would then be
    /// counted `resident: 1`, and a backend selecting it would find nothing.
    #[test]
    fn a_render_target_is_made_resident() {
        use crate::registers::ColourTargetExtent;

        let mut submission = submission();
        let target = ResourceId(0x8000_0000_0040_0040);
        submission.targets.insert(
            target,
            ColourTargetExtent {
                width: 64,
                height: 64,
            },
        );

        let mut backend = RecordingBackend::new();
        let outcome = drive(&mut backend, &submission).expect("recording never fails");
        assert_eq!(
            outcome.resident, 2,
            "the module and the target were both made resident"
        );
        assert!(
            backend.resident().contains(&target),
            "the target reached the backend before the commands: {:?}",
            backend.resident()
        );
    }

    /// **The driver hands the frame's guest-memory window to the backend before the commands.**
    ///
    /// A shader reads its vertices from the window, so it must be set before a draw runs. Made to
    /// fail against a driver that never set it: the recorder would report an empty window.
    #[test]
    fn the_guest_memory_window_reaches_the_backend() {
        let mut submission = submission();
        submission.guest_memory = vec![0xCAFE_F00D, 0x0000_0001];

        let mut backend = RecordingBackend::new();
        drive(&mut backend, &submission).expect("recording never fails");
        assert_eq!(
            backend.guest_memory(),
            &[0xCAFE_F00D, 0x0000_0001],
            "the driver handed the frame's guest memory to the backend"
        );
    }

    /// A backend that makes resources resident but refuses every command as unsupported.
    #[derive(Debug, Default)]
    struct RefusingBackend {
        resident: usize,
    }

    impl RenderBackend for RefusingBackend {
        fn name(&self) -> &'static str {
            "refusing"
        }
        fn ensure_resident(
            &mut self,
            _id: ResourceId,
            _resource: Resource<'_>,
        ) -> Result<(), BackendError> {
            self.resident += 1;
            Ok(())
        }
        fn execute(&mut self, command: &RenderCommand) -> Result<(), BackendError> {
            let _ = command;
            Err(BackendError::Unsupported {
                command: "everything",
            })
        }
        fn present(&mut self) -> Result<(), BackendError> {
            Err(BackendError::Unsupported { command: "present" })
        }
    }

    #[test]
    fn refused_commands_are_counted_and_do_not_abort_the_frame() {
        // A backend with no arms yet still makes resources resident and runs to the end of the
        // stream; the refusals are the report, not a stop.
        let mut backend = RefusingBackend::default();
        let outcome = drive(&mut backend, &submission()).expect("residency succeeded");
        assert_eq!(
            outcome,
            FrameOutcome {
                resident: 1,
                executed: 0,
                refused: 2,
                presented: false,
            }
        );
    }

    /// A backend whose residency fails, to prove that ends the frame.
    #[derive(Debug, Default)]
    struct BrokenResidency;

    impl RenderBackend for BrokenResidency {
        fn name(&self) -> &'static str {
            "broken"
        }
        fn ensure_resident(
            &mut self,
            id: ResourceId,
            _resource: Resource<'_>,
        ) -> Result<(), BackendError> {
            Err(BackendError::UnknownResource(id))
        }
        fn execute(&mut self, _command: &RenderCommand) -> Result<(), BackendError> {
            Ok(())
        }
        fn present(&mut self) -> Result<(), BackendError> {
            Ok(())
        }
    }

    #[test]
    fn a_residency_error_ends_the_frame() {
        // A resource that cannot be made resident is one the commands cannot reference, so the
        // frame stops rather than refusing every command for a reason already known.
        let mut backend = BrokenResidency;
        let error = drive(&mut backend, &submission()).expect_err("residency failed");
        assert_eq!(error, BackendError::UnknownResource(ResourceId(1)));
    }
}
