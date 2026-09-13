; Unary float arithmetic and transcendentals: sqrt, rsq, sin, cos, exp2, log2.
; Vector ALU operations lowering directly from AMDGPU intrinsics onto their
; respective VOP1 short forms (v_sqrt_f32, v_rsq_f32, v_sin_f32, v_cos_f32,
; v_exp_f32, v_log_f32).
target triple = "amdgcn-amd-amdhsa"

declare float @llvm.amdgcn.sqrt.f32(float)
declare float @llvm.amdgcn.rsq.f32(float)
declare float @llvm.amdgcn.sin.f32(float)
declare float @llvm.amdgcn.cos.f32(float)
declare float @llvm.amdgcn.exp2.f32(float)
declare float @llvm.amdgcn.log.f32(float)

define amdgpu_kernel void @unary(ptr addrspace(1) %out, ptr addrspace(1) %in) {
entry:
  %x = load volatile float, ptr addrspace(1) %in
  %v_sqrt = call float @llvm.amdgcn.sqrt.f32(float %x)
  %v_rsq = call float @llvm.amdgcn.rsq.f32(float %v_sqrt)
  %v_sin = call float @llvm.amdgcn.sin.f32(float %v_rsq)
  %v_cos = call float @llvm.amdgcn.cos.f32(float %v_sin)
  %v_exp = call float @llvm.amdgcn.exp2.f32(float %v_cos)
  %v_log = call float @llvm.amdgcn.log.f32(float %v_exp)
  store float %v_log, ptr addrspace(1) %out
  ret void
}
