# 2026-08-31 (later) - second payload run absorbed: five vaddrs confirmed (D419)


The D274 obSCEne changes came back from hardware exactly as hoped. The 139-exports section confirmed
twelve vaddrs (up from eight): the previously-refuted sceKernelGetTscFrequency (0x1cf30) now confirms
under the widened band, and the four new candidates - getsid 0x1590, sceKernelReadTsc 0x1cfa0,
sceKernelGetProcessTimeCounter 0x1d010, sceKernelGetProcessTimeCounterFrequency 0x1d030 - all pass.
Each already sat in libkernel-vaddrs.txt as a candidate with the exact vaddr hardware reported, so
this was confirmation, not correction. Promoted all five to `confirmed` (7→12); firmware tests pass.

The trap to not fall into: this payload run shows 363 fails, all under 900-surface/* ("none of this
library is present"). That is the payload path, not a regression - elfldr resolves only base+vaddr
exports, so the whole-surface census sees no import tables. The number that matters is 139-exports:
12/12, 0xc confirmed. No byte dumps this run, so no new struct layouts; SwVersion stays the last one.

Next data still wants hardware, not code: a native-PS5-mode run to un-refuse GetModuleInfo and give
PS5-native module-info layout. Nothing more to crunch here unattended.

