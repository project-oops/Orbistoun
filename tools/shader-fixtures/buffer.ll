; Typed buffer access through the resource descriptor path, which is how a real shader
; reads vertex and constant data. Its own encoding family again.
target triple = "amdgcn-amd-amdhsa"

declare float @llvm.amdgcn.raw.buffer.load.f32(<4 x i32>, i32, i32, i32)
declare void @llvm.amdgcn.raw.buffer.store.f32(float, <4 x i32>, i32, i32, i32)

define amdgpu_kernel void @buffer(<4 x i32> inreg %rsrc, i32 %offset) {
entry:
  %v = call float @llvm.amdgcn.raw.buffer.load.f32(<4 x i32> %rsrc, i32 %offset, i32 0, i32 0)
  %w = fmul float %v, 2.000000e+00
  call void @llvm.amdgcn.raw.buffer.store.f32(float %w, <4 x i32> %rsrc, i32 %offset, i32 0, i32 0)
  ret void
}
