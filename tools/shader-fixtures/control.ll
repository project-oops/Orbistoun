; Branching. Produces scalar control-flow instructions and execution-mask
; manipulation - the instructions that matter most for reconstructing structure, and
; the ones a translator will find hardest.
target triple = "amdgcn-amd-amdhsa"

define amdgpu_kernel void @control(ptr addrspace(1) %out, i32 %n) {
entry:
  %c = icmp sgt i32 %n, 0
  br i1 %c, label %then, label %otherwise

then:
  store i32 1, ptr addrspace(1) %out
  br label %done

otherwise:
  store i32 2, ptr addrspace(1) %out
  br label %done

done:
  ret void
}
