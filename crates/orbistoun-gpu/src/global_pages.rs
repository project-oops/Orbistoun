//! A dispatch's global buffer holds the guest pages its waves' global loads and stores reach, each
//! whole, behind a hash table of each page's number and where its words start (D961), laid out as
//! [`orbistoun_translate::wavefront::GLOBAL_PAGE_BYTES`] says, and what the stores changed is
//! written back page by page. Pointers a program follows can lie gigabytes apart, and the table is
//! sized by the pages held, not the span between them.

use std::collections::BTreeSet;

/// The bytes of one page.
pub const PAGE: u64 = orbistoun_translate::wavefront::GLOBAL_PAGE_BYTES as u64;

/// The words of one page.
const PAGE_WORDS: usize = (PAGE / 4) as usize;

/// The largest table, as a power of two: 2^24 slots of three words, 192 MiB, which no set of pages
/// the buffer may hold needs (D961).
const MOST_TABLE_BITS: u32 = 24;

/// A dispatch's global loads' buffer: its slot, the page its table counts from, and the pages it
/// holds, ascending.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlobalPages {
    /// The slot it is bound at.
    pub slot: usize,
    /// The first page's guest address, which the push block carries.
    pub base: u64,
    /// Every page it holds, by guest address, ascending: `base` first.
    pub pages: Vec<u64>,
}

impl GlobalPages {
    /// The pages `spans` - `(address, bytes)` - reach, as one buffer at `slot`; `None` where they
    /// reach none.
    #[must_use]
    pub fn of(slot: usize, spans: impl IntoIterator<Item = (u64, u64)>) -> Option<Self> {
        let mut pages = BTreeSet::new();
        for (address, bytes) in spans {
            if bytes == 0 {
                continue;
            }
            let last = address.saturating_add(bytes - 1) & !(PAGE - 1);
            let mut page = address & !(PAGE - 1);
            while page <= last {
                pages.insert(page);
                page += PAGE;
            }
        }
        let pages: Vec<u64> = pages.into_iter().collect();
        Some(Self {
            slot,
            base: *pages.first()?,
            pages,
        })
    }

    /// The guest span from the first page's start to the last page's end.
    #[must_use]
    pub fn span(&self) -> (u64, u64) {
        let end = self.pages.last().map_or(self.base, |&last| last + PAGE);
        (self.base, end - self.base)
    }

    /// How many words the buffer takes - its table's size, the table, and the pages - or `None`
    /// where no table up to `MOST_TABLE_BITS` places every page within its probes.
    #[must_use]
    pub fn words(&self) -> Option<u64> {
        let bits = self.table_bits()?;
        Some(1 + 3 * (1_u64 << bits) + self.pages.len() as u64 * PAGE_WORDS as u64)
    }

    /// The table's size as a power of two: the least, of at least four slots a page, that places
    /// every page within [`orbistoun_translate::wavefront::GLOBAL_PAGE_PROBES`] slots of its own.
    fn table_bits(&self) -> Option<u32> {
        let least = (self.pages.len().max(1) * 4)
            .next_power_of_two()
            .trailing_zeros();
        (least.max(1)..=MOST_TABLE_BITS).find(|&bits| self.placed(bits).is_some())
    }

    /// Each page's slot in a table of `1 << bits` slots, by linear probing in page order; `None`
    /// where a page finds no empty slot within its probes.
    fn placed(&self, bits: u32) -> Option<Vec<usize>> {
        use orbistoun_translate::wavefront::{GLOBAL_PAGE_PROBES, global_page_slot};
        let size = 1_usize << bits;
        let mut taken = vec![false; size];
        let mut slots = Vec::with_capacity(self.pages.len());
        for &page in &self.pages {
            let home = global_page_slot(page / PAGE, bits) as usize;
            let slot = (0..GLOBAL_PAGE_PROBES as usize)
                .map(|probe| (home + probe) & (size - 1))
                .find(|&slot| !taken[slot])?;
            taken[slot] = true;
            slots.push(slot);
        }
        Some(slots)
    }

    /// Whether a page the buffer holds shares a byte with `range`, `(address, bytes)`.
    #[must_use]
    pub fn holds_any_of(&self, range: (u64, u64)) -> bool {
        self.pages
            .iter()
            .any(|&page| crate::pipeline::overlaps((page, PAGE), range))
    }

    /// The buffer's words, each page as `read` answers it; a page `read` cannot answer is not held,
    /// and its slot is left empty. Empty where no table places every page ([`Self::words`]).
    pub fn buffer_words(&self, mut read: impl FnMut(u64) -> Option<Vec<u32>>) -> Vec<u32> {
        let Some(bits) = self.table_bits() else {
            return Vec::new();
        };
        let Some(slots) = self.placed(bits) else {
            return Vec::new();
        };
        let mut words = vec![0_u32; 1 + 3 * (1_usize << bits)];
        words[0] = bits;
        for (&page, &slot) in self.pages.iter().zip(&slots) {
            let Some(mut held) = read(page) else {
                continue;
            };
            held.resize(PAGE_WORDS, 0);
            let Ok(start) = u32::try_from(words.len()) else {
                continue;
            };
            let number = page / PAGE;
            let at = 1 + 3 * slot;
            words[at..at + 3].copy_from_slice(&[number as u32, (number >> 32) as u32, start]);
            words.extend_from_slice(&held);
        }
        words
    }

    /// Each run of words the dispatch's global stores changed (D961), from the buffer's words
    /// `before` and `after` it ran, as the guest address it starts at and its words: each page and
    /// where its words lie are read from the table `before` holds, and a run never crosses a page's
    /// end.
    #[must_use]
    pub fn changed_runs<'a>(&self, (before, after): (&[u32], &'a [u32])) -> Vec<(u64, &'a [u32])> {
        let slots = before
            .first()
            .map_or(0, |&bits| 1_usize << bits.min(MOST_TABLE_BITS));
        let mut runs = Vec::new();
        for slot in 0..slots {
            let Some(&[low, high, start]) = before.get(1 + 3 * slot..4 + 3 * slot) else {
                continue;
            };
            let start = start as usize;
            let (Some(was), Some(now)) = (
                before.get(start..start + PAGE_WORDS).filter(|_| start != 0),
                after.get(start..start + PAGE_WORDS),
            ) else {
                continue;
            };
            let address = (u64::from(high) << 32 | u64::from(low)) * PAGE;
            let mut word = 0;
            while word < PAGE_WORDS {
                if was[word] == now[word] {
                    word += 1;
                    continue;
                }
                let first = word;
                while word < PAGE_WORDS && was[word] != now[word] {
                    word += 1;
                }
                runs.push((address + first as u64 * 4, &now[first..word]));
            }
        }
        runs.sort_by_key(|&(address, _)| address);
        runs
    }
}

/// The ranges a dispatch's buffers hold, by slot, `buffers`, as what may share no byte with its
/// window: each buffer's range, but for the global loads' buffer, `global`, each page it holds.
#[must_use]
pub fn bound_ranges(buffers: &[(u64, u64)], global: Option<&GlobalPages>) -> Vec<(u64, u64)> {
    let mut bound: Vec<(u64, u64)> = buffers
        .iter()
        .enumerate()
        .filter(|&(slot, _)| global.is_none_or(|global| global.slot != slot))
        .map(|(_, &range)| range)
        .collect();
    if let Some(global) = global {
        bound.extend(global.pages.iter().map(|&page| (page, PAGE)));
    }
    bound
}

#[cfg(test)]
mod tests {
    use super::{GlobalPages, PAGE};

    /// Two loads a gigabyte apart hold two pages, a load across a page's end holds both its
    /// pages, and the table spans from the first page to the last: what binds is what is read.
    #[test]
    fn the_pages_held_are_those_the_loads_reach() {
        let far = 0x7400_0000_0000_u64 + (1 << 30);
        let found = GlobalPages::of(
            3,
            [
                (0x7400_0000_1ffc, 8),
                (far + 0x18, 4),
                (0x7400_0000_1000, 4),
                (0x7400_0000_5000, 0),
            ],
        )
        .expect("pages");
        assert_eq!(found.base, 0x7400_0000_1000);
        assert_eq!(found.pages, [0x7400_0000_1000, 0x7400_0000_2000, far]);
        assert_eq!(
            found.span(),
            (0x7400_0000_1000, far + PAGE - 0x7400_0000_1000)
        );
        // Three pages: a table of sixteen slots, three words each.
        assert_eq!(found.words(), Some(1 + 3 * 16 + 3 * 1024));
        assert!(found.holds_any_of((far + 0xffc, 8)));
        assert!(!found.holds_any_of((0x7400_0000_3000, 0x1000)));
        assert_eq!(GlobalPages::of(0, [(0x1000, 0)]), None, "nothing read");
    }

    /// Where the word at `address` lies in `words`, as the module looks it up: the page's number
    /// hashed to its slot, and the probes after it, for a slot holding that number.
    fn lookup(words: &[u32], address: u64) -> Option<usize> {
        use orbistoun_translate::wavefront::{GLOBAL_PAGE_PROBES, global_page_slot};
        let bits = words[0];
        let number = address / PAGE;
        let home = global_page_slot(number, bits);
        (0..GLOBAL_PAGE_PROBES).find_map(|probe| {
            let slot = ((home + probe) & ((1 << bits) - 1)) as usize;
            let [low, high, start] = words[1 + 3 * slot..4 + 3 * slot] else {
                return None;
            };
            (start != 0 && low == number as u32 && high == (number >> 32) as u32)
                .then(|| start as usize + (address % PAGE / 4) as usize)
        })
    }

    /// Pages gigabytes apart - eighteen of them between the first and the last, as Grand Theft
    /// Auto V's program at 0x74005b87e400 reads - bind only themselves: the table is sized by the
    /// pages held, not the span, and every page is found by its full number. A page the read
    /// cannot answer, and a page not held, are not found.
    #[test]
    fn pages_gigabytes_apart_are_found_by_their_number() {
        let base = 0x10_a60c_8000_u64;
        let mut addresses: Vec<u64> = (0..363)
            .map(|i| base + i * (49 << 20) + 0x123 * 4)
            .collect();
        addresses.push(base + 0x4_3237_6000);
        let found = GlobalPages::of(0, addresses.iter().map(|&a| (a, 4))).expect("pages");
        assert_eq!(found.pages.len(), 364);
        assert!(
            found.span().1 > 16 << 30,
            "a span past what a dense table indexes"
        );
        let words = found.buffer_words(|page| {
            (page != base + 7 * (49 << 20))
                .then(|| (0..1024).map(|w| (page >> 12) as u32 ^ w).collect())
        });
        assert_eq!(
            Some(words.len() as u64 + 1024),
            found.words(),
            "every page read but one"
        );
        for &address in &addresses {
            let page = address & !(PAGE - 1);
            let at = lookup(&words, address);
            if page == base + 7 * (49 << 20) {
                assert_eq!(at, None, "the page not read is not held");
            } else {
                let at = at.expect("held");
                let word = (address % PAGE / 4) as u32;
                assert_eq!(words[at], (page >> 12) as u32 ^ word, "{address:#x}");
            }
        }
        assert_eq!(
            lookup(&words, base + 0x1000),
            None,
            "a page between, not held"
        );
        assert_eq!(
            lookup(&words, base + (1 << 44)),
            None,
            "the same low bits, another span"
        );
    }

    /// What the global stores changed is written back to the pages they changed, each run at its
    /// page's guest address, found through the table: a page's words do not run into the next's,
    /// though the pages lie gigabytes apart.
    #[test]
    fn changed_words_go_back_to_their_pages() {
        let far = 0x3000 + (20 << 30);
        let found = GlobalPages::of(0, [(0x1000, 4), (far, 4)]).expect("pages");
        let before = found.buffer_words(|_| Some(vec![0; 1024]));
        let mut after = before.clone();
        // Page 0x1000's last two words, the far page's first.
        let low = lookup(&before, 0x1000).expect("held");
        let high = lookup(&before, far).expect("held");
        after[low + 1022] = 7;
        after[low + 1023] = 8;
        after[high] = 9;
        let runs = found.changed_runs((&before, &after));
        assert_eq!(runs, [(0x1ff8, &[7_u32, 8][..]), (far, &[9_u32][..])]);
    }
}
