; A fragment shader. Reaches the export encoding, which every pixel shader ends in and
; which no compute kernel ever emits - so without this the family is entirely
; unverified.
; Graphics environment: the compute triple refuses non-compute shaders outright.
target triple = "amdgcn-mesa-mesa3d"

declare void @llvm.amdgcn.exp.f32(i32 immarg, i32 immarg, float, float, float, float, i1 immarg, i1 immarg)

define amdgpu_ps void @pixel(float %a, float %b) {
entry:
  %c = fadd float %a, %b
  %d = fmul float %c, %a
  call void @llvm.amdgcn.exp.f32(i32 0, i32 15, float %d, float %d, float %d, float %d, i1 true, i1 true)
  ret void
}
