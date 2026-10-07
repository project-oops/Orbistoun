//! `sceKernelAprResolveFilepathsWithPrefixToIdsAndFileSizes` resolves each path joined to its
//! prefix, as the plain form resolves a whole path: measured to answer identically (obSCEne -ap02,
//! arms 1 and 2).

use orbistoun_core::GUEST_ARG_REGISTERS;

fn call(name: &str, args: [u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (_, handler) = orbistoun_kernel::implementations()
        .iter()
        .find(|(n, _)| *n == name)
        .unwrap_or_else(|| panic!("{name} is implemented"));
    handler(&args)
}

/// An index that names one file, as a title's own index does (D591).
fn index(path: &str) -> Option<(u64, u64)> {
    (path == "/app0/data/level0").then_some((7, 0x1234))
}

#[test]
fn a_prefixed_path_resolves_as_its_whole_path() {
    orbistoun_kernel::apr::on_index_lookup(index);
    let prefix = c"/app0/";
    let (named, missing) = (c"data/level0", c"data/absent");
    let paths = [named.as_ptr() as u64, missing.as_ptr() as u64];
    let mut ids = [0xa1a1_a1a1_u32; 2];
    let mut sizes = [0xa1a1_a1a1_a1a1_a1a1_u64; 2];
    let mut statuses = [0xa1a1_a1a1_u32; 2];
    let answer = call(
        "sceKernelAprResolveFilepathsWithPrefixToIdsAndFileSizes",
        [
            prefix.as_ptr() as u64,
            paths.as_ptr() as u64,
            2,
            ids.as_mut_ptr() as u64,
            sizes.as_mut_ptr() as u64,
            statuses.as_mut_ptr() as u64,
        ],
    );
    assert_eq!(
        (ids[0], sizes[0], statuses[0]),
        (7, 0x1234, 0),
        "the indexed file"
    );
    assert_eq!(
        (ids[1], sizes[1], statuses[1]),
        (u32::MAX, 0, 0),
        "a path the index does not name"
    );
    assert_eq!(
        answer,
        u64::from(u32::MAX),
        "-1, as for any unresolved path"
    );

    // An empty prefix, as PPSA04263 passes, leaves the path whole.
    let whole = c"/app0/data/level0";
    let one = [whole.as_ptr() as u64];
    let answer = call(
        "sceKernelAprResolveFilepathsWithPrefixToIdsAndFileSizes",
        [
            c"".as_ptr() as u64,
            one.as_ptr() as u64,
            1,
            ids.as_mut_ptr() as u64,
            sizes.as_mut_ptr() as u64,
            statuses.as_mut_ptr() as u64,
        ],
    );
    assert_eq!((answer, ids[0], sizes[0]), (0, 7, 0x1234));
}
