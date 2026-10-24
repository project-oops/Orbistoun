//! Runs a captured guest dispatch again alone: `replay-dispatch`.

use std::path::Path;

use anyhow::{Result, bail};

/// Replays the dispatch captured in `dir` `repeat` times, prints each run, and writes what the last
/// left into `out` where it is given; fails where a run lost the device, so a script can tell.
pub(crate) fn cmd_replay_dispatch(dir: &Path, repeat: u32, out: Option<&Path>) -> Result<()> {
    let replay = orbistoun_gpu_vulkan::replay_dispatch(dir, repeat, out)?;
    print!("{replay}");
    if replay.lost {
        bail!("the dispatch lost the device");
    }
    Ok(())
}
