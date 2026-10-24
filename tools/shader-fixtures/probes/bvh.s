// The ray-tracing image instructions, `image_bvh_intersect_ray` and `image_bvh64_intersect_ray`:
// a ray against one node of a bounding-volume hierarchy. Grand Theft Auto V's scene colour on its
// default graphics mode comes from a compute pass that uses the first.
//
// target triple: amdgcn-amd-amdhsa
//
// Printed order is data, address, resource. The data is always four registers and the resource
// always four (the instruction requires a 128-bit descriptor); the address is eleven or twelve
// registers. The `a16` form (eight or nine) is left out: the solver would take its trailing
// modifier for an operand, and the translator reads the bit from the second word (bit 62, where
// the reference assembler encodes `a16` for these too). Each field is sampled at its top and at
// two other values, so no narrower window explains it: `v[252:255]` sets the data field's top bit, `v[245:255]` the
// address field's, and `s[64:67]` (16) the resource's five-bit quarter-scale field's.

// ---- image_bvh_intersect_ray -------------------------------------------------------
image_bvh_intersect_ray v[4:7], v[0:10], s[4:7]
image_bvh_intersect_ray v[252:255], v[100:110], s[64:67]
image_bvh_intersect_ray v[40:43], v[245:255], s[100:103]
image_bvh_intersect_ray v[128:131], v[17:27], s[36:39]

// ---- image_bvh64_intersect_ray -----------------------------------------------------
image_bvh64_intersect_ray v[4:7], v[0:11], s[4:7]
image_bvh64_intersect_ray v[252:255], v[100:111], s[64:67]
image_bvh64_intersect_ray v[40:43], v[244:255], s[100:103]
image_bvh64_intersect_ray v[128:131], v[17:28], s[36:39]
