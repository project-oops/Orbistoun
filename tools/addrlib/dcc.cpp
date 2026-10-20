// A 64KB colour surface's DCC key equation, read off Mesa's addrlib under the console's
// configuration.
//
// # Why this exists
//
// A DCC key byte describes one compress block of the surface; which byte describes which block is
// addrlib's meta equation. Mesa carries the GFX10 equation as data (`gfx9_meta_equation`,
// `ac_surface.h` 182-219) and reads a key's address off it in `gfx10_nir_meta_addr_from_coord`
// (`ac_nir_surface.c` 11-57). orbistoun does the same in Rust, so the equation is read off addrlib
// once per texel size and pipe alignment, and carried as data in `dcc.rs`.
//
// # What it checks
//
// Mesa's address function, as written here, against `Addr2ComputeDccAddrFromCoord` for every
// compress block of three surfaces, under a pipe XOR of zero and two others. It exits non-zero if
// any block disagrees.
//
//   dcc <bytes per texel> <pipe aligned: 0 or 1> [slices]
//
// The layout is 64KB_R_X, or the one `DCC_SWIZZLE` names: 64KB_S_X, 64KB_Z_X or 64KB_R_X.
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>

#include "addrinterface.h"

namespace {

constexpr UINT_32 kFamilyNv = 0x8F;
constexpr UINT_32 kRevision = 0x82;
constexpr UINT_32 kGbAddrConfig = 0x4;
constexpr UINT_32 kArcticIslandEngine = 0xD;
// `GB_ADDR_CONFIG.NUM_PIPES` (bits 2:0) and `PIPE_INTERLEAVE_SIZE` (bits 5:3) of `kGbAddrConfig`.
constexpr UINT_32 kPipesLog2 = kGbAddrConfig & 0x7;
constexpr UINT_32 kPipeInterleaveLog2 = 8 + ((kGbAddrConfig >> 3) & 0x7);

void *ADDR_API alloc_sys_mem(const ADDR_ALLOCSYSMEM_INPUT *input) {
  return std::malloc(input->sizeInBytes);
}

ADDR_E_RETURNCODE ADDR_API free_sys_mem(const ADDR_FREESYSMEM_INPUT *input) {
  std::free(input->pVirtAddr);
  return ADDR_OK;
}

UINT_32 log2_of(UINT_32 value) {
  UINT_32 bits = 0;
  while ((1u << bits) < value) {
    ++bits;
  }
  return bits;
}

struct Dcc {
  ADDR2_COMPUTE_DCCINFO_OUTPUT info;
  UINT_16 bits[64];
};

// The swizzle mode `DCC_SWIZZLE` names, 64KB_R_X where it names none.
AddrSwizzleMode swizzle_mode() {
  const char *name = std::getenv("DCC_SWIZZLE");
  if (name == nullptr || std::strcmp(name, "64KB_R_X") == 0) {
    return ADDR_SW_64KB_R_X;
  }
  if (std::strcmp(name, "64KB_S_X") == 0) {
    return ADDR_SW_64KB_S_X;
  }
  if (std::strcmp(name, "64KB_Z_X") == 0) {
    return ADDR_SW_64KB_Z_X;
  }
  std::fprintf(stderr, "DCC_SWIZZLE is 64KB_S_X, 64KB_Z_X or 64KB_R_X\n");
  std::exit(1);
}

const char *swizzle_name() {
  const char *name = std::getenv("DCC_SWIZZLE");
  return name == nullptr ? "64KB_R_X" : name;
}

Dcc dcc_info(ADDR_HANDLE handle, UINT_32 bytes, bool pipe_aligned, UINT_32 width, UINT_32 height,
             UINT_32 slices = 1) {
  ADDR2_COMPUTE_DCCINFO_INPUT in = {};
  in.size = sizeof(in);
  in.dccKeyFlags.pipeAligned = pipe_aligned;
  in.resourceType = ADDR_RSRC_TEX_2D;
  in.swizzleMode = swizzle_mode();
  in.bpp = bytes * 8;
  in.unalignedWidth = width;
  in.unalignedHeight = height;
  in.numSlices = slices;
  in.numFrags = 1;
  in.numMipLevels = 1;
  in.firstMipIdInTail = 1;
  Dcc out = {};
  out.info.size = sizeof(out.info);
  if (Addr2ComputeDccInfo(handle, &in, &out.info) != ADDR_OK) {
    std::fprintf(stderr, "addrlib refused the DCC surface %ux%u\n", width, height);
    std::exit(2);
  }
  // Mesa keeps elements 4..68 of the equation; the first four and the last are zero
  // (`ac_copy_dcc_equation`, `ac_surface.c` 2189-2198).
  for (UINT_32 i = 0; i < 64; ++i) {
    out.bits[i] = out.info.equation.gfx10_bits[i + 4];
  }
  return out;
}

UINT_64 addrlib_address(ADDR_HANDLE handle, const Dcc &dcc, UINT_32 bytes, bool pipe_aligned,
                        UINT_32 width, UINT_32 height, UINT_32 x, UINT_32 y, UINT_32 pipe_xor,
                        UINT_32 slice = 0, UINT_32 slices = 1) {
  ADDR2_COMPUTE_DCC_ADDRFROMCOORD_INPUT in = {};
  in.size = sizeof(in);
  in.x = x;
  in.y = y;
  in.slice = slice;
  in.dccKeyFlags.pipeAligned = pipe_aligned;
  in.resourceType = ADDR_RSRC_TEX_2D;
  in.swizzleMode = swizzle_mode();
  in.bpp = bytes * 8;
  in.unalignedWidth = width;
  in.unalignedHeight = height;
  in.numSlices = slices;
  in.numMipLevels = 1;
  in.numFrags = 1;
  in.pipeXor = pipe_xor;
  in.pitch = dcc.info.pitch;
  in.height = dcc.info.height;
  in.compressBlkWidth = dcc.info.compressBlkWidth;
  in.compressBlkHeight = dcc.info.compressBlkHeight;
  in.compressBlkDepth = dcc.info.compressBlkDepth;
  in.metaBlkWidth = dcc.info.metaBlkWidth;
  in.metaBlkHeight = dcc.info.metaBlkHeight;
  in.metaBlkDepth = dcc.info.metaBlkDepth;
  in.dccRamSliceSize = dcc.info.dccRamSliceSize;
  ADDR2_COMPUTE_DCC_ADDRFROMCOORD_OUTPUT out = {};
  out.size = sizeof(out);
  if (Addr2ComputeDccAddrFromCoord(handle, &in, &out) != ADDR_OK) {
    std::fprintf(stderr, "addrlib refused the key of (%u, %u)\n", x, y);
    std::exit(2);
  }
  return out.addr;
}

// Mesa's `gfx10_nir_meta_addr_from_coord` (`ac_nir_surface.c` 11-57) with integers for its NIR,
// as `ac_nir_dcc_addr_from_coord` calls it: a bias of `log2(bytes) - 8`, the address from bit one.
UINT_64 mesa_address(const Dcc &dcc, UINT_32 bytes, UINT_32 x, UINT_32 y, UINT_32 pipe_xor,
                     UINT_32 z = 0) {
  const UINT_32 width_log2 = log2_of(dcc.info.metaBlkWidth);
  const UINT_32 height_log2 = log2_of(dcc.info.metaBlkHeight);
  const int bias = static_cast<int>(log2_of(bytes)) - 8;
  const UINT_32 size_log2 = static_cast<UINT_32>(static_cast<int>(width_log2 + height_log2) + bias);
  const UINT_32 start = 1;
  const UINT_32 coords[4] = {x, y, z, 0};
  UINT_64 address = 0;
  for (UINT_32 i = start; i < size_log2 + 1; ++i) {
    UINT_32 v = 0;
    for (UINT_32 c = 0; c < 4; ++c) {
      UINT_32 mask = dcc.bits[i * 4 + c - start * 4];
      while (mask) {
        const UINT_32 bit = __builtin_ctz(mask);
        mask &= mask - 1;
        v ^= (coords[c] >> bit) & 1;
      }
    }
    address |= static_cast<UINT_64>(v) << i;
  }
  const UINT_64 block_mask = (1ull << size_log2) - 1;
  const UINT_64 pipe_mask = (1ull << kPipesLog2) - 1;
  const UINT_64 index =
      static_cast<UINT_64>(y >> height_log2) * (dcc.info.pitch >> width_log2) + (x >> width_log2);
  const UINT_64 pipe = ((pipe_xor & pipe_mask) << kPipeInterleaveLog2) & block_mask;
  return static_cast<UINT_64>(dcc.info.dccRamSliceSize) * z + (index << size_log2) +
         ((address >> 1) ^ pipe);
}

} // namespace

int main(int argc, char **argv) {
  if (argc != 3 && argc != 4) {
    std::fprintf(stderr, "usage: dcc <bytes per texel> <pipe aligned: 0 or 1> [slices]\n");
    return 1;
  }
  // An array's keys: each slice's at its place, by the equation's slice terms.
  const UINT_32 slices = argc == 4 ? static_cast<UINT_32>(std::strtoul(argv[3], nullptr, 0)) : 1;
  const UINT_32 bytes = static_cast<UINT_32>(std::strtoul(argv[1], nullptr, 0));
  const bool pipe_aligned = std::strtoul(argv[2], nullptr, 0) != 0;
  if (bytes == 0 || (bytes & (bytes - 1)) != 0 || bytes > 16) {
    std::fprintf(stderr, "a texel is one to sixteen bytes, a power of two\n");
    return 1;
  }

  ADDR_CREATE_INPUT create = {};
  create.size = sizeof(create);
  create.chipEngine = kArcticIslandEngine;
  create.chipFamily = kFamilyNv;
  create.chipRevision = kRevision;
  create.regValue.gbAddrConfig = kGbAddrConfig;
  create.callbacks.allocSysMem = alloc_sys_mem;
  create.callbacks.freeSysMem = free_sys_mem;
  ADDR_CREATE_OUTPUT created = {};
  created.size = sizeof(created);
  if (AddrCreate(&create, &created) != ADDR_OK) {
    std::fprintf(stderr, "addrlib refused the configuration\n");
    return 2;
  }

  const Dcc probe = dcc_info(created.hLib, bytes, pipe_aligned, 1920, 1080, slices);
  std::printf("%s at %u bytes a texel, %s: compress blocks %u x %u, meta blocks %u x %u "
              "of %u bytes\n",
              swizzle_name(), bytes, pipe_aligned ? "pipe aligned" : "unaligned", probe.info.compressBlkWidth,
              probe.info.compressBlkHeight, probe.info.metaBlkWidth, probe.info.metaBlkHeight,
              probe.info.metaBlkSize);
  std::printf("equation: [");
  for (UINT_32 i = 0; i < 64; ++i) {
    std::printf("%s0x%x", i == 0 ? "" : ", ", probe.bits[i]);
  }
  std::printf("]\n");
  const UINT_32 widths[] = {1, 100, 480, 1920, 2048, 4000};
  for (UINT_32 width : widths) {
    const Dcc sized = dcc_info(created.hLib, bytes, pipe_aligned, width, 64);
    std::printf("width %u: key pitch %u, height %u for 64 rows, %u bytes a slice\n", width,
                sized.info.pitch, sized.info.height, sized.info.dccRamSliceSize);
  }

  // Addresses to check a port against: a 1920 x 1080 surface's keys at a few blocks.
  {
    const Dcc dcc = dcc_info(created.hLib, bytes, pipe_aligned, 1920, 1080);
    const UINT_32 samples[][3] = {{8, 0, 0},     {0, 16, 0},   {200, 100, 0},
                                  {1000, 600, 5}, {1912, 1076, 13}, {520, 260, 0}};
    std::printf("samples (x, y, pipe xor, key byte):");
    for (const auto &sample : samples) {
      std::printf(" (%u, %u, %u, 0x%llx)", sample[0], sample[1], sample[2],
                  static_cast<unsigned long long>(addrlib_address(
                      created.hLib, dcc, bytes, pipe_aligned, 1920, 1080, sample[0], sample[1],
                      sample[2])));
    }
    std::printf("\n");
  }
  // The same blocks of slice three of a six-slice array: its keys a slice of keys on, by the
  // equation's slice terms.
  {
    const Dcc dcc = dcc_info(created.hLib, bytes, pipe_aligned, 1920, 1080, 6);
    const UINT_32 samples[][3] = {{8, 0, 0}, {200, 100, 0}, {1000, 600, 5}};
    std::printf("slice 3 of 6, %u bytes a slice (x, y, pipe xor, key byte):",
                dcc.info.dccRamSliceSize);
    for (const auto &sample : samples) {
      std::printf(" (%u, %u, %u, 0x%llx)", sample[0], sample[1], sample[2],
                  static_cast<unsigned long long>(addrlib_address(
                      created.hLib, dcc, bytes, pipe_aligned, 1920, 1080, sample[0], sample[1],
                      sample[2], 3, 6)));
    }
    std::printf("\n");
  }

  // Every compress block of three surfaces, under three pipe XORs.
  const UINT_32 shapes[][2] = {{1920, 1080}, {480, 270}, {4000, 300}};
  const UINT_32 pipe_xors[] = {0, 5, 13};
  UINT_64 wrong = 0;
  UINT_64 checked = 0;
  for (const auto &shape : shapes) {
    const Dcc dcc = dcc_info(created.hLib, bytes, pipe_aligned, shape[0], shape[1], slices);
    for (UINT_32 pipe_xor : pipe_xors) {
      for (UINT_32 z = 0; z < slices; ++z)
      for (UINT_32 y = 0; y < shape[1]; y += dcc.info.compressBlkHeight) {
        for (UINT_32 x = 0; x < shape[0]; x += dcc.info.compressBlkWidth) {
          const UINT_64 expected =
              addrlib_address(created.hLib, dcc, bytes, pipe_aligned, shape[0], shape[1], x, y,
                              pipe_xor, z, slices);
          const UINT_64 actual = mesa_address(dcc, bytes, x, y, pipe_xor, z);
          ++checked;
          if (actual != expected) {
            if (wrong < 8) {
              std::printf("%ux%u, pipe xor %u, (%u, %u): addrlib 0x%llx, the equation 0x%llx\n",
                          shape[0], shape[1], pipe_xor, x, y,
                          static_cast<unsigned long long>(expected),
                          static_cast<unsigned long long>(actual));
            }
            ++wrong;
          }
        }
      }
    }
  }
  std::printf("%llu of %llu blocks disagree with addrlib under the equation\n",
              static_cast<unsigned long long>(wrong), static_cast<unsigned long long>(checked));
  AddrDestroy(created.hLib);
  return wrong == 0 ? 0 : 3;
}
