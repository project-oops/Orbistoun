; Vector comparison. Reaches the VOPC encoding, which nothing else here does - a
; comparison writes a per-lane mask rather than a value, so it has its own family.
target triple = "amdgcn-amd-amdhsa"

declare i32 @llvm.amdgcn.workitem.id.x()

define amdgpu_kernel void @compare(ptr addrspace(1) %out, ptr addrspace(1) %in) {
entry:
  %tid = call i32 @llvm.amdgcn.workitem.id.x()
  %p = getelementptr float, ptr addrspace(1) %in, i32 %tid
  %v = load float, ptr addrspace(1) %p
  %c = fcmp ogt float %v, 5.000000e-01
  %s = select i1 %c, float 1.000000e+00, float 0.000000e+00
  %q = getelementptr float, ptr addrspace(1) %out, i32 %tid
  store float %s, ptr addrspace(1) %q
  ret void
}
