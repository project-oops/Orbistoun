//! Every committed title's pad script reads and validates, the way a run reads it (D721, D987): a
//! script that would end a long run at its first step is caught here instead.

#[test]
fn every_committed_pad_script_is_well_formed() {
    let compat = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../compat");
    let mut read = 0;
    for entry in std::fs::read_dir(&compat).expect("the compat records are beside the crates") {
        let path = entry.expect("a record").path().join("inputs.toml");
        if !path.is_file() {
            continue;
        }
        if let Err(why) = orbistoun_service::read_pad_script(&path) {
            panic!("{}: {why}", path.display());
        }
        read += 1;
    }
    assert!(read > 0, "at least one title replays a script");
}
