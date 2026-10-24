//! Times the driver's compile of one translated module, for the shader census
//! ([`orbistoun_gpu::shader_census`]).
//!
//! A compute module is compiled as a guest dispatch's pipeline is, through the same layout, but
//! with no pipeline cache, so the time is the driver's whole compile, and outside the compile
//! watch, so several threads may compile at once. A draw's pipeline needs state the kept
//! translations do not hold, so a draw's module is timed as a shader module alone.

use std::time::Instant;

use ash::vk;
use orbistoun_gpu::shader_census::{Compiled, Program, Timed};

use crate::compute::{
    Compile, DispatchError, GlobalShare, for_this_device, make_pipeline, session,
};

/// Compiles `program` on the session's device and says how long the driver took. A refusal is
/// timed too, and named.
pub fn time_compile(program: &Program<'_>) -> Compiled {
    let timed = program.timed();
    let started = Instant::now();
    let made = compile(program, timed);
    Compiled {
        seconds: started.elapsed().as_secs_f64(),
        timed,
        failed: made.err().map(|e| e.to_string()),
    }
}

fn compile(program: &Program<'_>, timed: Timed) -> Result<(), DispatchError> {
    // A clone of the device's dispatch table, so no compile holds the session's lock.
    let device = session()?
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .device
        .clone();
    let module = for_this_device(program.module);
    if timed == Timed::Module {
        return module_alone(&device, &module);
    }
    let share = GlobalShare::for_module(&module)?;
    let images = crate::compute_images::layout_bindings_for(program.fetched, program.stored);
    // With the buffer set, which a module reaching a dispatch's buffers declares; one that does
    // not leaves it unused.
    make_pipeline(
        &device,
        &module,
        (&images, &share),
        (true, 0),
        Compile::Alone,
    )?
    .release(&device);
    Ok(())
}

/// Creates and releases `module` as a shader module.
fn module_alone(device: &ash::Device, module: &[u32]) -> Result<(), DispatchError> {
    if !orbistoun_spirv::well_formed(module) {
        return Err(DispatchError::Vulkan(
            "create_shader_module",
            vk::Result::ERROR_INVALID_SHADER_NV,
        ));
    }
    let info = vk::ShaderModuleCreateInfo::default().code(module);
    // SAFETY: the words outlive the call and are laid out as SPIR-V.
    let shader = unsafe { device.create_shader_module(&info, None) }
        .map_err(|e| DispatchError::Vulkan("create_shader_module", e))?;
    // SAFETY: created just above on this device, used by nothing, and destroyed once.
    unsafe { device.destroy_shader_module(shader, None) };
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::time_compile;
    use crate::compute::{Availability, probe};
    use orbistoun_gpu::shader_census::{Program, Timed};
    use orbistoun_translate::wavefront::Stage;

    /// A compute module is compiled whole as a pipeline and a draw's as a module, each timed; a
    /// module the device cannot take is named, not timed as a success.
    #[test]
    fn a_small_module_compiles_and_is_timed() {
        if let Availability::Unavailable { reason } = probe() {
            println!("[shader census] SKIPPED - no device: {reason}");
            return;
        }
        let module = orbistoun_spirv::minimal_compute_module([1, 1, 1]);
        let program = |stage, module| Program {
            stage,
            module,
            fetched: 1,
            stored: 1,
        };
        let compute = time_compile(&program(Stage::Compute, &module));
        assert_eq!((compute.timed, compute.failed), (Timed::Pipeline, None));
        assert!(compute.seconds > 0.0);
        let fragment = orbistoun_spirv::passthrough_fragment_module();
        let draw = time_compile(&program(Stage::Fragment, &fragment));
        assert_eq!((draw.timed, draw.failed), (Timed::Module, None));
        let broken = time_compile(&program(Stage::Compute, &[1, 2, 3]));
        assert!(broken.failed.is_some());
    }
}
