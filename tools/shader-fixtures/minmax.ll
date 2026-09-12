; Float min/max. The vector ALU's saturating pair - `v_min_f32`/`v_max_f32` - which a
; shader reaches for constantly (clamps, saturates, tone maps) and which the compiler
; lowers `llvm.minnum`/`llvm.maxnum` straight onto. Their VOP2 short forms were unnamed
; until this fixture, so the translator refused them and every clamp with them.
target triple = "amdgcn-amd-amdhsa"

declare float @llvm.maxnum.f32(float, float)
declare float @llvm.minnum.f32(float, float)

define amdgpu_kernel void @minmax(ptr addrspace(1) %out, ptr addrspace(1) %in) {
entry:
  %a = load volatile float, ptr addrspace(1) %in
  %b = load volatile float, ptr addrspace(1) %in
  %hi = call float @llvm.maxnum.f32(float %a, float %b)
  %lo = call float @llvm.minnum.f32(float %hi, float %a)
  store float %lo, ptr addrspace(1) %out
  ret void
}
