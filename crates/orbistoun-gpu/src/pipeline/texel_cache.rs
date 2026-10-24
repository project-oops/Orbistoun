//! Texels read from guest memory, kept by where they lie and their shape, the oldest let go past a
//! budget of bytes.
//!
//! A frame of a large world samples hundreds of distinct textures. Kept by count and let go whole
//! when full, a cache smaller than that set is emptied several times a frame, and every texture it
//! held - the frame's unchanged ones included - is read and detiled again the next time it is
//! bound. Kept by bytes and let go oldest first, a set that fits stays.

use std::collections::{HashMap, VecDeque};

use super::CachedTexels;

/// Where a texture's texels lie and their shape: base address, width, height, and a tag for how
/// they were read.
pub(super) type TexelKey = (u64, u32, u32, u32);

/// How many bytes of texels the cache keeps: a frame's textures beside a few 4K targets, and a
/// bound for a guest streaming new textures every frame. Every byte is a second copy of guest
/// memory the device holds a third of, so it is held to a share of the host's overhead budget.
const TEXEL_CACHE_BYTES: usize = 256 << 20;

/// How many textures the cache keeps, however small: a bound on its bookkeeping.
const TEXEL_CACHE_TEXTURES: usize = 16_384;

/// See the module documentation.
#[derive(Debug)]
pub(super) struct TexelCache {
    held: HashMap<TexelKey, CachedTexels>,
    /// The keys held, oldest kept first.
    order: VecDeque<TexelKey>,
    bytes: usize,
    /// The most bytes and textures it keeps.
    limits: (usize, usize),
}

impl TexelCache {
    pub(super) fn new() -> Self {
        Self::within((TEXEL_CACHE_BYTES, TEXEL_CACHE_TEXTURES))
    }

    fn within(limits: (usize, usize)) -> Self {
        Self {
            held: HashMap::new(),
            order: VecDeque::new(),
            bytes: 0,
            limits,
        }
    }

    /// The bytes of texels kept.
    pub(super) const fn bytes(&self) -> usize {
        self.bytes
    }

    /// The texels kept under `key`.
    pub(super) fn get(&self, key: &TexelKey) -> Option<&CachedTexels> {
        self.held.get(key)
    }

    /// Keeps `texels` under `key`, newest, in place of what it held; the oldest are let go while
    /// the cache is over its budget, though never the one just kept.
    pub(super) fn keep(&mut self, key: TexelKey, texels: CachedTexels) {
        self.bytes += bytes_of(&texels);
        if let Some(old) = self.held.insert(key, texels) {
            self.bytes -= bytes_of(&old);
            self.order.retain(|held| *held != key);
        }
        self.order.push_back(key);
        let (most_bytes, most_textures) = self.limits;
        while (self.bytes > most_bytes || self.order.len() > most_textures) && self.order.len() > 1
        {
            let Some(oldest) = self.order.pop_front() else {
                break;
            };
            if let Some(gone) = self.held.remove(&oldest) {
                self.bytes -= bytes_of(&gone);
            }
            crate::perf::span(crate::perf::Span::TexturesEmptied, || ());
        }
    }
}

fn bytes_of(texels: &CachedTexels) -> usize {
    texels.texels.len() * 4
}

#[cfg(test)]
mod tests {
    use super::{CachedTexels, TexelCache};

    fn texels(words: usize) -> CachedTexels {
        CachedTexels {
            since: None,
            hash: words as u64,
            texels: vec![0u32; words].into(),
        }
    }

    /// A thousand textures, the set a large world's frame samples, all stay while they fit its
    /// bytes, and a texture kept again replaces itself rather than counting twice.
    #[test]
    fn every_texture_that_fits_the_budget_stays() {
        let mut cache = TexelCache::new();
        for base in 0..1000u64 {
            cache.keep((base * 0x1000, 16, 16, 0), texels(256));
        }
        cache.keep((0, 16, 16, 0), texels(256));
        assert!((0..1000u64).all(|base| cache.get(&(base * 0x1000, 16, 16, 0)).is_some()));
        assert_eq!(cache.order.len(), 1000);
        assert_eq!(cache.bytes, 1000 * 256 * 4);
    }

    /// Past the budget the oldest go first, the one just kept stays, and the bytes held are what
    /// is held.
    #[test]
    fn past_the_budget_the_oldest_texture_goes() {
        let quarter = 64;
        let mut cache = TexelCache::within((quarter * 4 * 4, 100));
        for base in 0..4u64 {
            cache.keep((base, 1, 1, 0), texels(quarter));
        }
        assert!(cache.get(&(0, 1, 1, 0)).is_some(), "four quarters fit");
        cache.keep((4, 1, 1, 0), texels(quarter));
        assert!(cache.get(&(0, 1, 1, 0)).is_none(), "the oldest went");
        assert!((1..5u64).all(|base| cache.get(&(base, 1, 1, 0)).is_some()));
        cache.keep((5, 1, 1, 0), texels(quarter * 8));
        assert!(
            cache.get(&(5, 1, 1, 0)).is_some(),
            "larger than the budget, kept alone"
        );
        assert_eq!(cache.order.len(), 1);
        assert_eq!(cache.bytes, quarter * 8 * 4);
    }
}
