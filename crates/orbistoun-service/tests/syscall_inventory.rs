//! The link plan's raw `syscall` inventory, against titles in the library (D724).
//!
//! Titles cannot be in this repository, so this reads the operator's library and is opt-in.

/// Each of these ships exactly one raw `syscall` in its executable segments.
const ONE_SITE_EACH: [&str; 2] = ["PPSA99980", "obscene-probe-prospero-native"];

#[test]
#[ignore = "reads titles from the operator's library; opt-in via --ignored"]
fn titles_with_one_raw_syscall_list_one_site() {
    let titles = orbistoun_paths::Paths::resolve().titles_dir();
    for title in ONE_SITE_EACH {
        let path = titles.join(title).join("eboot.bin");
        let bytes = std::fs::read(&path)
            .unwrap_or_else(|e| panic!("{title} is not in the library at {}: {e}", path.display()));
        let sites = orbistoun_loader::inventory::syscall_sites(&bytes, 0x4000_0000_0000)
            .expect("the executable parses");
        assert_eq!(sites.len(), 1, "{title}: {sites:#x?}");
    }
}
