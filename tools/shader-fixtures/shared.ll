; Local data share. Its own encoding family, and one no ordinary arithmetic or global
; memory access will ever produce.
target triple = "amdgcn-amd-amdhsa"

@scratch = internal addrspace(3) global [64 x float] poison

declare i32 @llvm.amdgcn.workitem.id.x()

define amdgpu_kernel void @shared(ptr addrspace(1) %out) {
entry:
  %tid = call i32 @llvm.amdgcn.workitem.id.x()
  %p = getelementptr [64 x float], ptr addrspace(3) @scratch, i32 0, i32 %tid
  store float 1.000000e+00, ptr addrspace(3) %p
  fence syncscope("workgroup") release
  %v = load float, ptr addrspace(3) %p
  %q = getelementptr float, ptr addrspace(1) %out, i32 %tid
  store float %v, ptr addrspace(1) %q
  ret void
}
