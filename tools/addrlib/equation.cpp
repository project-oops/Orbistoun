// A swizzle mode's addressing equation, read off Mesa's addrlib under the console's configuration.
//
// # Why this exists
//
// orbistoun detiles and retiles surfaces with an XOR equation per layout and texel size: where
// each low bit of `x` and of `y` lands in a block's byte offset. addrlib is the published
// reference for those equations, but it is configuration-driven C++ and orbistoun is Rust, so the
// equation is read off it once, one coordinate bit at a time, and carried as data in `tiling.rs`.
// Every layout so far was read that way by hand; this is that reading kept, so the next texel size
// is a command rather than a scratch harness.
//
// # What it checks
//
// The basis alone is only a claim that the layout is linear in the coordinates. The tool also
// computes addrlib's address for every texel of two surfaces, eight blocks square and five by three,
// and checks that row-major blocks, each laid out by the terms, give the same offset. Terms for
// coordinate bits above the block are printed apart: a mode whose pipe or bank bits draw on higher
// coordinates has them. It exits non-zero if any texel disagrees.
//
// # Configuration
//
// Family NV (0x8F), revision 0x82, `GB_ADDR_CONFIG` 0x4: the configuration whose 32-bpp
// `64KB_R_X` equation is the layout measured on the console (see `tiling.rs`).
//
// Usage: equation <swizzle mode> <bytes per texel> [blocks] [3d]
//   blocks: how many blocks a side the terms are read and checked over, a power of two (8)
//   swizzle mode: 64KB_R_X, 64KB_D_X, 4KB_D_X, 64KB_S, 4KB_S, 64KB_Z_X
//   3d: a three-dimensional surface, whose terms for `z` are read and checked as well; its blocks
//       run across, then down, then through the slices
//
// Build with build.sh beside this file.

#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>

#include "addrinterface.h"

namespace {

constexpr UINT_32 kFamilyNv = 0x8F;
constexpr UINT_32 kRevision = 0x82;
constexpr UINT_32 kGbAddrConfig = 0x4;
// The engine addrlib serves GFX9 and later through, as Mesa defines it (`ac_surface.c` 90).
constexpr UINT_32 kArcticIslandEngine = 0xD;

void *ADDR_API alloc_sys_mem(const ADDR_ALLOCSYSMEM_INPUT *input) {
  return std::malloc(input->sizeInBytes);
}

ADDR_E_RETURNCODE ADDR_API free_sys_mem(const ADDR_FREESYSMEM_INPUT *input) {
  std::free(input->pVirtAddr);
  return ADDR_OK;
}

struct Mode {
  const char *name;
  AddrSwizzleMode mode;
};

constexpr Mode kModes[] = {
    {"64KB_R_X", ADDR_SW_64KB_R_X}, {"64KB_D_X", ADDR_SW_64KB_D_X},
    {"4KB_D_X", ADDR_SW_4KB_D_X},   {"64KB_S", ADDR_SW_64KB_S},
    {"4KB_S", ADDR_SW_4KB_S},     {"64KB_Z_X", ADDR_SW_64KB_Z_X},
};

struct Surface {
  ADDR_HANDLE handle;
  AddrSwizzleMode mode;
  UINT_32 bpp;
  UINT_32 width;
  UINT_32 height;
  // Slices of a three-dimensional surface; one for a two-dimensional one.
  UINT_32 depth;
  bool three_d;
};

AddrResourceType resource_type(bool three_d) {
  return three_d ? ADDR_RSRC_TEX_3D : ADDR_RSRC_TEX_2D;
}

UINT_64 address_of(const Surface &surface, UINT_32 x, UINT_32 y, UINT_32 z) {
  ADDR2_COMPUTE_SURFACE_ADDRFROMCOORD_INPUT in = {};
  in.size = sizeof(in);
  in.x = x;
  in.y = y;
  in.slice = z;
  in.swizzleMode = surface.mode;
  in.resourceType = resource_type(surface.three_d);
  in.bpp = surface.bpp;
  in.unalignedWidth = surface.width;
  in.unalignedHeight = surface.height;
  in.numSlices = surface.depth;
  in.numMipLevels = 1;
  in.numSamples = 1;
  in.numFrags = 1;
  ADDR2_COMPUTE_SURFACE_ADDRFROMCOORD_OUTPUT out = {};
  out.size = sizeof(out);
  if (Addr2ComputeSurfaceAddrFromCoord(surface.handle, &in, &out) != ADDR_OK) {
    std::fprintf(stderr, "addrlib refused (%u, %u, %u)\n", x, y, z);
    std::exit(2);
  }
  return out.addr;
}

UINT_32 log2_of(UINT_32 value) {
  UINT_32 bits = 0;
  while ((1u << bits) < value) {
    ++bits;
  }
  return bits;
}

} // namespace

int main(int argc, char **argv) {
  const bool three_d = argc >= 4 && std::strcmp(argv[argc - 1], "3d") == 0;
  // A two-dimensional array: what a layer adds beyond its slice's place is the question.
  const bool array = argc >= 4 && std::strcmp(argv[argc - 1], "array") == 0;
  const int counted = three_d || array ? argc - 1 : argc;
  if (counted != 3 && counted != 4) {
    std::fprintf(stderr,
                 "usage: equation <swizzle mode> <bytes per texel> [blocks] [3d | array]\n");
    return 1;
  }
  const Mode *chosen = nullptr;
  for (const Mode &mode : kModes) {
    if (std::strcmp(mode.name, argv[1]) == 0) {
      chosen = &mode;
    }
  }
  const UINT_32 bytes = static_cast<UINT_32>(std::strtoul(argv[2], nullptr, 0));
  if (chosen == nullptr || bytes == 0 || (bytes & (bytes - 1)) != 0 || bytes > 16) {
    std::fprintf(stderr, "unknown swizzle mode or texel size\n");
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

  // The block's extent at this texel size, from addrlib itself.
  ADDR2_COMPUTE_SURFACE_INFO_INPUT info_in = {};
  info_in.size = sizeof(info_in);
  info_in.swizzleMode = chosen->mode;
  info_in.resourceType = resource_type(three_d);
  info_in.bpp = bytes * 8;
  info_in.width = 1;
  info_in.height = 1;
  info_in.numSlices = 1;
  info_in.numMipLevels = 1;
  info_in.numSamples = 1;
  info_in.numFrags = 1;
  ADDR2_COMPUTE_SURFACE_INFO_OUTPUT info = {};
  info.size = sizeof(info);
  if (Addr2ComputeSurfaceInfo(created.hLib, &info_in, &info) != ADDR_OK) {
    std::fprintf(stderr, "addrlib refused the surface\n");
    return 2;
  }
  const UINT_32 block_width = info.blockWidth;
  const UINT_32 block_height = info.blockHeight;
  const UINT_32 block_depth = three_d ? info.blockSlices : 1;
  const UINT_64 block_bytes =
      static_cast<UINT_64>(block_width) * block_height * block_depth * bytes;

  // One basis term per coordinate bit, up to an eight-block surface: the in-block equation, then
  // what each bit above the block adds inside the block beyond moving to the next one. A layout
  // whose pipe or bank bits draw on higher coordinates shows terms above the block.
  const UINT_32 most_blocks =
      counted == 4 ? static_cast<UINT_32>(std::strtoul(argv[3], nullptr, 0)) : 8;
  const UINT_32 x_block_bits = log2_of(block_width);
  const UINT_32 y_block_bits = log2_of(block_height);
  const UINT_32 z_block_bits = log2_of(block_depth);
  // A three-dimensional surface is read over two blocks across and down and `blocks` through the
  // slices: what a slice adds is the question, and a surface as wide as it is deep costs the check
  // the cube of the count.
  const UINT_32 side_blocks = three_d ? 2 : most_blocks;
  const UINT_32 x_bits = x_block_bits + log2_of(side_blocks);
  const UINT_32 y_bits = y_block_bits + log2_of(side_blocks);
  // A two-dimensional surface has one slice, and no bit of `z`; an array has `blocks` layers.
  const UINT_32 z_bits =
      three_d ? z_block_bits + log2_of(most_blocks) : (array ? log2_of(most_blocks) : 0);
  const UINT_32 most_depth = three_d ? most_blocks * block_depth : (array ? most_blocks : 1);
  const Surface widest = {created.hLib,
                          chosen->mode,
                          bytes * 8,
                          side_blocks * block_width,
                          side_blocks * block_height,
                          most_depth,
                          three_d};
  auto row_major = [&](const Surface &surface, UINT_32 x, UINT_32 y, UINT_32 z) {
    // A row or a slice is padded to whole blocks.
    const UINT_64 per_row = (surface.width + block_width - 1) / block_width;
    const UINT_64 per_slice = per_row * ((surface.height + block_height - 1) / block_height);
    return ((z / block_depth) * per_slice + (y / block_height) * per_row + (x / block_width)) *
           block_bytes;
  };
  UINT_64 x_basis[32] = {};
  UINT_64 y_basis[32] = {};
  UINT_64 z_basis[32] = {};
  for (UINT_32 bit = 0; bit < x_bits; ++bit) {
    x_basis[bit] = address_of(widest, 1u << bit, 0, 0) ^ row_major(widest, 1u << bit, 0, 0);
  }
  for (UINT_32 bit = 0; bit < y_bits; ++bit) {
    y_basis[bit] = address_of(widest, 0, 1u << bit, 0) ^ row_major(widest, 0, 1u << bit, 0);
  }
  for (UINT_32 bit = 0; bit < z_bits; ++bit) {
    z_basis[bit] = address_of(widest, 0, 0, 1u << bit) ^ row_major(widest, 0, 0, 1u << bit);
  }

  // Every texel of three surfaces of different shape: row-major blocks, each by the basis. The
  // third is a quarter of a block across and down, padded to one - a small volume lies so.
  const Surface shapes[] = {widest,
                            {created.hLib, chosen->mode, bytes * 8, 5 * block_width,
                             3 * block_height, three_d ? 3 * block_depth : most_depth, three_d},
                            {created.hLib, chosen->mode, bytes * 8, block_width / 4,
                             block_height / 4, most_depth, three_d}};
  UINT_64 wrong = 0;
  UINT_64 checked = 0;
  for (const Surface &surface : shapes) {
    for (UINT_32 z = 0; z < surface.depth; ++z) {
      for (UINT_32 y = 0; y < surface.height; ++y) {
        for (UINT_32 x = 0; x < surface.width; ++x) {
          UINT_64 expected = row_major(surface, x, y, z);
          for (UINT_32 bit = 0; bit < x_bits; ++bit) {
            if ((x >> bit) & 1) {
              expected ^= x_basis[bit];
            }
          }
          for (UINT_32 bit = 0; bit < y_bits; ++bit) {
            if ((y >> bit) & 1) {
              expected ^= y_basis[bit];
            }
          }
          for (UINT_32 bit = 0; bit < z_bits; ++bit) {
            if ((z >> bit) & 1) {
              expected ^= z_basis[bit];
            }
          }
          const UINT_64 actual = address_of(surface, x, y, z);
          ++checked;
          if (actual != expected) {
            // The first few, so a layout this rule does not describe says how it differs.
            if (wrong < 8) {
              std::printf("%u-texel-wide surface, (%u, %u, %u): addrlib 0x%llx, the rule 0x%llx\n",
                          surface.width, x, y, z, static_cast<unsigned long long>(actual),
                          static_cast<unsigned long long>(expected));
            }
            ++wrong;
          }
        }
      }
    }
  }

  auto print_terms = [](const char *axis, const UINT_64 *basis, UINT_32 from, UINT_32 to) {
    std::printf("%s: [", axis);
    for (UINT_32 bit = from; bit < to; ++bit) {
      std::printf("%s0x%llx", bit == from ? "" : ", ", static_cast<unsigned long long>(basis[bit]));
    }
    std::printf("]\n");
  };
  std::printf("%s%s at %u bytes a texel: %u x %u x %u-texel blocks of %llu bytes\n", chosen->name,
              three_d ? " (3D)" : (array ? " (2D array)" : ""), bytes, block_width, block_height,
              block_depth,
              static_cast<unsigned long long>(block_bytes));
  print_terms("x in the block", x_basis, 0, x_block_bits);
  print_terms("y in the block", y_basis, 0, y_block_bits);
  if (three_d) {
    print_terms("z in the block", z_basis, 0, z_block_bits);
  }
  print_terms("x above the block", x_basis, x_block_bits, x_bits);
  print_terms("y above the block", y_basis, y_block_bits, y_bits);
  if (three_d || array) {
    print_terms(array ? "layer beyond its slice" : "z above the block", z_basis, z_block_bits,
                z_bits);
  }
  std::printf("%llu of %llu texels disagree with row-major blocks under these terms\n",
              static_cast<unsigned long long>(wrong), static_cast<unsigned long long>(checked));
  AddrDestroy(created.hLib);
  return wrong == 0 ? 0 : 3;
}
