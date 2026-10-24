//! Says when the driver holds the device thread compiling one pipeline, and keeps the module.
//!
//! A module the driver's compiler takes minutes over stalls a run with the device idle and nothing
//! in the log: Grand Theft Auto V's first in-world frames sat five minutes inside the driver's
//! compiler before a debugger said so. A watcher looks every few seconds; a compile running past
//! [`OVERDUE`] is logged once with its SPIR-V written where the run keeps its frames, and a slow
//! one that does finish says how long it took.

use std::fmt::Write as _;
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

/// How long one compile runs before the watcher reports it and keeps its module.
const OVERDUE: Duration = Duration::from_secs(30);
/// How long a finished compile took before it is worth a line.
const SLOW: Duration = Duration::from_secs(2);
/// How often the watcher looks.
const LOOK_EVERY: Duration = Duration::from_secs(5);

/// The compile under way.
struct Compiling {
    /// What it is a pipeline for.
    what: String,
    /// The modules it was handed.
    modules: Vec<Vec<u32>>,
    started: Instant,
    /// Whether the watcher has reported it.
    reported: bool,
}

static NOW: Mutex<Option<Compiling>> = Mutex::new(None);

fn now() -> std::sync::MutexGuard<'static, Option<Compiling>> {
    NOW.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Runs `compile`, the driver's compile of a pipeline for `what` from `modules`, under the watch.
pub(crate) fn compiling<T>(what: String, modules: &[&[u32]], compile: impl FnOnce() -> T) -> T {
    start_watcher();
    let started = Instant::now();
    *now() = Some(Compiling {
        what,
        modules: modules.iter().map(|words| words.to_vec()).collect(),
        started,
        reported: false,
    });
    let out = compile();
    let ended = now().take();
    let took = started.elapsed();
    if let Some(ended) = ended
        && took >= SLOW
    {
        tracing::info!(
            "the driver took {:.1} s to compile {} ({} words of SPIR-V)",
            took.as_secs_f64(),
            ended.what,
            words(&ended.modules)
        );
    }
    out
}

/// Every module's words, together.
fn words(modules: &[Vec<u32>]) -> usize {
    modules.iter().map(Vec::len).sum()
}

/// Starts the watcher, once.
fn start_watcher() {
    static STARTED: std::sync::Once = std::sync::Once::new();
    STARTED.call_once(|| {
        let _ = std::thread::Builder::new()
            .name("orbistoun-compile-watch".into())
            .spawn(|| {
                loop {
                    std::thread::sleep(LOOK_EVERY);
                    look(Instant::now());
                }
            });
    });
}

/// Reports the compile under way, once, if it has run past [`OVERDUE`] by `at`.
fn look(at: Instant) {
    let mut now = now();
    let Some(compiling) = now.as_mut() else {
        return;
    };
    if !overdue(compiling, at) {
        return;
    }
    compiling.reported = true;
    let kept = keep(&compiling.modules);
    tracing::warn!(
        "the driver has been compiling {} for {} s ({} words of SPIR-V){}",
        compiling.what,
        at.duration_since(compiling.started).as_secs(),
        words(&compiling.modules),
        kept
    );
}

/// Whether `compiling` has run past [`OVERDUE`] by `at` and is not yet reported.
fn overdue(compiling: &Compiling, at: Instant) -> bool {
    !compiling.reported && at.duration_since(compiling.started) >= OVERDUE
}

/// Writes each module to `slow-<hash>.spv` where the run keeps its frames, and says where.
fn keep(modules: &[Vec<u32>]) -> String {
    let dir = orbistoun_env::FRAME_DIR
        .get()
        .map_or_else(std::env::temp_dir, std::path::PathBuf::from);
    let mut said = String::new();
    for module in modules {
        let path = dir.join(file_name(module));
        let bytes: Vec<u8> = module.iter().flat_map(|w| w.to_le_bytes()).collect();
        let _ = match std::fs::write(&path, bytes) {
            Ok(()) => write!(said, "; kept at {}", path.display()),
            Err(e) => write!(said, "; not kept: {e}"),
        };
    }
    said
}

/// The name a module is kept under: its FNV-1a hash, the same each run for the same words.
fn file_name(module: &[u32]) -> String {
    let hash = module
        .iter()
        .flat_map(|w| w.to_le_bytes())
        .fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
        });
    format!("slow-{hash:016x}.spv")
}

#[cfg(test)]
mod tests {
    use super::{Compiling, OVERDUE, overdue};
    use std::time::Instant;

    /// A compile is overdue once it has run past the bound, and only until it is reported.
    #[test]
    fn a_compile_is_reported_once_past_the_bound() {
        let started = Instant::now();
        let mut compiling = Compiling {
            what: "a test pipeline".into(),
            modules: vec![vec![0x0723_0203]],
            started,
            reported: false,
        };
        assert!(!overdue(&compiling, started));
        assert!(overdue(&compiling, started + OVERDUE));
        compiling.reported = true;
        assert!(!overdue(&compiling, started + OVERDUE * 2));
    }

    /// The same words are kept under the same name, and other words under another.
    #[test]
    fn a_module_s_name_follows_its_words() {
        let a = super::file_name(&[1, 2, 3]);
        assert_eq!(a, super::file_name(&[1, 2, 3]));
        assert_ne!(a, super::file_name(&[1, 2, 4]));
        assert_eq!(a.len(), "slow-".len() + 16 + ".spv".len());
        assert!(a.starts_with("slow-"));
    }

    /// What the compile answers is what the watch answers.
    #[test]
    fn the_watch_answers_the_compile_s_result() {
        assert_eq!(super::compiling("a test pipeline".into(), &[&[1]], || 7), 7);
    }
}
