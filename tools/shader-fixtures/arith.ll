; Float arithmetic. Exercises the vector ALU encodings, which are the bulk of any
; real shader by instruction count.
target triple = "amdgcn-amd-amdhsa"

define amdgpu_kernel void @arith(ptr addrspace(1) %out, float %a, float %b) {
entry:
  %m = fmul float %a, %b
  %s = fadd float %m, 2.0
  %d = fdiv float %s, 3.0
  %r = fsub float %d, %a
  store float %r, ptr addrspace(1) %out
  ret void
}
