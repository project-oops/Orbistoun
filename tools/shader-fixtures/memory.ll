; Vector loads and stores. Exercises the wide memory encodings, which are eight bytes
; each - so a fixture where every instruction is the same length as its neighbours
; would not catch a width mistake here.
target triple = "amdgcn-amd-amdhsa"

define amdgpu_kernel void @memory(ptr addrspace(1) %out, ptr addrspace(1) %in) {
entry:
  %v = load <4 x float>, ptr addrspace(1) %in
  %w = fmul <4 x float> %v, %v
  store <4 x float> %w, ptr addrspace(1) %out
  ret void
}
