; Texture sampling. The image encoding family, reached only through the sampling path.
; Graphics environment: the compute triple refuses non-compute shaders outright.
target triple = "amdgcn-mesa-mesa3d"

declare <4 x float> @llvm.amdgcn.image.sample.2d.v4f32.f32(i32 immarg, float, float, <8 x i32>, <4 x i32>, i1 immarg, i32 immarg, i32 immarg)
declare void @llvm.amdgcn.exp.f32(i32 immarg, i32 immarg, float, float, float, float, i1 immarg, i1 immarg)

define amdgpu_ps void @image(<8 x i32> inreg %rsrc, <4 x i32> inreg %samp, float %s, float %t) {
entry:
  %v = call <4 x float> @llvm.amdgcn.image.sample.2d.v4f32.f32(i32 15, float %s, float %t, <8 x i32> %rsrc, <4 x i32> %samp, i1 false, i32 0, i32 0)
  %x = extractelement <4 x float> %v, i32 0
  call void @llvm.amdgcn.exp.f32(i32 0, i32 15, float %x, float %x, float %x, float %x, i1 true, i1 true)
  ret void
}
