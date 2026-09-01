//! Probing which hashing convention a real module actually uses.
//!
//! A search that finds nothing has two possible causes that look identical: the suffix
//! is wrong, or the digest is being read in the wrong byte order. This separates them.
//!
//! Published standard-library names are the instrument. A module importing dozens of
//! them will match *many* under the right convention and none under a wrong one, so the
//! signal is unmistakable rather than marginal.
//!
//! Run with a container path and a candidate suffix:
//!
//! ```text
//! cargo run -p orbistoun-names --example probe -- <container> <suffix-hex>
//! ```

use std::collections::HashSet;

use orbistoun_nid::Nid;
use sha1::{Digest, Sha1};

/// How the first eight digest bytes become a number.
#[derive(Debug, Clone, Copy)]
enum ByteOrder {
    /// Least significant byte first.
    Little,
    /// Most significant byte first.
    Big,
}

/// Where the suffix goes relative to the name.
#[derive(Debug, Clone, Copy)]
enum Placement {
    /// Name then suffix.
    After,
    /// Suffix then name.
    Before,
}

fn hash(name: &str, suffix: &[u8], order: ByteOrder, placement: Placement) -> u64 {
    let mut h = Sha1::new();
    match placement {
        Placement::After => {
            h.update(name.as_bytes());
            h.update(suffix);
        }
        Placement::Before => {
            h.update(suffix);
            h.update(name.as_bytes());
        }
    }
    let digest = h.finalize();
    let mut bytes = [0_u8; 8];
    bytes.copy_from_slice(&digest[..8]);
    match order {
        ByteOrder::Little => u64::from_le_bytes(bytes),
        ByteOrder::Big => u64::from_be_bytes(bytes),
    }
}

fn main() {
    let mut args = std::env::args().skip(1);
    let path = args.next().expect("usage: probe <container> <suffix-hex>");
    let suffix_hex = args.next().unwrap_or_default();

    let suffix: Vec<u8> = (0..suffix_hex.len() / 2)
        .map(|i| u8::from_str_radix(&suffix_hex[i * 2..i * 2 + 2], 16).expect("hex"))
        .collect();

    let bytes = std::fs::read(&path).expect("read the container");
    let container = orbistoun_elf::Container::parse(&bytes).expect("parse");
    let hasher = orbistoun_nid::NidHasher::new(suffix.clone());
    let imports = container.raw_imports(&bytes, &hasher).expect("imports");

    // Kept apart, not merged: the whole question is which of the two the decoder should
    // be producing, and a combined set answers "one of them" - which is what sent this
    // investigation the wrong way once already.
    let as_decoded: HashSet<u64> = imports.iter().map(|i| i.nid).collect();
    let swapped: HashSet<u64> = imports.iter().map(|i| i.nid.swap_bytes()).collect();

    println!("{} imports", imports.len());
    println!("suffix: {} bytes", suffix.len());

    let names = orbistoun_names::standard_names();
    for order in [ByteOrder::Little, ByteOrder::Big] {
        for placement in [Placement::After, Placement::Before] {
            let direct = names
                .iter()
                .filter(|n| as_decoded.contains(&hash(n, &suffix, order, placement)))
                .count();
            let needs_swap = names
                .iter()
                .filter(|n| swapped.contains(&hash(n, &suffix, order, placement)))
                .count();
            println!(
                "  {order:?} / {placement:?}: {direct} match as decoded, {needs_swap} match swapped"
            );
        }
    }

    // What the decoder currently believes, for comparison.
    let first = imports.first().expect("at least one import");
    println!(
        "\nfirst import: encoded {:?} -> {}",
        first.name,
        Nid::from_raw(first.nid)
    );
}
