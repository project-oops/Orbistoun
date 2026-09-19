//! Vulkan implementation of [`orbistoun_gpu::RenderBackend`].
//!
//! **This is the only crate in the workspace that names a graphics API.** The
//! translator in `orbistoun-gpu` has no dependency on `ash`, so host-API concepts
//! cannot leak into it - `cargo` enforces that, not code review (CLAUDE.md principle
//! 12).
//!
//! # Status
//!
//! The backend makes resources resident and **executes**: a `Dispatch` runs a bound compute shader
//! (into a resident buffer or an interim throwaway) and reads it back; a `SetRenderTargets` selects
//! a resident colour target; and a `Draw` runs a bound geometry+fragment pipeline into an attachment
//! sized from that target - the guest's decoded dimensions, or an interim square when none is set -
//! and reads the frame back (D701). The geometry stage is drawn as a mesh or a vertex shader
//! depending on the bound module's own execution model - a guest's geometry translates to a mesh
//! shader (D688) - and a mesh module on a device with no mesh stage is refused rather than issued. An
//! indexed `Draw` takes the same path, because a guest's indexed geometry is a mesh shader that reads
//! its own indices from the window (worklog 643). A `SetViewport` restricts a following draw to a
//! rectangle (worklog 644). Only a `ClearColour` is still refused with
//! [`orbistoun_gpu::BackendError::Unsupported`], the honest answer (D010) while its register oracle
//! is not in hand (D702): a backend that silently accepted a command and drew nothing would look like
//! a rendering bug rather than an unimplemented layer.
//!
//! The device is opened lazily, on the first resource made resident, through the same shared session
//! [`compute`] and [`framebuffer`] use. `ash` is a real dependency here, deliberately not one while
//! nothing used Vulkan (D019).
//!
//! The architectural boundary is unaffected: it is enforced by `orbistoun-gpu` having **no** path to
//! a graphics API, not by this crate having one.

pub mod compute;
pub mod framebuffer;
pub use compute::{Availability, DispatchError, Output, dispatch, probe};

use std::collections::BTreeMap;

use ash::vk;
use orbistoun_gpu::{
    BackendError, Rect, RenderBackend, RenderCommand, Resource, ResourceId, ShaderStage,
};

/// The interim width of a compute dispatch's output buffer, in `u32`s.
///
/// A guest dispatch's buffers are resources it binds, and their sizes come from the guest. Until
/// the buffer arm lands (D701), a dispatch runs a compute shader that writes its own storage into
/// a buffer of this fixed width and reads it back - enough to execute a translated shader end to
/// end, not yet enough to run a guest's.
const DISPATCH_WORDS: usize = 64;

/// The interim size of the colour attachment a `Draw` renders into.
///
/// A guest's render target has its own dimensions, decoded from the CB registers a stream sets;
/// until that decode lands a draw renders into a fixed square, enough to run a translated
/// vertex+fragment pipeline end to end and read the frame back.
const RENDER_WIDTH: u32 = 64;
/// See [`RENDER_WIDTH`].
const RENDER_HEIGHT: u32 = 64;
/// The colour a `Draw` clears its attachment to before drawing - opaque black.
const CLEAR_COLOUR: [f32; 4] = [0.0, 0.0, 0.0, 1.0];

/// The first word of a SPIR-V module, and the `OpEntryPoint` opcode and mesh execution model, from
/// the Khronos SPIR-V specification. **Transcribed from an open standard, not the guest's** - so
/// unlike a register offset these are documented constants, not a hypothesis.
const SPIRV_MAGIC: u32 = 0x0723_0203;
/// `OpEntryPoint`'s opcode, in the low sixteen bits of its first word.
const OP_ENTRY_POINT: u16 = 15;
/// The `MeshEXT` execution model, operand one of a mesh module's `OpEntryPoint`.
const EXECUTION_MODEL_MESH_EXT: u32 = 5365;

/// Whether a translated module's entry point is a mesh shader rather than a vertex one.
///
/// A guest's geometry stage translates to a mesh shader (D688) and a fullscreen-triangle test shader
/// to a vertex one; the two are drawn by different calls, and the module itself is what says which -
/// the guest bound a `Vertex` stage either way, and the host mesh-ness is a translation detail this
/// crate owns, not the frontend's to carry (principle 12).
///
/// Reads the execution model of the first `OpEntryPoint` (its operand one), walking the instruction
/// stream after the five-word header. Anything that is not a SPIR-V module, or names no entry point,
/// is not a mesh module - it is the vertex path's to reject if it is malformed.
fn is_mesh_module(spirv: &[u32]) -> bool {
    if spirv.first() != Some(&SPIRV_MAGIC) {
        return false;
    }
    // Past the header: magic, version, generator, id bound, schema.
    let mut index = 5;
    while let Some(&word) = spirv.get(index) {
        let word_count = (word >> 16) as usize;
        // A zero-length instruction would never advance the walk; treat the module as malformed
        // rather than loop forever.
        if word_count == 0 {
            return false;
        }
        if (word & 0xFFFF) as u16 == OP_ENTRY_POINT {
            return spirv.get(index + 1).copied() == Some(EXECUTION_MODEL_MESH_EXT);
        }
        index += word_count;
    }
    false
}

/// The Vulkan scissor a guest's viewport rectangle names.
///
/// A plain repackaging: the guest's rectangle in its target's pixels becomes the rectangle
/// rasterisation is restricted to. It is clamped to the attachment where it is used, not here.
fn scissor_of(rect: Rect) -> vk::Rect2D {
    vk::Rect2D {
        offset: vk::Offset2D {
            x: rect.x,
            y: rect.y,
        },
        extent: vk::Extent2D {
            width: rect.width,
            height: rect.height,
        },
    }
}

/// Uploads bytes into a fresh host-visible buffer on the session's device.
///
/// The buffer arm of `ensure_resident` and the guest-memory window (D703) both create a buffer this
/// way, so the one unsafe upload lives here. At least one word, because a zero-sized buffer is not a
/// legal binding and an empty window still needs one.
fn upload_host_buffer(
    session: &compute::Session,
    bytes: &[u8],
) -> Result<compute::DispatchBuffer, BackendError> {
    let words = bytes.len().div_ceil(4).max(1);
    let size = (words * 4) as vk::DeviceSize;
    let (buffer, memory) = compute::create_host_buffer(
        &session.instance,
        session.physical,
        &session.device,
        size,
        words,
    )
    .map_err(|e| BackendError::Device(format!("create_host_buffer: {e:?}")))?;
    // SAFETY: the memory is host-visible and coherent, just created and not mapped.
    let mapped = unsafe {
        session
            .device
            .map_memory(memory, 0, size, vk::MemoryMapFlags::empty())
    }
    .map_err(|e| BackendError::Device(format!("map_memory: {e:?}")))?;
    // SAFETY: `mapped` covers `size` bytes, which is at least `bytes.len()`, and this process has
    // exclusive access while it is mapped.
    unsafe {
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), mapped.cast::<u8>(), bytes.len());
    }
    // SAFETY: mapped immediately above; the pointer is not used after unmapping.
    unsafe { session.device.unmap_memory(memory) };
    Ok(compute::DispatchBuffer {
        buffer,
        memory,
        size,
        words,
    })
}

/// A content hash of the guest-memory window, so a stable window uploads once (D703).
fn window_hash(words: &[u32]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    words.hash(&mut hasher);
    hasher.finish()
}

/// Whether the shared session's device can run a mesh stage.
///
/// A guest's geometry is a mesh shader, and a device without the stage can run a frame's pixel
/// shaders and not its geometry - which is a refusal to name (D010), not a crash at draw time.
fn mesh_stage_available() -> bool {
    matches!(
        probe(),
        Availability::Available { properties } if properties.mesh_shading
    )
}

/// A shader made resident: the module the driver validated, and its words.
///
/// The words are kept so a compute dispatch can run the shader now, through the tested compute
/// path; when the executor assembles pipelines from resident modules directly, they become
/// redundant and go.
#[derive(Debug)]
struct ResidentShader {
    module: vk::ShaderModule,
    spirv: Vec<u32>,
}

/// A Vulkan render backend.
///
/// It holds the host objects a submission's resources become, keyed by their content id, runs a
/// compute dispatch of a resident shader and a draw of a resident vertex+fragment pipeline into a
/// target-sized attachment, and refuses the commands whose execution has not landed.
/// [`VulkanBackend::new`] creates no device - the device is acquired lazily, on the first resource
/// made resident.
#[derive(Debug, Default)]
pub struct VulkanBackend {
    refused: usize,
    /// Shaders made resident, by content id: created once on first sight, reused across frames,
    /// and destroyed when the backend drops.
    shaders: BTreeMap<ResourceId, ResidentShader>,
    /// Buffers made resident, by content id: a `vk::Buffer` holding the guest's bytes, reused
    /// across frames and destroyed when the backend drops.
    buffers: BTreeMap<ResourceId, compute::DispatchBuffer>,
    /// Colour render targets made resident, by id, to their `(width, height)`. A target holds no
    /// host object - a draw allocates its own attachment - so this carries only the dimensions a
    /// following `Draw` sizes that attachment to, and needs no cleanup on drop.
    targets: BTreeMap<ResourceId, (u32, u32)>,
    /// The colour target a `SetRenderTargets` selected, that a following `Draw` renders into.
    current_target: Option<ResourceId>,
    /// The viewport a `SetViewport` set, restricting a following `Draw` to a rectangle, or [`None`]
    /// for the whole attachment.
    current_viewport: Option<Rect>,
    /// The compute shader a `BindShader` bound, that a following `Dispatch` runs.
    bound_compute: Option<ResourceId>,
    /// The vertex shader a `BindShader` bound, that a following `Draw` runs.
    bound_vertex: Option<ResourceId>,
    /// The fragment shader a `BindShader` bound, that a following `Draw` runs.
    bound_fragment: Option<ResourceId>,
    /// The buffer a `BindBuffer` bound, that a following `Dispatch` writes into.
    bound_buffer: Option<ResourceId>,
    /// The observation buffer of the most recent dispatch, read back from the device.
    last_output: Option<Vec<u32>>,
    /// The frame the most recent `Draw` rendered, read back from the device.
    last_pixels: Option<framebuffer::Pixels>,
    /// The frame's guest-memory window, set once before its commands (D703): a geometry shader
    /// fetches its vertices from here.
    guest_memory: Vec<u32>,
    /// The window uploaded to the device, bound directly by a mesh draw instead of seeded each time
    /// (D703, worklog 645), with the content hash it was built from so a stable window uploads once.
    window_buffer: Option<compute::DispatchBuffer>,
    /// The hash of the words [`Self::window_buffer`] holds, to know when it is stale.
    window_hash: u64,
    /// How many times the window buffer has actually been uploaded - it distinguishes "uploaded once
    /// and reused" from "re-uploaded every draw", which are identical from the pixels (D703).
    window_uploads: usize,
    /// The guest-memory window read back after the most recent mesh `Draw`, or [`None`] when the
    /// last draw took the vertex path (which reads no window).
    last_window: Option<Vec<u32>>,
}

impl VulkanBackend {
    /// Creates a backend holding nothing. The Vulkan device is opened lazily.
    pub const fn new() -> Self {
        Self {
            refused: 0,
            shaders: BTreeMap::new(),
            buffers: BTreeMap::new(),
            targets: BTreeMap::new(),
            current_target: None,
            current_viewport: None,
            bound_compute: None,
            bound_vertex: None,
            bound_fragment: None,
            bound_buffer: None,
            last_output: None,
            last_pixels: None,
            guest_memory: Vec::new(),
            window_buffer: None,
            window_hash: 0,
            window_uploads: 0,
            last_window: None,
        }
    }

    /// How many commands have been refused.
    ///
    /// Useful before the backend does anything real: it distinguishes "the translator
    /// emitted nothing" from "the translator emitted plenty and none of it landed",
    /// which look identical from a black screen.
    pub const fn refused(&self) -> usize {
        self.refused
    }

    /// How many distinct shader modules the backend currently holds.
    pub fn resident_shaders(&self) -> usize {
        self.shaders.len()
    }

    /// How many distinct buffers the backend currently holds.
    pub fn resident_buffers(&self) -> usize {
        self.buffers.len()
    }

    /// How many distinct colour render targets the backend currently holds.
    pub fn resident_targets(&self) -> usize {
        self.targets.len()
    }

    /// The dimensions a `Draw` would render into now: the selected target's, or the interim square
    /// when no target has been set.
    fn render_extent(&self) -> (u32, u32) {
        self.current_target
            .and_then(|id| self.targets.get(&id))
            .copied()
            .unwrap_or((RENDER_WIDTH, RENDER_HEIGHT))
    }

    /// The observation buffer of the most recent dispatch, or [`None`] if none has run.
    pub fn last_dispatch_output(&self) -> Option<&[u32]> {
        self.last_output.as_deref()
    }

    /// The frame the most recent `Draw` rendered, or [`None`] if none has.
    pub fn last_frame(&self) -> Option<&framebuffer::Pixels> {
        self.last_pixels.as_ref()
    }

    /// The guest-memory window read back after the most recent mesh `Draw` or compute `Dispatch`, or
    /// [`None`] when the last draw took the vertex path (which reads no window).
    ///
    /// How a shader that reads or writes guest memory is observed: for a draw, the frame shows what it
    /// drew and this shows what the window held after; for a dispatch, this is the guest's result,
    /// which lives in guest memory, not in the observation window (worklog 647). Either way it is what
    /// the frame set (worklog 641) unless a shader wrote it.
    pub fn last_window(&self) -> Option<&[u32]> {
        self.last_window.as_deref()
    }

    /// The guest-memory window as a resident buffer, uploaded once and reused until its bytes change.
    ///
    /// The direct-bind D703 deferred: a stable window - the common case - is uploaded once and bound
    /// by every draw that reads it, rather than copied into a fresh buffer each draw. When the bytes
    /// change the stale buffer is destroyed and a new one uploaded, so this never grows without bound.
    fn ensure_window_buffer(&mut self) -> Result<compute::DispatchBuffer, BackendError> {
        let hash = window_hash(&self.guest_memory);
        if let Some(buffer) = self.window_buffer {
            if self.window_hash == hash {
                return Ok(buffer);
            }
        }
        let session = compute::session()
            .map_err(|e| BackendError::Device(format!("no Vulkan session: {e:?}")))?;
        let session = session
            .lock()
            .map_err(|_| BackendError::Device("Vulkan session lock poisoned".to_owned()))?;
        // The stale window, destroyed before the new one is uploaded: no draw is in flight between
        // here and the next bind, and it was created by this backend.
        if let Some(stale) = self.window_buffer.take() {
            // SAFETY: created by this backend on this device, no longer bound, destroyed once.
            unsafe { session.device.destroy_buffer(stale.buffer, None) };
            // SAFETY: the buffer above is destroyed and nothing is bound to the memory now.
            unsafe { session.device.free_memory(stale.memory, None) };
        }
        let bytes: Vec<u8> = self
            .guest_memory
            .iter()
            .flat_map(|word| word.to_le_bytes())
            .collect();
        let buffer = upload_host_buffer(&session, &bytes)?;
        self.window_buffer = Some(buffer);
        self.window_hash = hash;
        self.window_uploads += 1;
        Ok(buffer)
    }

    /// How many times the guest-memory window has been uploaded to the device - one for a window that
    /// stayed the same across draws, more only when its bytes changed (D703).
    pub const fn window_uploads(&self) -> usize {
        self.window_uploads
    }

    /// Runs the bound geometry+fragment pipeline and keeps the frame it drew.
    ///
    /// The attachment is sized from the target a `SetRenderTargets` selected - the guest's decoded
    /// dimensions (worklog 637) - or the interim square ([`RENDER_WIDTH`]) when the stream set no
    /// target.
    ///
    /// **Which stage produced the geometry is the module's to say, not the command's.** A guest
    /// binds a `Vertex` stage either way, but its geometry program translates to a *mesh* shader
    /// (D688) while a fullscreen-triangle test shader is a real vertex one - so the module's
    /// execution model chooses the path (worklog 640). A vertex draw issues the guest's decoded
    /// count (worklog 639); a mesh draw is one workgroup, because a mesh shader declares its own
    /// output and the count does not apply. A mesh module on a device with no mesh stage is refused
    /// by name (D010) rather than issued as an invalid command.
    ///
    /// **An indexed draw takes the same paths (worklog 643).** A guest's indexed geometry is a mesh
    /// shader that fetches its own indices from guest memory - the index buffer sits in the window
    /// (worklog 641), not a host binding - so a mesh geometry's indexed draw *is* its mesh draw. A
    /// vertex pipeline's indexed draw would need a host index buffer bound, which the executor does
    /// not build, so it is refused rather than drawn as a non-indexed draw pretending to be one.
    ///
    /// A draw with either shader unbound is refused rather than run half a pipeline.
    fn draw_graphics(
        &mut self,
        draw: framebuffer::VertexDraw,
        indexed: bool,
    ) -> Result<(), BackendError> {
        let (Some(vertex_id), Some(fragment_id)) = (self.bound_vertex, self.bound_fragment) else {
            self.refused += 1;
            return Err(BackendError::Unsupported {
                command: "Draw with no vertex and fragment shaders bound",
            });
        };
        let geometry = self
            .shaders
            .get(&vertex_id)
            .ok_or(BackendError::UnknownResource(vertex_id))?
            .spirv
            .clone();
        let fragment = self
            .shaders
            .get(&fragment_id)
            .ok_or(BackendError::UnknownResource(fragment_id))?
            .spirv
            .clone();
        let (width, height) = self.render_extent();
        // The viewport a `SetViewport` set restricts the draw to a rectangle (worklog 644); none is
        // the whole attachment.
        let scissor = self.current_viewport.map(scissor_of);
        if is_mesh_module(&geometry) {
            if !mesh_stage_available() {
                self.refused += 1;
                return Err(BackendError::Unsupported {
                    command: "Draw of a mesh geometry shader on a device with no mesh stage",
                });
            }
            // The guest-memory window the frame set, uploaded once and bound directly (worklog 645),
            // so a guest's primitive shader fetches its vertices from it (worklog 641); read back
            // after, in case a shader wrote it.
            let window = self.ensure_window_buffer()?;
            let (pixels, read_back) = framebuffer::draw_mesh_into(
                &geometry,
                &fragment,
                CLEAR_COLOUR,
                width,
                height,
                &window,
                scissor,
            )
            .map_err(|e| BackendError::Device(format!("draw (mesh): {e:?}")))?;
            self.last_pixels = Some(pixels);
            self.last_window = Some(read_back);
        } else if indexed {
            // A vertex pipeline's indexed draw fetches from a host index buffer the executor does not
            // bind. This is only reached by a vertex module - a test shader - because a guest's
            // indexed geometry is a mesh shader that read its own indices above; refusing is the
            // honest answer rather than a non-indexed draw standing in for one (D010).
            self.refused += 1;
            return Err(BackendError::Unsupported {
                command: "DrawIndexed on a vertex pipeline (no index buffer binding)",
            });
        } else {
            let pixels = framebuffer::draw_vertices(
                CLEAR_COLOUR,
                (width, height),
                draw,
                scissor,
                &geometry,
                &fragment,
            )
            .map_err(|e| BackendError::Device(format!("draw: {e:?}")))?;
            self.last_pixels = Some(pixels);
            self.last_window = None;
        }
        Ok(())
    }

    /// Runs the bound compute shader on the device and keeps both what it wrote to the observation
    /// window (binding 0) and to guest memory (binding 1).
    ///
    /// The guest-memory window is bound at binding 1 and read back (worklog 647): **a guest's dispatch
    /// leaves its result in guest memory, not in the observation window a translated shader reports
    /// registers through** (worklog 635). The observation is a bound resident buffer when a
    /// `BindBuffer` named one, or a throwaway otherwise. The writeback is `last_window`, the
    /// observation `last_output`.
    fn dispatch_compute(&mut self, groups: [u32; 3]) -> Result<(), BackendError> {
        let Some(shader_id) = self.bound_compute else {
            self.refused += 1;
            return Err(BackendError::Unsupported {
                command: "Dispatch with no compute shader bound",
            });
        };
        let spirv = self
            .shaders
            .get(&shader_id)
            .ok_or(BackendError::UnknownResource(shader_id))?
            .spirv
            .clone();
        let window = self.ensure_window_buffer()?;
        let (observed, guest_memory) = match self.bound_buffer {
            Some(buffer_id) => {
                let observation = *self
                    .buffers
                    .get(&buffer_id)
                    .ok_or(BackendError::UnknownResource(buffer_id))?;
                compute::dispatch_bound(&observation, &window, &spirv, groups)
                    .map_err(|e| BackendError::Device(format!("dispatch: {e:?}")))?
            }
            None => compute::dispatch_reading_window(&window, DISPATCH_WORDS, &spirv, groups)
                .map_err(|e| BackendError::Device(format!("dispatch: {e:?}")))?,
        };
        self.last_output = Some(observed);
        self.last_window = Some(guest_memory);
        Ok(())
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

    fn ensure_resident(
        &mut self,
        id: ResourceId,
        resource: Resource<'_>,
    ) -> Result<(), BackendError> {
        match resource {
            Resource::Shader(spirv) => {
                // Idempotent: a module already resident under this content id is the same module,
                // so it is reused rather than created again ("upload once").
                if self.shaders.contains_key(&id) {
                    return Ok(());
                }
                let session = compute::session()
                    .map_err(|e| BackendError::Device(format!("no Vulkan session: {e:?}")))?;
                let session = session
                    .lock()
                    .map_err(|_| BackendError::Device("Vulkan session lock poisoned".to_owned()))?;
                let info = vk::ShaderModuleCreateInfo::default().code(spirv);
                // SAFETY: `info` is fully initialised and borrows `spirv` for the length of the
                // call; the device is live for the length of the locked session.
                let module = unsafe { session.device.create_shader_module(&info, None) }
                    .map_err(|e| BackendError::Device(format!("create_shader_module: {e:?}")))?;
                self.shaders.insert(
                    id,
                    ResidentShader {
                        module,
                        spirv: spirv.to_vec(),
                    },
                );
                Ok(())
            }
            Resource::Buffer(bytes) => {
                if self.buffers.contains_key(&id) {
                    return Ok(());
                }
                let session = compute::session()
                    .map_err(|e| BackendError::Device(format!("no Vulkan session: {e:?}")))?;
                let session = session
                    .lock()
                    .map_err(|_| BackendError::Device("Vulkan session lock poisoned".to_owned()))?;
                let resident = upload_host_buffer(&session, bytes)?;
                self.buffers.insert(id, resident);
                Ok(())
            }
            Resource::RenderTarget { width, height } => {
                // A target holds no host object - a draw allocates its own attachment - so making it
                // resident is recording its size. Idempotent by id, like the others.
                self.targets.insert(id, (width, height));
                Ok(())
            }
        }
    }

    fn execute(&mut self, command: &RenderCommand) -> Result<(), BackendError> {
        match command {
            // The colour target a following draw renders into. Its first colour attachment sizes the
            // draw; a target named but not resident is a real error, and an empty set clears the
            // selection back to the interim square. The depth target is not modelled yet, and the
            // frontend emits none - a depth attachment is a later arm (D010).
            RenderCommand::SetRenderTargets { colour, depth: _ } => match colour.first() {
                Some(id) if self.targets.contains_key(id) => {
                    self.current_target = Some(*id);
                    Ok(())
                }
                Some(id) => Err(BackendError::UnknownResource(*id)),
                None => {
                    self.current_target = None;
                    Ok(())
                }
            },
            // The rectangle a following draw is restricted to. State, like the target: set now,
            // applied at the draw (worklog 644). Its own rectangle is not validated here - a scissor
            // outside the attachment is clamped where it is used, not refused.
            RenderCommand::SetViewport(rect) => {
                self.current_viewport = Some(*rect);
                Ok(())
            }
            // A shader is bound to its stage now and run by the following dispatch or draw.
            // Binding an unknown shader is a real error, not a gap.
            RenderCommand::BindShader { stage, shader } => {
                if !self.shaders.contains_key(shader) {
                    return Err(BackendError::UnknownResource(*shader));
                }
                match stage {
                    ShaderStage::Compute => self.bound_compute = Some(*shader),
                    ShaderStage::Vertex => self.bound_vertex = Some(*shader),
                    ShaderStage::Fragment => self.bound_fragment = Some(*shader),
                }
                Ok(())
            }
            // A buffer is bound as the dispatch's output; its offset and length are not yet read -
            // the whole buffer is bound - which the fixed 2-storage-buffer convention allows.
            RenderCommand::BindBuffer { buffer, .. } => {
                if self.buffers.contains_key(buffer) {
                    self.bound_buffer = Some(*buffer);
                    Ok(())
                } else {
                    Err(BackendError::UnknownResource(*buffer))
                }
            }
            RenderCommand::Dispatch { x, y, z } => self.dispatch_compute([*x, *y, *z]),
            RenderCommand::Draw {
                vertices,
                instances,
                first_vertex,
            } => self.draw_graphics(
                framebuffer::VertexDraw {
                    vertices: *vertices,
                    instances: *instances,
                    first_vertex: *first_vertex,
                },
                false,
            ),
            // A guest's indexed geometry is a mesh shader reading its own indices from the window, so
            // this takes the mesh path; a vertex pipeline's indexed draw is refused for want of an
            // index buffer binding (worklog 643). The count rides in `vertices` for symmetry, unused
            // by the mesh path.
            RenderCommand::DrawIndexed {
                indices,
                instances,
                first_index,
            } => self.draw_graphics(
                framebuffer::VertexDraw {
                    vertices: *indices,
                    instances: *instances,
                    first_vertex: *first_index,
                },
                true,
            ),
            // The rest - a `ClearColour` (awaiting its register oracle, D702) and a `Fence` - are
            // refused by name, honestly (D010), until their execution lands.
            other => {
                self.refused += 1;
                Err(BackendError::Unsupported {
                    command: command_name(other),
                })
            }
        }
    }

    fn present(&mut self) -> Result<(), BackendError> {
        Err(BackendError::Unsupported { command: "present" })
    }

    fn set_guest_memory(&mut self, memory: &[u32]) {
        self.guest_memory = memory.to_vec();
    }
}

impl Drop for VulkanBackend {
    /// Destroys the shader modules the backend created. Best-effort: if the session cannot be
    /// reacquired the process is tearing down anyway and the driver reclaims them.
    fn drop(&mut self) {
        if self.shaders.is_empty() && self.buffers.is_empty() && self.window_buffer.is_none() {
            return;
        }
        let Ok(session) = compute::session() else {
            return;
        };
        let Ok(session) = session.lock() else {
            return;
        };
        for shader in self.shaders.values() {
            // SAFETY: each module was created by this backend on this device and nothing else
            // holds it; destroying it once here is the only release.
            unsafe { session.device.destroy_shader_module(shader.module, None) };
        }
        for buffer in self.buffers.values() {
            // SAFETY: the buffer and memory were created by this backend on this device, are no
            // longer in use, and are destroyed exactly once.
            unsafe { session.device.destroy_buffer(buffer.buffer, None) };
            // SAFETY: the buffer above is destroyed and nothing is bound to the memory now.
            unsafe { session.device.free_memory(buffer.memory, None) };
        }
        if let Some(window) = self.window_buffer {
            // SAFETY: the guest-memory window was created by this backend on this device, is no
            // longer in use, and is destroyed exactly once.
            unsafe { session.device.destroy_buffer(window.buffer, None) };
            // SAFETY: the buffer above is destroyed and nothing is bound to the memory now.
            unsafe { session.device.free_memory(window.memory, None) };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::VulkanBackend;
    use orbistoun_gpu::{BackendError, Rect, RenderBackend, RenderCommand};

    #[test]
    fn an_unimplemented_command_is_refused_by_name_not_silently_dropped() {
        // The failure this guards against: a backend that returns Ok and draws
        // nothing is indistinguishable from a rendering bug. A command whose execution
        // has not landed must be refused *by name*, so a report says which capability is
        // missing. `ClearColour` is one of those still-unimplemented commands - it was
        // `SetViewport` until that one learned to restrict a draw to a rectangle (worklog 644),
        // and `SetRenderTargets` before that. Its own register oracle is not in hand (D702).
        let mut b = VulkanBackend::new();
        let err = b
            .execute(&RenderCommand::ClearColour {
                target: orbistoun_gpu::ResourceId(0),
                value: [0.0, 0.0, 0.0, 1.0],
            })
            .expect_err("clears are not implemented yet");
        assert_eq!(
            err,
            BackendError::Unsupported {
                command: "ClearColour"
            }
        );
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

    /// **A translated shader is made resident as a real module, and a resident one is reused.**
    ///
    /// This is the first arm of the resource-residency mechanism (D701): the backend turns the
    /// translator's SPIR-V into a `vk::ShaderModule` the driver accepted - a stronger signal than
    /// "the words start with the SPIR-V magic" - and, keyed by content id, creates it once. Skips
    /// where no Vulkan device is present, exactly as the compute tests do; on a machine with one
    /// it is a real check that a translated module loads.
    #[test]
    fn a_translated_shader_is_made_resident_once_and_reused() {
        use orbistoun_gpu::{Resource, ResourceId};

        if !super::probe().is_available() {
            return;
        }
        let spirv = orbistoun_spirv::minimal_compute_module([1, 1, 1]);
        let mut backend = VulkanBackend::new();

        backend
            .ensure_resident(ResourceId(1), Resource::Shader(&spirv))
            .expect("a valid module is accepted by the driver");
        assert_eq!(backend.resident_shaders(), 1);

        // The same content id is already resident, so it is reused rather than created again.
        backend
            .ensure_resident(ResourceId(1), Resource::Shader(&spirv))
            .expect("a resident module is reused");
        assert_eq!(
            backend.resident_shaders(),
            1,
            "the second ensure of one id is a cache hit, not a second module"
        );

        // A different content id is a different module.
        backend
            .ensure_resident(ResourceId(2), Resource::Shader(&spirv))
            .expect("a second id is a second module");
        assert_eq!(backend.resident_shaders(), 2);
    }

    /// **A bound compute shader runs on dispatch, and its output reads back through the executor.**
    ///
    /// The first genuinely-executed command: a translated compute shader is made resident, bound,
    /// and dispatched, and the constant it writes comes back - the same chain the dispatch harness
    /// proves, now driven through `RenderBackend::execute` (D701). The output buffer is the interim
    /// fixed width until buffer resources carry their own. Skips where there is no device.
    #[test]
    fn a_bound_compute_shader_runs_on_dispatch_and_reads_back() {
        use orbistoun_gpu::{Resource, ResourceId, ShaderStage};

        const VALUE: u32 = 0xABCD_1234;

        if !super::probe().is_available() {
            return;
        }
        let spirv =
            orbistoun_spirv::storage_buffer_write_module(VALUE, super::DISPATCH_WORDS as u32);
        let mut backend = VulkanBackend::new();

        backend
            .ensure_resident(ResourceId(7), Resource::Shader(&spirv))
            .expect("resident");
        backend
            .execute(&RenderCommand::BindShader {
                stage: ShaderStage::Compute,
                shader: ResourceId(7),
            })
            .expect("bind the compute shader");
        backend
            .execute(&RenderCommand::Dispatch { x: 1, y: 1, z: 1 })
            .expect("the dispatch runs");

        let output = backend
            .last_dispatch_output()
            .expect("an output was read back");
        assert_eq!(
            output[0], VALUE,
            "the translated shader wrote its constant through the executor: {output:#x?}"
        );
    }

    /// **A dispatch writes into a bound *resident* buffer, not a throwaway one.**
    ///
    /// The guest's own resource: a buffer is made resident, bound, and a compute shader writes its
    /// constant into it - `dispatch_into` binds the resident buffer at binding 0 rather than the
    /// fixed-width scratch the no-buffer path uses. Reading the buffer back proves the shader wrote
    /// the guest's buffer, which is the whole point of the buffer arm. Skips where no device.
    #[test]
    fn a_dispatch_writes_into_a_bound_resident_buffer() {
        use orbistoun_gpu::{Resource, ResourceId, ShaderStage};

        const VALUE: u32 = 0x0BAD_F00D;
        const WORDS: u32 = 4;

        if !super::probe().is_available() {
            return;
        }
        let zeros = [0_u8; (WORDS * 4) as usize];
        let spirv = orbistoun_spirv::storage_buffer_write_module(VALUE, WORDS);
        let mut backend = VulkanBackend::new();

        backend
            .ensure_resident(ResourceId(10), Resource::Buffer(&zeros))
            .expect("buffer resident");
        backend
            .ensure_resident(ResourceId(11), Resource::Shader(&spirv))
            .expect("shader resident");
        assert_eq!(backend.resident_buffers(), 1);

        backend
            .execute(&RenderCommand::BindBuffer {
                stage: ShaderStage::Compute,
                slot: 0,
                buffer: ResourceId(10),
                offset: 0,
                length: u64::from(WORDS) * 4,
            })
            .expect("bind the output buffer");
        backend
            .execute(&RenderCommand::BindShader {
                stage: ShaderStage::Compute,
                shader: ResourceId(11),
            })
            .expect("bind the shader");
        backend
            .execute(&RenderCommand::Dispatch { x: 1, y: 1, z: 1 })
            .expect("the dispatch runs");

        let output = backend.last_dispatch_output().expect("an output");
        assert_eq!(
            output[0], VALUE,
            "the shader wrote its constant into the resident buffer: {output:#x?}"
        );
    }

    /// **A compute dispatch reads back the guest-memory window - where a guest's result lives.**
    ///
    /// The gap worklog 635 named and 647 closes: a guest compute shader leaves its result in guest
    /// memory (binding 1), not in the observation window (binding 0) a translated shader reports
    /// registers through. Seeded with known words and dispatched with a shader that ignores the
    /// window, the window comes back the seed - proof it was bound at binding 1 and read back, not the
    /// zeros the observation-only path returned. Skips where there is no device.
    #[test]
    fn a_dispatch_reads_back_the_guest_memory_window() {
        use orbistoun_gpu::{Resource, ResourceId, ShaderStage};

        if !super::probe().is_available() {
            return;
        }
        let seed = [0xFEED_0001u32, 0xFEED_0002, 0xFEED_0003];
        let spirv = orbistoun_spirv::minimal_compute_module([1, 1, 1]);
        let mut backend = VulkanBackend::new();

        backend
            .ensure_resident(ResourceId(100), Resource::Shader(&spirv))
            .expect("shader resident");
        backend.set_guest_memory(&seed);
        backend
            .execute(&RenderCommand::BindShader {
                stage: ShaderStage::Compute,
                shader: ResourceId(100),
            })
            .expect("bind the compute shader");
        backend
            .execute(&RenderCommand::Dispatch { x: 1, y: 1, z: 1 })
            .expect("the dispatch runs");

        assert_eq!(
            backend.last_window(),
            Some(seed.as_slice()),
            "a dispatch reads back guest memory, where a guest's result lives, not the observation"
        );
    }

    /// A dispatch with nothing bound is refused, not run against a stale or empty shader.
    #[test]
    fn a_dispatch_with_no_bound_shader_is_refused() {
        let mut backend = VulkanBackend::new();
        let err = backend
            .execute(&RenderCommand::Dispatch { x: 1, y: 1, z: 1 })
            .expect_err("nothing is bound");
        assert!(matches!(err, BackendError::Unsupported { .. }));
    }

    /// **A bound vertex+fragment pipeline draws a coloured frame through the executor.**
    ///
    /// The first graphics execution: a fullscreen-triangle vertex shader and a constant-colour
    /// fragment shader are made resident, bound, and a `Draw` renders them into the interim
    /// attachment - reading the frame back gives the fragment's colour, the graphics counterpart of
    /// the compute dispatch. Skips where there is no device.
    #[test]
    fn a_bound_vertex_and_fragment_pipeline_draws_a_frame() {
        use orbistoun_gpu::{Resource, ResourceId, ShaderStage};

        if !super::probe().is_available() {
            return;
        }
        let green = [0.0, 1.0, 0.0, 1.0];
        let vertex = orbistoun_spirv::fullscreen_triangle_vertex_module();
        let fragment = orbistoun_spirv::constant_colour_fragment_module(green);
        let mut backend = VulkanBackend::new();

        backend
            .ensure_resident(ResourceId(20), Resource::Shader(&vertex))
            .expect("vertex resident");
        backend
            .ensure_resident(ResourceId(21), Resource::Shader(&fragment))
            .expect("fragment resident");
        backend
            .execute(&RenderCommand::BindShader {
                stage: ShaderStage::Vertex,
                shader: ResourceId(20),
            })
            .expect("bind vertex");
        backend
            .execute(&RenderCommand::BindShader {
                stage: ShaderStage::Fragment,
                shader: ResourceId(21),
            })
            .expect("bind fragment");
        backend
            .execute(&RenderCommand::Draw {
                vertices: 3,
                instances: 1,
                first_vertex: 0,
            })
            .expect("the draw runs");

        let frame = backend.last_frame().expect("a frame was rendered");
        assert_eq!(
            frame.at(super::RENDER_WIDTH / 2, super::RENDER_HEIGHT / 2),
            Some([0, 255, 0, 255]),
            "the fullscreen triangle painted the frame the fragment's green"
        );
    }

    /// **The geometry stage is routed by the module's execution model, not the command's stage.**
    ///
    /// A guest binds a `Vertex` stage whether its geometry is a real vertex shader or - as every
    /// guest's is - an NGG primitive shader translated to a mesh shader (D688). The module itself
    /// says which, through its execution model, and getting that wrong draws a guest's geometry
    /// through the wrong call. Checked against real modules with no device, so the reader is pinned
    /// even where a mesh stage cannot run.
    #[test]
    fn the_geometry_stage_is_routed_by_the_module_not_the_command() {
        let green = [0.0, 1.0, 0.0, 1.0];
        assert!(
            super::is_mesh_module(&orbistoun_spirv::triangle_mesh_module([green; 3])),
            "a mesh module's execution model is MeshEXT"
        );
        assert!(
            !super::is_mesh_module(&orbistoun_spirv::fullscreen_triangle_vertex_module()),
            "a vertex module is not a mesh one"
        );
        assert!(
            !super::is_mesh_module(&orbistoun_spirv::constant_colour_fragment_module(green)),
            "a fragment module is not a mesh one"
        );
        // Not SPIR-V, and empty: not a mesh module, and no panic reading past the end.
        assert!(!super::is_mesh_module(&[0, 1, 2]));
        assert!(!super::is_mesh_module(&[]));
    }

    /// **A bound mesh geometry shader is drawn through the mesh path.**
    ///
    /// The executor routes a guest's geometry - a mesh shader - to the mesh draw, reading that from
    /// the module rather than the command. `triangle_mesh_module` emits a fullscreen triangle in the
    /// colour it is given and the passthrough fragment shows it, so the frame comes back that colour.
    /// The made-to-fail is structural: a mesh module drawn through the *vertex* path fails pipeline
    /// creation (its execution model is not vertex), so a green frame is itself the proof the routing
    /// worked. Skips where the device has no mesh stage.
    #[test]
    fn a_bound_mesh_geometry_shader_draws_through_the_mesh_path() {
        use orbistoun_gpu::{Resource, ResourceId, ShaderStage};

        if !super::mesh_stage_available() {
            return;
        }
        let green = [0.0, 1.0, 0.0, 1.0];
        let mesh = orbistoun_spirv::triangle_mesh_module([green; 3]);
        let fragment = orbistoun_spirv::passthrough_fragment_module();
        let mut backend = VulkanBackend::new();

        backend
            .ensure_resident(ResourceId(50), Resource::Shader(&mesh))
            .expect("mesh resident");
        backend
            .ensure_resident(ResourceId(51), Resource::Shader(&fragment))
            .expect("fragment resident");
        backend
            .execute(&RenderCommand::BindShader {
                stage: ShaderStage::Vertex,
                shader: ResourceId(50),
            })
            .expect("bind the geometry shader");
        backend
            .execute(&RenderCommand::BindShader {
                stage: ShaderStage::Fragment,
                shader: ResourceId(51),
            })
            .expect("bind fragment");
        backend
            .execute(&RenderCommand::Draw {
                vertices: 3,
                instances: 1,
                first_vertex: 0,
            })
            .expect("the mesh draw runs");

        let frame = backend.last_frame().expect("a frame");
        assert_eq!(
            frame.at(super::RENDER_WIDTH / 2, super::RENDER_HEIGHT / 2),
            Some([0, 255, 0, 255]),
            "the mesh shader emitted a triangle, shaded green - the module routed to the mesh path"
        );
    }

    /// **A mesh draw's guest-memory window is fed from the frame and read back through the executor.**
    ///
    /// A guest's geometry fetches its vertices from the window; the executor seeds it from the
    /// frame's guest memory (worklog 641) and reads it back. Seeded with known words and drawn with a
    /// mesh that ignores the window, the window comes back the seed - proof it was bound and carried
    /// the guest's bytes, not the zeros an unfed window would. Skips where the device has no mesh
    /// stage.
    #[test]
    fn a_mesh_draw_binds_and_reads_back_the_guest_memory_window() {
        use orbistoun_gpu::{Resource, ResourceId, ShaderStage};

        if !super::mesh_stage_available() {
            return;
        }
        let seed = [0xABCD_0001u32, 0xABCD_0002, 0xABCD_0003, 0xABCD_0004];
        let green = [0.0, 1.0, 0.0, 1.0];
        let mesh = orbistoun_spirv::triangle_mesh_module([green; 3]);
        let fragment = orbistoun_spirv::passthrough_fragment_module();
        let mut backend = VulkanBackend::new();

        backend
            .ensure_resident(ResourceId(60), Resource::Shader(&mesh))
            .expect("mesh resident");
        backend
            .ensure_resident(ResourceId(61), Resource::Shader(&fragment))
            .expect("fragment resident");
        backend.set_guest_memory(&seed);
        backend
            .execute(&RenderCommand::BindShader {
                stage: ShaderStage::Vertex,
                shader: ResourceId(60),
            })
            .expect("bind the geometry shader");
        backend
            .execute(&RenderCommand::BindShader {
                stage: ShaderStage::Fragment,
                shader: ResourceId(61),
            })
            .expect("bind fragment");
        backend
            .execute(&RenderCommand::Draw {
                vertices: 3,
                instances: 1,
                first_vertex: 0,
            })
            .expect("the mesh draw runs");

        assert_eq!(
            backend.last_window(),
            Some(seed.as_slice()),
            "the window was seeded from the frame's guest memory and read back unchanged"
        );
    }

    /// **The window is uploaded once, bound directly, and survives the draw.**
    ///
    /// The direct-bind D703 deferred (worklog 645): the window is uploaded to a resident buffer and
    /// bound by each draw rather than copied fresh. Two draws over the same window both read it back
    /// correctly - which proves the draw did not destroy the resident buffer, since a release that
    /// freed it would fault the second draw - and the upload count stays one, proving it was reused
    /// rather than re-uploaded. Skips where there is no mesh stage.
    #[test]
    fn the_window_is_uploaded_once_and_survives_the_draw() {
        use orbistoun_gpu::{Resource, ResourceId, ShaderStage};

        if !super::mesh_stage_available() {
            return;
        }
        let seed = [0x1234_0001u32, 0x1234_0002, 0x1234_0003, 0x1234_0004];
        let green = [0.0, 1.0, 0.0, 1.0];
        let mesh = orbistoun_spirv::triangle_mesh_module([green; 3]);
        let fragment = orbistoun_spirv::passthrough_fragment_module();
        let mut backend = VulkanBackend::new();

        backend
            .ensure_resident(ResourceId(90), Resource::Shader(&mesh))
            .expect("mesh resident");
        backend
            .ensure_resident(ResourceId(91), Resource::Shader(&fragment))
            .expect("fragment resident");
        backend.set_guest_memory(&seed);
        backend
            .execute(&RenderCommand::BindShader {
                stage: ShaderStage::Vertex,
                shader: ResourceId(90),
            })
            .expect("bind the geometry shader");
        backend
            .execute(&RenderCommand::BindShader {
                stage: ShaderStage::Fragment,
                shader: ResourceId(91),
            })
            .expect("bind fragment");

        let draw = RenderCommand::Draw {
            vertices: 3,
            instances: 1,
            first_vertex: 0,
        };
        backend
            .execute(&draw)
            .expect("first draw uploads and binds");
        assert_eq!(backend.last_window(), Some(seed.as_slice()));

        // The second draw reuses the resident buffer the first left intact - a release that freed it
        // would fault here rather than read back.
        backend
            .execute(&draw)
            .expect("second draw reuses the surviving window");
        assert_eq!(backend.last_window(), Some(seed.as_slice()));
        assert_eq!(
            backend.window_uploads(),
            1,
            "an unchanged window is uploaded once, not per draw"
        );
    }

    /// **A window whose bytes change is re-uploaded; an unchanged one is not.**
    ///
    /// The other half of "upload once" (D703): the resident buffer is content-keyed, so setting the
    /// same window again binds the same buffer, and setting different bytes uploads a fresh one. Made
    /// to fail against an upload that never refreshed (a stale window) or one that refreshed every
    /// draw (no caching). Skips where there is no mesh stage.
    #[test]
    fn a_changed_window_is_re_uploaded_and_an_unchanged_one_is_not() {
        use orbistoun_gpu::{Resource, ResourceId, ShaderStage};

        if !super::mesh_stage_available() {
            return;
        }
        let green = [0.0, 1.0, 0.0, 1.0];
        let mesh = orbistoun_spirv::triangle_mesh_module([green; 3]);
        let fragment = orbistoun_spirv::passthrough_fragment_module();
        let mut backend = VulkanBackend::new();

        backend
            .ensure_resident(ResourceId(92), Resource::Shader(&mesh))
            .expect("mesh resident");
        backend
            .ensure_resident(ResourceId(93), Resource::Shader(&fragment))
            .expect("fragment resident");
        backend
            .execute(&RenderCommand::BindShader {
                stage: ShaderStage::Vertex,
                shader: ResourceId(92),
            })
            .expect("bind the geometry shader");
        backend
            .execute(&RenderCommand::BindShader {
                stage: ShaderStage::Fragment,
                shader: ResourceId(93),
            })
            .expect("bind fragment");
        let draw = RenderCommand::Draw {
            vertices: 3,
            instances: 1,
            first_vertex: 0,
        };

        backend.set_guest_memory(&[0xAAAA_0001, 0xAAAA_0002]);
        backend.execute(&draw).expect("first window");
        assert_eq!(backend.window_uploads(), 1);

        // The same bytes again: no re-upload.
        backend.set_guest_memory(&[0xAAAA_0001, 0xAAAA_0002]);
        backend.execute(&draw).expect("same window again");
        assert_eq!(
            backend.window_uploads(),
            1,
            "an unchanged window is not re-uploaded"
        );

        // Different bytes: a fresh upload, and the new window reads back.
        backend.set_guest_memory(&[0xBBBB_0001, 0xBBBB_0002]);
        backend.execute(&draw).expect("a changed window");
        assert_eq!(
            backend.window_uploads(),
            2,
            "a changed window is re-uploaded"
        );
        assert_eq!(
            backend.last_window(),
            Some([0xBBBB_0001, 0xBBBB_0002].as_slice())
        );
    }

    /// A draw with a shader stage unbound is refused, not run as half a pipeline.
    #[test]
    fn a_draw_with_no_shaders_bound_is_refused() {
        let mut backend = VulkanBackend::new();
        let err = backend
            .execute(&RenderCommand::Draw {
                vertices: 3,
                instances: 1,
                first_vertex: 0,
            })
            .expect_err("nothing is bound");
        assert!(matches!(err, BackendError::Unsupported { .. }));
    }

    /// **A whole frame composes through the driver: target, viewport, shaders, draw.**
    ///
    /// Everything the executor grew this session, driven as one submission rather than command by
    /// command: the driver makes the colour target and the two modules resident, a `SetRenderTargets`
    /// sizes the attachment (worklog 637), a `SetViewport` clips it (644, 646), and a bound
    /// vertex+fragment pipeline draws (636). The frame comes back the target's size, the fragment's
    /// green inside the viewport and the clear outside - proof the pieces compose, not just that each
    /// runs alone. Skips where there is no device.
    #[test]
    fn a_full_frame_composes_through_the_driver() {
        use orbistoun_gpu::pipeline::Submission;
        use orbistoun_gpu::{ColourTargetExtent, Rect, ResourceId, ShaderStage, drive};
        use std::collections::BTreeMap;

        if !super::probe().is_available() {
            return;
        }
        let (width, height) = (128u32, 96u32);
        let target =
            ResourceId(0x8000_0000_0000_0000 | (u64::from(width) << 16) | u64::from(height));
        let (vertex_id, fragment_id) = (ResourceId(1), ResourceId(2));
        let vertex = orbistoun_spirv::fullscreen_triangle_vertex_module();
        let fragment = orbistoun_spirv::constant_colour_fragment_module([0.0, 1.0, 0.0, 1.0]);

        let submission = Submission {
            commands: vec![
                RenderCommand::SetRenderTargets {
                    colour: vec![target],
                    depth: None,
                },
                RenderCommand::SetViewport(Rect {
                    x: 0,
                    y: 0,
                    width: width / 2,
                    height,
                }),
                RenderCommand::BindShader {
                    stage: ShaderStage::Vertex,
                    shader: vertex_id,
                },
                RenderCommand::BindShader {
                    stage: ShaderStage::Fragment,
                    shader: fragment_id,
                },
                RenderCommand::Draw {
                    vertices: 3,
                    instances: 1,
                    first_vertex: 0,
                },
            ],
            modules: BTreeMap::from([(vertex_id, vertex), (fragment_id, fragment)]),
            targets: BTreeMap::from([(target, ColourTargetExtent { width, height })]),
            ..Submission::default()
        };

        let mut backend = VulkanBackend::new();
        let outcome = drive(&mut backend, &submission).expect("the frame drives");
        assert_eq!(outcome.resident, 3, "two modules and one colour target");
        assert_eq!(outcome.executed, 5, "all five commands ran");
        assert_eq!(outcome.refused, 0);

        let frame = backend.last_frame().expect("a frame was rendered");
        assert_eq!(
            (frame.width, frame.height),
            (width, height),
            "the colour target sized the attachment"
        );
        assert_eq!(
            frame.at(width / 4, height / 2),
            Some([0, 255, 0, 255]),
            "inside the viewport is the fragment's green"
        );
        assert_eq!(
            frame.at(width * 3 / 4, height / 2),
            Some([0, 0, 0, 255]),
            "outside the viewport is the clear"
        );
    }

    /// **An indexed draw routes to the graphics path, not the generic refusal.**
    ///
    /// A `DrawIndexed` used to be refused by name like any unimplemented command; now it reaches
    /// `draw_graphics` (worklog 643). With nothing bound, the refusal it gets is the *graphics* one -
    /// "no vertex and fragment shaders bound" - not the generic "DrawIndexed", which is what proves it
    /// routed there. No device needed: the bound-shader check runs before the device is touched.
    #[test]
    fn an_indexed_draw_routes_to_the_graphics_path() {
        let mut backend = VulkanBackend::new();
        let err = backend
            .execute(&RenderCommand::DrawIndexed {
                indices: 36,
                instances: 1,
                first_index: 0,
            })
            .expect_err("nothing is bound");
        assert_eq!(
            err,
            BackendError::Unsupported {
                command: "Draw with no vertex and fragment shaders bound",
            },
            "an indexed draw reaches the graphics path, so its refusal is the graphics one"
        );
    }

    /// **An indexed draw of mesh geometry executes through the mesh path.**
    ///
    /// A guest's indexed geometry is a mesh shader that reads its own indices from the window
    /// (worklog 641); its `DrawIndexed` is therefore its mesh draw, and it renders rather than being
    /// refused. `triangle_mesh_module` drew through a `DrawIndexed` comes back the fragment's green -
    /// the same proof-by-structure as the mesh `Draw` (a mesh module on the vertex path fails pipeline
    /// creation). Skips where the device has no mesh stage.
    #[test]
    fn an_indexed_draw_of_mesh_geometry_executes() {
        use orbistoun_gpu::{Resource, ResourceId, ShaderStage};

        if !super::mesh_stage_available() {
            return;
        }
        let green = [0.0, 1.0, 0.0, 1.0];
        let mesh = orbistoun_spirv::triangle_mesh_module([green; 3]);
        let fragment = orbistoun_spirv::passthrough_fragment_module();
        let mut backend = VulkanBackend::new();

        backend
            .ensure_resident(ResourceId(70), Resource::Shader(&mesh))
            .expect("mesh resident");
        backend
            .ensure_resident(ResourceId(71), Resource::Shader(&fragment))
            .expect("fragment resident");
        backend
            .execute(&RenderCommand::BindShader {
                stage: ShaderStage::Vertex,
                shader: ResourceId(70),
            })
            .expect("bind the geometry shader");
        backend
            .execute(&RenderCommand::BindShader {
                stage: ShaderStage::Fragment,
                shader: ResourceId(71),
            })
            .expect("bind fragment");
        backend
            .execute(&RenderCommand::DrawIndexed {
                indices: 3,
                instances: 1,
                first_index: 0,
            })
            .expect("the indexed mesh draw runs");

        assert_eq!(
            backend
                .last_frame()
                .expect("a frame")
                .at(super::RENDER_WIDTH / 2, super::RENDER_HEIGHT / 2),
            Some([0, 255, 0, 255]),
            "the indexed draw ran the mesh shader through the mesh path"
        );
    }

    /// **An indexed draw on a *vertex* pipeline is refused, not drawn as a non-indexed one.**
    ///
    /// A vertex pipeline's indexed draw fetches from a host index buffer the executor does not bind,
    /// and drawing it without one would draw the wrong geometry while looking like it worked - the
    /// plausible-output failure (D010). This is only reachable by a vertex module (a test shader),
    /// since a guest's geometry is a mesh shader. Skips where there is no device to make the module
    /// resident.
    #[test]
    fn an_indexed_draw_on_a_vertex_pipeline_is_refused() {
        use orbistoun_gpu::{Resource, ResourceId, ShaderStage};

        if !super::probe().is_available() {
            return;
        }
        let vertex = orbistoun_spirv::fullscreen_triangle_vertex_module();
        let fragment = orbistoun_spirv::constant_colour_fragment_module([0.0, 1.0, 0.0, 1.0]);
        let mut backend = VulkanBackend::new();

        backend
            .ensure_resident(ResourceId(72), Resource::Shader(&vertex))
            .expect("vertex resident");
        backend
            .ensure_resident(ResourceId(73), Resource::Shader(&fragment))
            .expect("fragment resident");
        backend
            .execute(&RenderCommand::BindShader {
                stage: ShaderStage::Vertex,
                shader: ResourceId(72),
            })
            .expect("bind vertex");
        backend
            .execute(&RenderCommand::BindShader {
                stage: ShaderStage::Fragment,
                shader: ResourceId(73),
            })
            .expect("bind fragment");

        let err = backend
            .execute(&RenderCommand::DrawIndexed {
                indices: 3,
                instances: 1,
                first_index: 0,
            })
            .expect_err("a vertex pipeline has no index buffer bound");
        assert_eq!(
            err,
            BackendError::Unsupported {
                command: "DrawIndexed on a vertex pipeline (no index buffer binding)",
            }
        );
    }

    /// **A `SetViewport` is accepted as state, not refused.** It sets the rectangle a following draw
    /// is restricted to and touches no device, so it succeeds before anything is bound.
    #[test]
    fn a_set_viewport_is_accepted() {
        let mut b = VulkanBackend::new();
        b.execute(&RenderCommand::SetViewport(Rect {
            x: 0,
            y: 0,
            width: 16,
            height: 16,
        }))
        .expect("a viewport is state the backend keeps, not an unimplemented command");
    }

    /// **A `SetViewport` restricts a draw to its rectangle - pixels outside keep the clear.**
    ///
    /// The viewport is applied as the scissor (worklog 644): a fullscreen-triangle draw restricted to
    /// the left half of the attachment paints the left half the fragment's green and leaves the right
    /// half the clear black. A backend that ignored the viewport would paint the whole frame green, so
    /// the black right half is what proves the rectangle took effect. Skips where there is no device.
    #[test]
    fn a_set_viewport_restricts_the_draw_to_its_rectangle() {
        use orbistoun_gpu::{Resource, ResourceId, ShaderStage};

        if !super::probe().is_available() {
            return;
        }
        let green = [0.0, 1.0, 0.0, 1.0];
        let vertex = orbistoun_spirv::fullscreen_triangle_vertex_module();
        let fragment = orbistoun_spirv::constant_colour_fragment_module(green);
        let mut backend = VulkanBackend::new();

        backend
            .ensure_resident(ResourceId(80), Resource::Shader(&vertex))
            .expect("vertex resident");
        backend
            .ensure_resident(ResourceId(81), Resource::Shader(&fragment))
            .expect("fragment resident");

        // Restrict to the left half of the interim attachment.
        let half = super::RENDER_WIDTH / 2;
        backend
            .execute(&RenderCommand::SetViewport(Rect {
                x: 0,
                y: 0,
                width: half,
                height: super::RENDER_HEIGHT,
            }))
            .expect("set the viewport");
        backend
            .execute(&RenderCommand::BindShader {
                stage: ShaderStage::Vertex,
                shader: ResourceId(80),
            })
            .expect("bind vertex");
        backend
            .execute(&RenderCommand::BindShader {
                stage: ShaderStage::Fragment,
                shader: ResourceId(81),
            })
            .expect("bind fragment");
        backend
            .execute(&RenderCommand::Draw {
                vertices: 3,
                instances: 1,
                first_vertex: 0,
            })
            .expect("the draw runs");

        let frame = backend.last_frame().expect("a frame");
        let mid_y = super::RENDER_HEIGHT / 2;
        assert_eq!(
            frame.at(half / 2, mid_y),
            Some([0, 255, 0, 255]),
            "inside the viewport is drawn the fragment's green"
        );
        assert_eq!(
            frame.at(half + half / 2, mid_y),
            Some([0, 0, 0, 255]),
            "outside the viewport keeps the clear the draw never reached"
        );
    }

    /// **A `Draw` issues the guest's decoded vertex count, not a fixed three.**
    ///
    /// The count reaches `cmd_draw`: with the same bound fullscreen-triangle pipeline, a draw of
    /// **zero** vertices issues no geometry, so the frame stays the clear colour, while a draw of
    /// three paints it the fragment's. A backend that ignored the count and always drew three would
    /// paint both, so the black frame is what proves the count is honoured. Skips where no device.
    #[test]
    fn a_draw_issues_its_decoded_vertex_count() {
        use orbistoun_gpu::{Resource, ResourceId, ShaderStage};

        if !super::probe().is_available() {
            return;
        }
        let green = [0.0, 1.0, 0.0, 1.0];
        let vertex = orbistoun_spirv::fullscreen_triangle_vertex_module();
        let fragment = orbistoun_spirv::constant_colour_fragment_module(green);
        let mut backend = VulkanBackend::new();

        backend
            .ensure_resident(ResourceId(40), Resource::Shader(&vertex))
            .expect("vertex resident");
        backend
            .ensure_resident(ResourceId(41), Resource::Shader(&fragment))
            .expect("fragment resident");
        backend
            .execute(&RenderCommand::BindShader {
                stage: ShaderStage::Vertex,
                shader: ResourceId(40),
            })
            .expect("bind vertex");
        backend
            .execute(&RenderCommand::BindShader {
                stage: ShaderStage::Fragment,
                shader: ResourceId(41),
            })
            .expect("bind fragment");

        // Zero vertices: nothing is drawn, so the centre stays the interim clear (opaque black).
        backend
            .execute(&RenderCommand::Draw {
                vertices: 0,
                instances: 1,
                first_vertex: 0,
            })
            .expect("the draw of zero vertices runs");
        let centre = (super::RENDER_WIDTH / 2, super::RENDER_HEIGHT / 2);
        assert_eq!(
            backend
                .last_frame()
                .expect("a frame")
                .at(centre.0, centre.1),
            Some([0, 0, 0, 255]),
            "a zero-vertex draw painted nothing, so the frame is the clear colour"
        );

        // Three vertices: the fullscreen triangle covers the frame in the fragment's green.
        backend
            .execute(&RenderCommand::Draw {
                vertices: 3,
                instances: 1,
                first_vertex: 0,
            })
            .expect("the draw of three vertices runs");
        assert_eq!(
            backend
                .last_frame()
                .expect("a frame")
                .at(centre.0, centre.1),
            Some([0, 255, 0, 255]),
            "three vertices painted the frame the fragment's green - the count drove the draw"
        );
    }

    /// **A `SetRenderTargets` naming a target that is not resident is refused, not silently drawn to
    /// the wrong size.** Selecting a target the backend never received is a real error - the
    /// residency pass should have delivered it - and answering `UnknownResource` says so rather than
    /// falling back to the interim square as if nothing were named.
    #[test]
    fn an_unresident_render_target_is_refused() {
        use orbistoun_gpu::ResourceId;

        let mut backend = VulkanBackend::new();
        let err = backend
            .execute(&RenderCommand::SetRenderTargets {
                colour: vec![ResourceId(0x8000_0000_0080_0060)],
                depth: None,
            })
            .expect_err("the target was never made resident");
        assert_eq!(
            err,
            BackendError::UnknownResource(ResourceId(0x8000_0000_0080_0060))
        );
    }

    /// **An empty `SetRenderTargets` clears the selection back to the interim square.** A frame that
    /// unbinds its target is not an error; the next draw simply has no guest size to honour, so the
    /// backend falls back to the fixed attachment rather than refusing.
    #[test]
    fn an_empty_render_target_set_clears_the_selection() {
        let mut backend = VulkanBackend::new();
        backend
            .execute(&RenderCommand::SetRenderTargets {
                colour: Vec::new(),
                depth: None,
            })
            .expect("an empty set is not an error");
        assert_eq!(
            backend.render_extent(),
            (super::RENDER_WIDTH, super::RENDER_HEIGHT),
            "with no target selected a draw uses the interim square"
        );
    }

    /// **A `Draw` renders into the selected target's dimensions, not the interim square.**
    ///
    /// The guest's decoded size (worklog 637) reaches the attachment: a 128x96 colour target is made
    /// resident, selected, and a fullscreen triangle drawn - the frame comes back 128x96, which the
    /// fixed `RENDER_WIDTH` square could not produce, and its centre is the fragment's colour. This
    /// is the join between the register decode and the pixels. Skips where there is no device.
    #[test]
    fn a_draw_renders_into_the_selected_target_size() {
        use orbistoun_gpu::{Resource, ResourceId, ShaderStage};

        const WIDTH: u32 = 128;
        const HEIGHT: u32 = 96;

        if !super::probe().is_available() {
            return;
        }
        let target =
            ResourceId(0x8000_0000_0000_0000 | (u64::from(WIDTH) << 16) | u64::from(HEIGHT));

        let blue = [0.0, 0.0, 1.0, 1.0];
        let vertex = orbistoun_spirv::fullscreen_triangle_vertex_module();
        let fragment = orbistoun_spirv::constant_colour_fragment_module(blue);
        let mut backend = VulkanBackend::new();

        backend
            .ensure_resident(
                target,
                Resource::RenderTarget {
                    width: WIDTH,
                    height: HEIGHT,
                },
            )
            .expect("the target is made resident");
        assert_eq!(backend.resident_targets(), 1);
        backend
            .ensure_resident(ResourceId(30), Resource::Shader(&vertex))
            .expect("vertex resident");
        backend
            .ensure_resident(ResourceId(31), Resource::Shader(&fragment))
            .expect("fragment resident");

        backend
            .execute(&RenderCommand::SetRenderTargets {
                colour: vec![target],
                depth: None,
            })
            .expect("select the target");
        backend
            .execute(&RenderCommand::BindShader {
                stage: ShaderStage::Vertex,
                shader: ResourceId(30),
            })
            .expect("bind vertex");
        backend
            .execute(&RenderCommand::BindShader {
                stage: ShaderStage::Fragment,
                shader: ResourceId(31),
            })
            .expect("bind fragment");
        backend
            .execute(&RenderCommand::Draw {
                vertices: 3,
                instances: 1,
                first_vertex: 0,
            })
            .expect("the draw runs");

        let frame = backend.last_frame().expect("a frame was rendered");
        let (interim_width, interim_height) = (super::RENDER_WIDTH, super::RENDER_HEIGHT);
        assert_eq!(
            (frame.width, frame.height),
            (WIDTH, HEIGHT),
            "the frame took the selected target's size, not the interim {interim_width}x{interim_height} square"
        );
        assert_eq!(
            frame.at(WIDTH / 2, HEIGHT / 2),
            Some([0, 0, 255, 255]),
            "the fullscreen triangle painted the target the fragment's blue"
        );
    }
}
