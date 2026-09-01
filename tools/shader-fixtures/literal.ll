; Forces a trailing 32-bit literal. 0x12345678 is not one of the inline constants the
; hardware can encode in an operand field, so the assembler must emit a literal dword
; after the instruction.
;
; This is the single highest-risk path in the decoder: a missed literal is read as an
; instruction, and every instruction after it decodes from the wrong offset.
target triple = "amdgcn-amd-amdhsa"

define amdgpu_kernel void @literal(ptr addrspace(1) %out, i32 %a) {
entry:
  %x = add i32 %a, 305419896
  %y = xor i32 %x, 2023406814
  store i32 %y, ptr addrspace(1) %out
  ret void
}
