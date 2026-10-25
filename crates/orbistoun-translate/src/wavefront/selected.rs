//! Images a dispatch selects from a descriptor table at run time, read through arrays (D983).
//!
//! A program loads an image descriptor's eight words through a table's descriptor at an offset it
//! computes - from a lane's value, made uniform by a waterfall - and samples through them. The host
//! cannot know which record a wave selects, so it binds every image the table's records name, in
//! arrays of sampled images - one of flat images, one of cubes, each image at the same place in
//! both - and a directory of their descriptors. The module looks the eight words its registers hold
//! up in the directory as it runs and reads the image found there. Words the directory does not
//! hold, or hold as the other shape, are an escape (D950): the access reads the array's first
//! image, and the dispatch's result is refused.
//!
//! The directory is a storage buffer of words. Word zero is a mask, one less than its slot count, a
//! power of two. The slots follow, [`SELECTED_DIRECTORY_SLOT_WORDS`] words each: a descriptor's
//! eight words, then its tag - its image's place in the arrays plus one, with
//! [`SELECTED_CUBE_TAG`] set for a cube - zero in an empty slot. A descriptor lies at the slot
//! [`selected_image_hash`] names, masked, or within [`SELECTED_DIRECTORY_PROBES`] slots after it,
//! wrapping.

use orbistoun_spirv::{Id, capability, decoration, op, storage};

use super::{BoundTexture, ComputedImage, Stage, TextureSource, UNIFORM_CONSTANT, Wavefront};
use crate::draw_buffers::SelectedImage;
use crate::model::{self, Model};

/// Words one slot of a selected image directory takes: a descriptor's eight words, then its tag.
pub const SELECTED_DIRECTORY_SLOT_WORDS: u32 = 9;

/// How many slots from its own a descriptor's lookup searches: the host grows the directory until
/// every descriptor lies within them.
pub const SELECTED_DIRECTORY_PROBES: u32 = 4;

/// In a directory slot's tag: the image is a cube, bound in the array of cubes.
pub const SELECTED_CUBE_TAG: u32 = 1 << 31;

/// The multiplier of [`selected_image_hash`]: the golden ratio's fraction of 2^32.
const SELECTED_HASH: u32 = 0x9e37_79b1;

/// The hash of an image descriptor's eight words a directory places it by: each word mixed in by a
/// multiply and a fold of its high bits. The module forms the same.
#[must_use]
pub fn selected_image_hash(words: &[u32; 8]) -> u32 {
    words.iter().fold(0_u32, |hash, &word| {
        let mixed = (hash ^ word).wrapping_mul(SELECTED_HASH);
        mixed ^ (mixed >> 15)
    })
}

/// A directory of `descriptors`, each with whether its image is a cube, found at its place in the
/// arrays: the smallest power-of-two table, at least twice their count, in which every one lies
/// within [`SELECTED_DIRECTORY_PROBES`] slots of its hash's. `None` for more than
/// [`orbistoun_spirv::MOST_SELECTED_IMAGES`] descriptors.
#[must_use]
pub fn selected_directory(descriptors: &[([u32; 8], bool)]) -> Option<Vec<u32>> {
    if descriptors.len() > orbistoun_spirv::MOST_SELECTED_IMAGES as usize {
        return None;
    }
    let width = SELECTED_DIRECTORY_SLOT_WORDS as usize;
    let mut slots = (descriptors.len() * 2).next_power_of_two().max(4);
    'grow: loop {
        let mask = u32::try_from(slots - 1).ok()?;
        let mut table = vec![0_u32; 1 + slots * width];
        table[0] = mask;
        for (place, (words, cube)) in descriptors.iter().enumerate() {
            let home = selected_image_hash(words) & mask;
            let free = (0..SELECTED_DIRECTORY_PROBES)
                .map(|probe| (home.wrapping_add(probe) & mask) as usize)
                .find(|&slot| table[1 + slot * width + 8] == 0);
            let Some(slot) = free else {
                slots *= 2;
                continue 'grow;
            };
            let at = 1 + slot * width;
            table[at..at + 8].copy_from_slice(words);
            table[at + 8] =
                (u32::try_from(place).ok()? + 1) | if *cube { SELECTED_CUBE_TAG } else { 0 };
        }
        return Some(table);
    }
}

/// One array of selected images, of one shape: its variable, a pointer to one image in it, and
/// the image type.
#[derive(Debug, Clone, Copy)]
struct ImageArray {
    images: Id,
    image_pointer: Id,
    image: Id,
}

/// What a module reading selected images declares once: the arrays of images, the directory, and
/// the types a lookup takes.
#[derive(Debug, Clone, Copy)]
pub(super) struct SelectedSet {
    /// Flat float images read through a sampler.
    flat: ImageArray,
    /// Cubes, the same.
    cube: ImageArray,
    /// The sampler type each selecting slot binds.
    sampler: Id,
    /// A pointer to a slot's sampler.
    sampler_pointer: Id,
    /// The directory buffer.
    directory: Id,
    /// A pointer to one of its words.
    directory_word: Id,
}

impl Wavefront<'_> {
    /// The texture an image access at byte `at` reads through `descriptor`, a record the program
    /// selected from a table at run time (D983): the slot every access selecting from the same
    /// table through the same sampler, in the same shape, shares, and the image this access's
    /// registers name, looked up as the module runs.
    pub(super) fn selected_texture(
        &mut self,
        descriptor: u32,
        sampler: Option<u32>,
        (at, shape, selected): (u32, model::ImageShape, SelectedImage),
    ) -> Result<model::Texture, &'static str> {
        if shape.volume() || shape.layered() {
            return Err(concat!(
                "an image a dispatch selects from a table at run time is read as a volume or an ",
                "array, and only a flat image or a cube is read so (D983)"
            ));
        }
        let shared = self.textures.iter().find(|bound| {
            bound.texture.shape == shape
                && bound.source.computed.and_then(|computed| computed.selected) == Some(selected)
        });
        let texture = match shared {
            Some(bound) => bound.texture,
            None => self.selected_slot(descriptor, sampler, (at, shape, selected))?,
        };
        let (loaded, missing) = self.selected_lookup(descriptor, texture);
        Ok(model::Texture {
            loaded: Some(loaded),
            missing: Some(missing),
            ..texture
        })
    }

    /// The coordinates, across then down, texture slot `slot`'s samples saturate and mirror once
    /// (D743).
    pub(super) fn slot_modes(&self, slot: u32) -> ([bool; 2], [bool; 2]) {
        let mode = |modes: &[[bool; 2]]| modes.get(slot as usize).copied().unwrap_or_default();
        (mode(&self.saturated), mode(&self.mirrored))
    }

    /// Declares a texture slot reading images selected from `selected`'s table: its sampler at the
    /// slot's own binding, the images in the shared arrays.
    fn selected_slot(
        &mut self,
        descriptor: u32,
        sampler: Option<u32>,
        (at, shape, selected): (u32, model::ImageShape, SelectedImage),
    ) -> Result<model::Texture, &'static str> {
        if self.stage != Stage::Compute {
            return Err("only a dispatch selects an image from a table at run time (D983)");
        }
        let slot = u32::try_from(self.textures.len()).unwrap_or(u32::MAX);
        let Some(&binding) = orbistoun_spirv::TEXTURE_BINDINGS.get(slot as usize) else {
            return Err(concat!(
                "this shader reads more textures than its stage has bindings: thirty-two, the ",
                "last eight a primitive shader's (D690)"
            ));
        };
        if self.integer_images[0] || self.integer_fetched >> slot & 1 != 0 {
            return Err(concat!(
                "an image a dispatch selects from a table at run time is an integer one, and the ",
                "arrays the selected images are bound in hold float images (D983)"
            ));
        }
        let set = self.selected_set();
        let variable = self.builder.id();
        self.decorate_binding(variable, binding);
        self.builder.declare(
            op::VARIABLE,
            &[set.sampler_pointer.0, variable.0, UNIFORM_CONSTANT],
        );
        let array = if shape.cube() { set.cube } else { set.flat };
        let combined = self
            .builder
            .declare_type(op::TYPE_SAMPLED_IMAGE, &[array.image.0]);
        let (_, components) = super::image_dim(shape);
        let coordinate = self
            .builder
            .declare_type(op::TYPE_VECTOR, &[self.f32_type.0, components]);
        let texel = self
            .builder
            .declare_type(op::TYPE_VECTOR, &[self.u32_type.0, 2]);
        // A compare is made in the module (D976), by the function and filter the host reads from
        // the slot's sampler; until it has, by none, so the module is translated again once it
        // has.
        let shader_compare = shape
            .compare
            .then(|| super::shader_compare_of(&self.compared, slot, shape).unwrap_or((0, false)));
        let texture = model::Texture {
            variable,
            sampled: combined,
            image: array.image,
            coordinate,
            texel,
            result: self.vec4,
            integer: false,
            saturate: self.slot_modes(slot).0,
            mirror: self.slot_modes(slot).1,
            shader_compare,
            shape,
            loaded: None,
            missing: None,
        };
        self.textures.push(BoundTexture {
            texture,
            descriptor,
            sampler,
            source: TextureSource {
                slot,
                cube: shape.cube(),
                compare: shape.compare,
                computed: Some(ComputedImage {
                    at,
                    register: descriptor,
                    first_register: self.images.user_data_registers.0,
                    selected: Some(selected),
                }),
                ..TextureSource::default()
            },
            disturbed: false,
            sampler_disturbed: false,
            sampler_load: None,
        });
        Ok(texture)
    }

    /// The arrays, the directory and their types, declared the first time a module reads a
    /// selected image, with the capabilities indexing an array by a value each invocation computes
    /// needs. Both arrays are declared, so every module reading selected images binds alike.
    fn selected_set(&mut self) -> SelectedSet {
        if let Some(set) = self.selected_set {
            return set;
        }
        let b = &mut self.builder;
        b.header(op::CAPABILITY, &[capability::SHADER_NON_UNIFORM]);
        b.header(
            op::CAPABILITY,
            &[capability::SAMPLED_IMAGE_ARRAY_NON_UNIFORM_INDEXING],
        );
        b.header(
            op::EXTENSION,
            &orbistoun_spirv::Builder::literal_string(orbistoun_spirv::DESCRIPTOR_INDEXING),
        );
        let length = b.declare_constant(self.u32_type, &[orbistoun_spirv::MOST_SELECTED_IMAGES]);
        let mut array = |dimension: u32, binding: u32| {
            // A float image read through a sampler, as a texture slot's image of the shape is.
            let image =
                b.declare_type(op::TYPE_IMAGE, &[self.f32_type.0, dimension, 0, 0, 0, 1, 0]);
            let (array, array_pointer, image_pointer, images) = (b.id(), b.id(), b.id(), b.id());
            b.declare(op::TYPE_ARRAY, &[array.0, image.0, length.0]);
            b.declare(
                op::TYPE_POINTER,
                &[array_pointer.0, UNIFORM_CONSTANT, array.0],
            );
            b.declare(
                op::TYPE_POINTER,
                &[image_pointer.0, UNIFORM_CONSTANT, image.0],
            );
            b.declare(op::VARIABLE, &[array_pointer.0, images.0, UNIFORM_CONSTANT]);
            b.annotate(op::DECORATE, &[images.0, decoration::DESCRIPTOR_SET, 0]);
            b.annotate(op::DECORATE, &[images.0, decoration::BINDING, binding]);
            ImageArray {
                images,
                image_pointer,
                image,
            }
        };
        let flat = array(super::DIM_2D, orbistoun_spirv::SELECTED_IMAGES_BINDING);
        let cube = array(super::DIM_CUBE, orbistoun_spirv::SELECTED_CUBES_BINDING);
        let sampler = b.declare_type(op::TYPE_SAMPLER, &[]);
        let sampler_pointer = b.id();
        b.declare(
            op::TYPE_POINTER,
            &[sampler_pointer.0, UNIFORM_CONSTANT, sampler.0],
        );
        // The directory: a read-only block of words, as long as the bound buffer.
        let (words, block, block_pointer, directory_word, directory) =
            (b.id(), b.id(), b.id(), b.id(), b.id());
        b.annotate(op::DECORATE, &[words.0, decoration::ARRAY_STRIDE, 4]);
        b.annotate(op::DECORATE, &[block.0, decoration::BLOCK]);
        b.annotate(op::MEMBER_DECORATE, &[block.0, 0, decoration::OFFSET, 0]);
        b.annotate(op::MEMBER_DECORATE, &[block.0, 0, decoration::NON_WRITABLE]);
        b.annotate(op::DECORATE, &[directory.0, decoration::DESCRIPTOR_SET, 0]);
        b.annotate(
            op::DECORATE,
            &[
                directory.0,
                decoration::BINDING,
                orbistoun_spirv::SELECTED_DIRECTORY_BINDING,
            ],
        );
        b.declare(op::TYPE_RUNTIME_ARRAY, &[words.0, self.u32_type.0]);
        b.declare(op::TYPE_STRUCT, &[block.0, words.0]);
        b.declare(
            op::TYPE_POINTER,
            &[block_pointer.0, storage::STORAGE_BUFFER, block.0],
        );
        b.declare(
            op::TYPE_POINTER,
            &[directory_word.0, storage::STORAGE_BUFFER, self.u32_type.0],
        );
        b.declare(
            op::VARIABLE,
            &[block_pointer.0, directory.0, storage::STORAGE_BUFFER],
        );
        let set = SelectedSet {
            flat,
            cube,
            sampler,
            sampler_pointer,
            directory,
            directory_word,
        };
        self.selected_set = Some(set);
        set
    }

    /// One word of the directory, at word `index`.
    fn directory_word(&mut self, set: SelectedSet, index: Id) -> Id {
        let zero = Model::constant(self, 0);
        let u32_type = self.u32_type;
        let b = &mut self.builder;
        let (pointer, value) = (b.id(), b.id());
        b.function(
            op::ACCESS_CHAIN,
            &[
                set.directory_word.0,
                pointer.0,
                set.directory.0,
                zero.0,
                index.0,
            ],
        );
        b.function(op::LOAD, &[u32_type.0, value.0, pointer.0]);
        value
    }

    /// The image the eight registers from `descriptor` name, paired with `texture`'s slot sampler,
    /// and whether the directory lacks them in `texture`'s shape: their hash's slot and the probes
    /// after it, each compared word by word.
    fn selected_lookup(&mut self, descriptor: u32, texture: model::Texture) -> (Id, Id) {
        let set = self.selected_set();
        let array = if texture.shape.cube() {
            set.cube
        } else {
            set.flat
        };
        let words: Vec<Id> = (descriptor..descriptor + model::IMAGE_DESCRIPTOR_REGISTERS)
            .map(|register| self.load_scalar(register))
            .collect();
        let (multiplier, fifteen) = (
            Model::constant(self, SELECTED_HASH),
            Model::constant(self, 15),
        );
        let mut hash = Model::constant(self, 0);
        for &word in &words {
            let mixed = self.binary(op::BITWISE_XOR, hash, word);
            let mixed = self.binary(op::IMUL, mixed, multiplier);
            let high = self.binary(op::SHIFT_RIGHT_LOGICAL, mixed, fifteen);
            hash = self.binary(op::BITWISE_XOR, mixed, high);
        }
        let zero = Model::constant(self, 0);
        let mask = self.directory_word(set, zero);
        let home = self.binary(op::BITWISE_AND, hash, mask);
        let (one, width, eight) = (
            Model::constant(self, 1),
            Model::constant(self, SELECTED_DIRECTORY_SLOT_WORDS),
            Model::constant(self, 8),
        );
        // The tag a hit carries: its shape's bit, the access's.
        let (cube_tag, shape_tag) = (
            Model::constant(self, SELECTED_CUBE_TAG),
            Model::constant(
                self,
                if texture.shape.cube() {
                    SELECTED_CUBE_TAG
                } else {
                    0
                },
            ),
        );
        let place_bits = Model::constant(self, !SELECTED_CUBE_TAG);
        let mut place = zero;
        let mut found = self.compare(op::INOT_EQUAL, zero, zero);
        for probe in 0..SELECTED_DIRECTORY_PROBES {
            let step = Model::constant(self, probe);
            let slot = self.add(home, step);
            let slot = self.binary(op::BITWISE_AND, slot, mask);
            let first = self.binary(op::IMUL, slot, width);
            let first = self.add(first, one);
            let mut same = self.compare(op::IEQUAL, zero, zero);
            for (n, &word) in (0_u32..).zip(&words) {
                let offset = Model::constant(self, n);
                let at = self.add(first, offset);
                let held = self.directory_word(set, at);
                let equal = self.compare(op::IEQUAL, held, word);
                same = self.both(same, equal);
            }
            let at = self.add(first, eight);
            let tag = self.directory_word(set, at);
            let tag_place = self.binary(op::BITWISE_AND, tag, place_bits);
            let filled = self.compare(op::INOT_EQUAL, tag_place, zero);
            let tag_shape = self.binary(op::BITWISE_AND, tag, cube_tag);
            let shaped = self.compare(op::IEQUAL, tag_shape, shape_tag);
            let hit = self.both(same, filled);
            let hit = self.both(hit, shaped);
            let index = self.binary(op::ISUB, tag_place, one);
            place = self.select(hit, index, place);
            found = self.either(found, hit);
        }
        // Never past the array, whatever a directory holds.
        let last = Model::constant(self, orbistoun_spirv::MOST_SELECTED_IMAGES - 1);
        let below = self.compare(op::ULESS_THAN, place, last);
        let place = self.select(below, place, last);
        let bool_type = self.bool_type;
        let b = &mut self.builder;
        let missing = b.id();
        b.function(op::LOGICAL_NOT, &[bool_type.0, missing.0, found.0]);
        let (pointer, image, sampler, combined) = (b.id(), b.id(), b.id(), b.id());
        b.function(
            op::ACCESS_CHAIN,
            &[array.image_pointer.0, pointer.0, array.images.0, place.0],
        );
        b.function(op::LOAD, &[array.image.0, image.0, pointer.0]);
        b.function(op::LOAD, &[set.sampler.0, sampler.0, texture.variable.0]);
        b.function(
            op::SAMPLED_IMAGE,
            &[texture.sampled.0, combined.0, image.0, sampler.0],
        );
        // The index differs between invocations, so the image reached through it is non-uniform,
        // and so is every value it is read through.
        for value in [place, pointer, image, combined] {
            b.annotate(op::DECORATE, &[value.0, decoration::NON_UNIFORM]);
        }
        (combined, missing)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        SELECTED_CUBE_TAG, SELECTED_DIRECTORY_PROBES, SELECTED_DIRECTORY_SLOT_WORDS,
        selected_directory, selected_image_hash,
    };

    /// Every descriptor lies within the probes of its hash's slot, tagged with its place plus one and
    /// its shape, and a lookup as the module makes it finds each where it was placed.
    #[test]
    fn a_directory_finds_every_descriptor_where_it_was_placed() {
        let descriptors: Vec<([u32; 8], bool)> = (0..300_u32)
            .map(|n| ([n, n * 7, 0x1000, 0, n & 3, 0, 0, 0x9000_0000], n % 5 == 0))
            .collect();
        let directory = selected_directory(&descriptors).expect("a directory");
        let mask = directory[0];
        assert!((mask + 1).is_power_of_two());
        let width = SELECTED_DIRECTORY_SLOT_WORDS as usize;
        for (place, (words, cube)) in descriptors.iter().enumerate() {
            let home = selected_image_hash(words) & mask;
            let found = (0..SELECTED_DIRECTORY_PROBES).find_map(|probe| {
                let at = 1 + ((home + probe) & mask) as usize * width;
                (directory[at..at + 8] == words[..]).then(|| directory[at + 8])
            });
            let tag = found.expect("within the probes");
            assert_eq!(tag & !SELECTED_CUBE_TAG, place as u32 + 1);
            assert_eq!(tag & SELECTED_CUBE_TAG != 0, *cube);
        }
    }

    /// More descriptors than the arrays hold make no directory.
    #[test]
    fn a_directory_past_the_arrays_is_refused() {
        let most = orbistoun_spirv::MOST_SELECTED_IMAGES;
        let descriptors: Vec<([u32; 8], bool)> = (0..=most)
            .map(|n| ([n, 0, 0, 0, 0, 0, 0, 0], false))
            .collect();
        assert!(selected_directory(&descriptors).is_none());
    }
}
