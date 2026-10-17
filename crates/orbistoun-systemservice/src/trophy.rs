//! `libSceNpTrophy2` - trophies, kept on the console for whoever is signed in.
//!
//! Trophies are a local feature: a console with no network account still keeps them, and earns
//! them, and syncs them only when it next goes online. So a title's trophy set is answered here from
//! its own trophy pack, `sce_sys/trophy2/trophyNN.ucp` - a plain archive of `tropconf.json` (the
//! set: each trophy's grade, group, platinum and unlock condition), `tropmeta_<language>.json` (the
//! names) and the icons - and what the user has earned is kept beside the title's save data, in
//! `/data/sce_trophy2`, so a later run finds it.
//!
//! Assumed, not measured: the call shapes, the structure layouts (the 0x520-byte trophy details,
//! a guest's own array stride, agrees) and the error codes are those the reference emulators give an
//! offline console; no obSCEne probe has reached this library. A hardware census replaces any of
//! them that disagrees.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use orbistoun_core::{GUEST_ARG_REGISTERS, GuestFn};
use orbistoun_hle::guest_module;
use orbistoun_mem::guest;

use crate::uds_stats::{Comparator, Event, Rules};

guest_module! {
    "libSceNpTrophy2" {
        // (context *, user, service label, options)
        "sceNpTrophy2CreateContext" => 4,
        // (handle *)
        "sceNpTrophy2CreateHandle" => 1,
        // (context, handle, options)
        "sceNpTrophy2RegisterContext" => 3,
        // (context, handle, details *, data *)
        "sceNpTrophy2GetGameInfo" => 4,
        // (context, handle, trophy, details *, data *)
        "sceNpTrophy2GetTrophyInfo" => 5,
        // (context, handle, offset, limit, details *, data *, count *)
        "sceNpTrophy2GetTrophyInfoArray" => 6,
        // (callback, user data)
        "sceNpTrophy2RegisterUnlockCallback" => 2,
        // (handle)
        "sceNpTrophy2AbortHandle" => 1,
        // (handle)
        "sceNpTrophy2DestroyHandle" => 1,
        // (context)
        "sceNpTrophy2DestroyContext" => 1,
    }
}

/// An argument the call refuses: a null destination, or options it does not take.
const INVALID_ARGUMENT: u64 = 0x8055_3904;
/// A context no `sceNpTrophy2CreateContext` made, or one destroyed.
const INVALID_CONTEXT: u64 = 0x8055_3909;
/// A trophy the set does not hold.
const INVALID_TROPHY_ID: u64 = 0x8055_390a;
/// A second context for the same user and trophy set.
const CONTEXT_ALREADY_EXISTS: u64 = 0x8055_3910;
/// A context past the most one title may hold.
const CONTEXT_EXCEEDS_MAX: u64 = 0x8055_391b;
/// A title with no trophy pack for the service label it named.
const TITLE_CONF_NOT_INSTALLED: u64 = 0x8055_391e;
/// A query on a context not yet registered.
const NOT_REGISTERED: u64 = 0x8055_3920;
/// A second registration of one context.
const ALREADY_REGISTERED: u64 = 0x8055_3921;

/// The most contexts one title may hold.
const MOST_CONTEXTS: usize = 8;

/// Bytes of `SceNpTrophy2GameDetails`: six counts and a 128-byte title.
const GAME_DETAILS_BYTES: usize = 152;
/// Bytes of `SceNpTrophy2GameData`: five unlocked counts and the percentage.
const GAME_DATA_BYTES: usize = 24;
/// Bytes of `SceNpTrophy2Details`: identity, grade, group, flags, target, then the name (128),
/// description (1024) and reward (128) strings.
const TROPHY_DETAILS_BYTES: usize = 0x520;
/// Bytes of `SceNpTrophy2Data`: the trophy, whether it is unlocked, its progress and when.
const TROPHY_DATA_BYTES: usize = 32;

/// The group a trophy outside every named group belongs to: the base game.
pub const BASE_GROUP: i32 = -1;

/// A trophy's grade, as the details report it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Grade {
    /// The trophy for earning every other in its set.
    Platinum = 1,
    /// Gold.
    Gold = 2,
    /// Silver.
    Silver = 3,
    /// Bronze.
    Bronze = 4,
}

impl Grade {
    /// The grade `tropconf.json` spells as one letter.
    fn from_letter(letter: &str) -> Option<Self> {
        match letter {
            "P" => Some(Self::Platinum),
            "G" => Some(Self::Gold),
            "S" => Some(Self::Silver),
            "B" => Some(Self::Bronze),
            _ => None,
        }
    }
}

/// One trophy of a set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Trophy {
    /// Its identifier, as the title names it.
    pub id: i32,
    /// Its grade.
    pub grade: Grade,
    /// The group it belongs to, [`BASE_GROUP`] for the base game.
    pub group: i32,
    /// The platinum earning it counts towards, if any.
    pub platinum: Option<i32>,
    /// Whether its name stays hidden until it is earned.
    pub hidden: bool,
    /// Whether earning it brings a reward.
    pub has_reward: bool,
    /// The value a progressive trophy counts up to.
    pub target: Option<u64>,
    /// The universal-data statistic whose value earns it, if one does.
    pub unlock: Option<Unlock>,
    /// Its name, in the set's language.
    pub name: String,
    /// What earns it.
    pub detail: String,
    /// The reward it brings.
    pub reward: String,
}

/// What earns a trophy: a universal-data statistic meeting a target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Unlock {
    /// The statistic.
    pub stat: u64,
    /// How it compares to the target.
    pub comparator: Comparator,
    /// The target.
    pub target: i64,
}

/// A title's trophy set, read from its trophy pack.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Pack {
    /// The communication identifier the set belongs to, `NPWRnnnnn_nn`.
    pub comm_id: String,
    /// The title's name.
    pub title: String,
    /// Each named group, by identifier, with its name.
    pub groups: BTreeMap<i32, String>,
    /// Each trophy, by identifier.
    pub trophies: BTreeMap<i32, Trophy>,
}

/// Bytes before a trophy pack's entry table.
const PACK_HEADER_BYTES: usize = 0x60;
/// A trophy pack's first word, big-endian.
const PACK_MAGIC: u32 = 0xb228_c60a;

/// The bytes of the entry named `name` in `pack`, a trophy pack: a big-endian header (magic,
/// version, total size, entry count at 0x10, entry size at 0x14), then from 0x60 one entry each of
/// a 32-byte name, a 64-bit offset and a 64-bit size.
#[must_use]
pub fn pack_entry<'a>(pack: &'a [u8], name: &str) -> Option<&'a [u8]> {
    let word = |at: usize| -> Option<u32> {
        Some(u32::from_be_bytes(pack.get(at..at + 4)?.try_into().ok()?))
    };
    let long = |at: usize| -> Option<u64> {
        Some(u64::from_be_bytes(pack.get(at..at + 8)?.try_into().ok()?))
    };
    if word(0)? != PACK_MAGIC {
        return None;
    }
    let count = usize::try_from(word(0x10)?).ok()?;
    let size = usize::try_from(word(0x14)?).ok()?;
    if size < 0x30 {
        return None;
    }
    (0..count).find_map(|index| {
        let at = PACK_HEADER_BYTES.checked_add(index.checked_mul(size)?)?;
        let field = pack.get(at..at + 0x20)?;
        let len = field.iter().position(|&b| b == 0).unwrap_or(field.len());
        if &field[..len] != name.as_bytes() {
            return None;
        }
        let offset = usize::try_from(long(at + 0x20)?).ok()?;
        let bytes = usize::try_from(long(at + 0x28)?).ok()?;
        pack.get(offset..offset.checked_add(bytes)?)
    })
}

/// An identifier `tropconf.json` spells as a decimal string, `"0007"`.
fn read_id(value: &serde_json::Value) -> Option<i32> {
    match value {
        serde_json::Value::String(text) => text.parse().ok(),
        serde_json::Value::Number(number) => i32::try_from(number.as_i64()?).ok(),
        _ => None,
    }
}

/// An unsigned value `tropconf.json` spells as a decimal string or a number.
fn read_unsigned(value: &serde_json::Value) -> Option<u64> {
    match value {
        serde_json::Value::String(text) => text.parse().ok(),
        serde_json::Value::Number(number) => number.as_u64(),
        _ => None,
    }
}

/// The trophy set in `pack`, with its names in the set's own default language, or `None` when the
/// pack holds no readable set.
#[must_use]
pub fn read_pack(pack: &[u8]) -> Option<Pack> {
    let conf: serde_json::Value =
        serde_json::from_slice(pack_entry(pack, "tropconf.json")?).ok()?;
    let language = conf.get("defaultLanguage")?.as_str()?;
    let meta: serde_json::Value =
        serde_json::from_slice(pack_entry(pack, &format!("tropmeta_{language}.json"))?).ok()?;
    let texts = meta.get("metadata")?;
    let text = |value: &serde_json::Value, key: &str| -> String {
        value
            .get(key)
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_owned()
    };
    let mut set = Pack {
        comm_id: text(&conf, "trophyNpCommId"),
        title: text(texts.get("titleMetadata")?, "name"),
        ..Pack::default()
    };
    for group in conf
        .get("groups")
        .and_then(|g| g.as_array())
        .into_iter()
        .flatten()
    {
        set.groups.insert(read_id(group.get("id")?)?, String::new());
    }
    for named in texts
        .get("groupMetadata")
        .and_then(|g| g.as_array())
        .into_iter()
        .flatten()
    {
        if let Some(name) = named
            .get("id")
            .and_then(read_id)
            .and_then(|id| set.groups.get_mut(&id))
        {
            *name = text(named, "name");
        }
    }
    for definition in conf.get("trophies")?.as_array()? {
        let id = read_id(definition.get("id")?)?;
        let flag =
            |key: &str| definition.get(key).and_then(serde_json::Value::as_bool) == Some(true);
        let condition = definition.get("unlockCondition");
        let progressive = condition
            .and_then(|c| c.get("progressive"))
            .and_then(serde_json::Value::as_bool)
            == Some(true);
        set.trophies.insert(
            id,
            Trophy {
                id,
                grade: Grade::from_letter(definition.get("grade")?.as_str()?)?,
                group: definition
                    .get("groupId")
                    .and_then(read_id)
                    .unwrap_or(BASE_GROUP),
                platinum: definition.get("platinumTrophyId").and_then(read_id),
                hidden: flag("hidden"),
                has_reward: flag("hasReward"),
                target: progressive
                    .then(|| {
                        condition
                            .and_then(|c| c.get("targetValue"))
                            .and_then(read_unsigned)
                    })
                    .flatten(),
                unlock: condition.and_then(|c| {
                    Some(Unlock {
                        stat: c.get("udsStatId").and_then(read_unsigned)?,
                        comparator: Comparator::parse(c.get("comparator")?.as_str()?)?,
                        target: i64::try_from(c.get("targetValue").and_then(read_unsigned)?)
                            .ok()?,
                    })
                }),
                name: String::new(),
                detail: String::new(),
                reward: String::new(),
            },
        );
    }
    for named in texts.get("trophyMetadata")?.as_array()? {
        if let Some(trophy) = named
            .get("id")
            .and_then(read_id)
            .and_then(|id| set.trophies.get_mut(&id))
        {
            trophy.name = text(named, "name");
            trophy.detail = text(named, "detail");
            trophy.reward = text(named, "reward");
        }
    }
    Some(set)
}

/// One user's record of a trophy set: what they have earned, and when, and the statistics that
/// earn the rest.
#[derive(Debug, Clone)]
struct Book {
    /// The set.
    pack: Arc<Pack>,
    /// The title's extraction rules, where it ships a universal-data pack.
    rules: Option<Arc<Rules>>,
    /// Each statistic the user's events have moved, by identifier.
    stats: BTreeMap<u64, i64>,
    /// Each trophy earned, with the tick it was earned at.
    earned: BTreeMap<i32, u64>,
    /// The guest path the record is kept at, or `None` for one kept only in memory.
    kept_at: Option<String>,
}

/// How many trophies of each grade, and of all grades, a selection holds.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Counts {
    /// All of them.
    all: u32,
    /// Platinum, gold, silver, bronze.
    by_grade: [u32; 4],
}

impl Counts {
    /// Counts one trophy of `grade`.
    fn add(&mut self, grade: Grade) {
        self.all += 1;
        self.by_grade[grade as usize - 1] += 1;
    }
}

impl Book {
    /// The trophies of `group` (every group when `None`), and those of them earned.
    fn counts(&self, group: Option<i32>) -> (Counts, Counts) {
        let (mut total, mut earned) = (Counts::default(), Counts::default());
        for trophy in self.pack.trophies.values() {
            if group.is_some_and(|group| group != trophy.group) {
                continue;
            }
            total.add(trophy.grade);
            if self.earned.contains_key(&trophy.id) {
                earned.add(trophy.grade);
            }
        }
        (total, earned)
    }

    /// Records `trophy` earned at `tick`, with the platinum it completes, and answers what was newly
    /// earned. A platinum is earned only by completing its set; a trophy earned before is not again.
    fn earn(&mut self, trophy: i32, tick: u64) -> Vec<i32> {
        let Some(found) = self.pack.trophies.get(&trophy) else {
            return Vec::new();
        };
        if found.grade == Grade::Platinum || self.earned.contains_key(&trophy) {
            return Vec::new();
        }
        self.earned.insert(trophy, tick);
        let mut newly = vec![trophy];
        if let Some(platinum) = found.platinum
            && self.pack.trophies.contains_key(&platinum)
            && !self.earned.contains_key(&platinum)
            && self
                .pack
                .trophies
                .values()
                .filter(|t| t.platinum == Some(platinum))
                .all(|t| self.earned.contains_key(&t.id))
        {
            self.earned.insert(platinum, tick);
            newly.push(platinum);
        }
        newly
    }

    /// Folds `event` into the statistics by the title's rules, and earns, at `tick`, each trophy
    /// whose statistic now meets its target. Answers what was newly earned.
    fn post(&mut self, event: &Event, tick: u64) -> Vec<i32> {
        let Some(rules) = self.rules.clone() else {
            return Vec::new();
        };
        let changed = rules.apply(&mut self.stats, event);
        let met: Vec<i32> = self
            .pack
            .trophies
            .values()
            .filter(|trophy| {
                trophy.unlock.is_some_and(|unlock| {
                    changed.contains(&unlock.stat)
                        && self
                            .stats
                            .get(&unlock.stat)
                            .is_some_and(|value| unlock.comparator.holds(value, &unlock.target))
                })
            })
            .map(|trophy| trophy.id)
            .collect();
        met.into_iter()
            .flat_map(|trophy| self.earn(trophy, tick))
            .collect()
    }

    /// The record as kept: each earned trophy with its tick, and each statistic.
    fn to_json(&self) -> String {
        let earned: serde_json::Map<String, serde_json::Value> = self
            .earned
            .iter()
            .map(|(id, tick)| (id.to_string(), serde_json::Value::from(*tick)))
            .collect();
        let stats: serde_json::Map<String, serde_json::Value> = self
            .stats
            .iter()
            .map(|(id, value)| (id.to_string(), serde_json::Value::from(*value)))
            .collect();
        serde_json::json!({ "commId": self.pack.comm_id, "earned": earned, "stats": stats })
            .to_string()
    }

    /// The earned trophies and the statistics a kept record holds.
    fn kept_from_json(text: &str) -> (BTreeMap<i32, u64>, BTreeMap<u64, i64>) {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(text) else {
            return (BTreeMap::new(), BTreeMap::new());
        };
        let table = |key: &str| {
            value
                .get(key)
                .and_then(serde_json::Value::as_object)
                .cloned()
                .unwrap_or_default()
        };
        let earned = table("earned")
            .iter()
            .filter_map(|(id, tick)| Some((id.parse().ok()?, tick.as_u64()?)))
            .collect();
        let stats = table("stats")
            .iter()
            .filter_map(|(id, value)| Some((id.parse().ok()?, value.as_i64()?)))
            .collect();
        (earned, stats)
    }

    /// Writes the record where it is kept.
    fn keep(&self) {
        let Some(path) = &self.kept_at else {
            return;
        };
        if let Some(host) = orbistoun_fs::mount::resolve_for_create(path) {
            if let Some(parent) = host.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let _ = std::fs::write(host, self.to_json());
        }
    }
}

/// One context: whose trophies, which set, and the record once registered.
#[derive(Debug, Clone)]
struct Context {
    /// The user.
    user: u32,
    /// The service label naming the set.
    label: u32,
    /// The record, from registration on.
    book: Option<(u32, u32)>,
}

/// Every context, handle and record.
#[derive(Debug, Default)]
struct Trophies {
    /// Each live context, by identifier.
    contexts: BTreeMap<i32, Context>,
    /// The identifier the next context takes.
    next_context: i32,
    /// The identifier the next handle takes.
    next_handle: i32,
    /// Each record, by user and service label.
    books: BTreeMap<(u32, u32), Book>,
    /// The callback a title asked to hear of earnings through, and its user data.
    unlock_callback: Option<(u64, u64)>,
}

/// The process's trophies.
static TROPHIES: Mutex<Trophies> = Mutex::new(Trophies {
    contexts: BTreeMap::new(),
    next_context: 0,
    next_handle: 0,
    books: BTreeMap::new(),
    unlock_callback: None,
});

/// The trophies, held.
fn trophies() -> std::sync::MutexGuard<'static, Trophies> {
    TROPHIES
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// The guest path of the trophy pack for `label`.
fn pack_path(label: u32) -> String {
    format!("/app0/sce_sys/trophy2/trophy{label:02}.ucp")
}

/// The guest path of the universal-data pack for `label`, whose rules turn events into the
/// statistics trophies are earned by.
fn rules_path(label: u32) -> String {
    format!("/app0/sce_sys/uds/uds{label:02}.ucp")
}

/// The guest path `user`'s record of the set `comm_id` is kept at.
fn record_path(comm_id: &str, user: u32) -> String {
    format!("/data/sce_trophy2/{comm_id}/user-{user}.json")
}

/// `user`'s record of the set for `label`, read from the title's packs and the record a previous run
/// kept, or `None` when the title has no trophy pack for it.
fn load_book(user: u32, label: u32) -> Option<Book> {
    let host = orbistoun_fs::mount::resolve_existing(&pack_path(label))?;
    let pack = read_pack(&std::fs::read(host).ok()?)?;
    let rules = orbistoun_fs::mount::resolve_existing(&rules_path(label))
        .and_then(|host| std::fs::read(host).ok())
        .and_then(|bytes| Rules::read(&bytes))
        .map(Arc::new);
    let kept_at = record_path(&pack.comm_id, user);
    let (earned, stats) = orbistoun_fs::mount::resolve_existing(&kept_at)
        .and_then(|host| std::fs::read_to_string(host).ok())
        .map(|text| Book::kept_from_json(&text))
        .unwrap_or_default();
    Some(Book {
        pack: Arc::new(pack),
        rules,
        stats,
        earned,
        kept_at: Some(kept_at),
    })
}

/// A 32-bit identifier passed in a 64-bit argument register.
fn id(arg: u64) -> i32 {
    arg as u32 as i32
}

/// Writes `value` into `out` at `at`, little-endian.
fn put(out: &mut [u8], at: usize, value: &[u8]) {
    out[at..at + value.len()].copy_from_slice(value);
}

/// Writes `text` into `out` at `at` as a NUL-terminated string of at most `room` bytes, cut at a
/// character boundary.
fn put_text(out: &mut [u8], at: usize, room: usize, text: &str) {
    let mut end = text.len().min(room - 1);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    put(out, at, &text.as_bytes()[..end]);
}

/// Writes the trophy counts of a game or group into `details` at `at`: all, then each grade.
fn put_counts(details: &mut [u8], at: usize, total: Counts) {
    put(details, at, &total.all.to_le_bytes());
    for (index, count) in total.by_grade.iter().enumerate() {
        put(details, at + 4 + index * 4, &count.to_le_bytes());
    }
}

/// The share of `total` earned, in percent, rounded down.
fn percent(total: Counts, earned: Counts) -> u32 {
    (earned.all * 100).checked_div(total.all).unwrap_or(0)
}

/// `SceNpTrophy2GameDetails` and `SceNpTrophy2GameData` for `book`.
fn game_info(book: &Book) -> ([u8; GAME_DETAILS_BYTES], [u8; GAME_DATA_BYTES]) {
    let (total, earned) = book.counts(None);
    let mut details = [0_u8; GAME_DETAILS_BYTES];
    let groups = u32::try_from(book.pack.groups.len()).unwrap_or(u32::MAX);
    put(&mut details, 0, &groups.to_le_bytes());
    put_counts(&mut details, 4, total);
    put_text(&mut details, 24, 128, &book.pack.title);
    let mut data = [0_u8; GAME_DATA_BYTES];
    put_counts(&mut data, 0, earned);
    put(&mut data, 20, &percent(total, earned).to_le_bytes());
    (details, data)
}

/// `SceNpTrophy2Details` and `SceNpTrophy2Data` for `trophy` of `book`. A progressive trophy's
/// target and progress carry type 1; progress is counted only as far as earned or not.
fn trophy_info(book: &Book, trophy: &Trophy) -> (Vec<u8>, [u8; TROPHY_DATA_BYTES]) {
    let mut details = vec![0_u8; TROPHY_DETAILS_BYTES];
    put(&mut details, 0, &trophy.id.to_le_bytes());
    put(&mut details, 4, &(trophy.grade as i32).to_le_bytes());
    put(&mut details, 8, &trophy.group.to_le_bytes());
    details[12] = u8::from(trophy.hidden);
    details[13] = u8::from(trophy.has_reward);
    if let Some(target) = trophy.target {
        put(&mut details, 0x10, &1_i32.to_le_bytes());
        put(&mut details, 0x18, &target.to_le_bytes());
    }
    put_text(&mut details, 0x20, 128, &trophy.name);
    put_text(&mut details, 0xa0, 1024, &trophy.detail);
    put_text(&mut details, 0x4a0, 128, &trophy.reward);
    let earned = book.earned.get(&trophy.id).copied();
    let mut data = [0_u8; TROPHY_DATA_BYTES];
    put(&mut data, 0, &trophy.id.to_le_bytes());
    data[4] = u8::from(earned.is_some());
    if let Some(target) = trophy.target {
        put(&mut data, 8, &1_i32.to_le_bytes());
        put(&mut data, 0x10, &earned.map_or(0, |_| target).to_le_bytes());
    }
    put(&mut data, 0x18, &earned.unwrap_or(0).to_le_bytes());
    (details, data)
}

/// Writes `bytes` to the guest at `at`, when `at` is not null.
fn write_out(at: u64, bytes: &[u8]) -> bool {
    // SAFETY: a destination the guest passed for this call, by its contract as large as the
    // structure written; the accessor checks it.
    at == 0 || unsafe { guest::write_bytes(at, bytes) }
}

/// The record behind registered context `context`, or the error a query on it answers.
fn registered(state: &Trophies, context: i32) -> Result<&Book, u64> {
    let found = state.contexts.get(&context).ok_or(INVALID_CONTEXT)?;
    let key = found.book.ok_or(NOT_REGISTERED)?;
    state.books.get(&key).ok_or(NOT_REGISTERED)
}

/// `sceNpTrophy2CreateContext(context *, user, label, options)`: a context for `user`'s trophies in
/// the set `label` names, its identifier written to `*context`. One per user and set, at most
/// eight; options other than zero are refused.
fn create_context(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (out, user, label, options) = (args[0], args[1] as u32, args[2] as u32, args[3]);
    if out == 0 || options != 0 {
        return INVALID_ARGUMENT;
    }
    let mut state = trophies();
    if state
        .contexts
        .values()
        .any(|c| c.user == user && c.label == label)
    {
        return CONTEXT_ALREADY_EXISTS;
    }
    if state.contexts.len() >= MOST_CONTEXTS {
        return CONTEXT_EXCEEDS_MAX;
    }
    let context = state.next_context + 1;
    // SAFETY: the guest's out-parameter, one 32-bit word by the call's contract.
    if !unsafe { guest::write_u32(out, context as u32) } {
        return INVALID_ARGUMENT;
    }
    state.next_context = context;
    state.contexts.insert(
        context,
        Context {
            user,
            label,
            book: None,
        },
    );
    0
}

/// `sceNpTrophy2CreateHandle(handle *)`: a handle for the calls that take one, written to
/// `*handle`. Handles carry no state: an offline console has no request to abort.
fn create_handle(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    if args[0] == 0 {
        return INVALID_ARGUMENT;
    }
    let mut state = trophies();
    let handle = state.next_handle + 1;
    // SAFETY: the guest's out-parameter, one 32-bit word by the call's contract.
    if !unsafe { guest::write_u32(args[0], handle as u32) } {
        return INVALID_ARGUMENT;
    }
    state.next_handle = handle;
    0
}

/// Registers `context` with `book` when the context has no record yet: the record already open for
/// its user and set is kept, so two registrations agree.
fn register(
    state: &mut Trophies,
    context: i32,
    book: impl FnOnce(u32, u32) -> Option<Book>,
) -> u64 {
    let Some(found) = state.contexts.get(&context) else {
        return INVALID_CONTEXT;
    };
    if found.book.is_some() {
        return ALREADY_REGISTERED;
    }
    let key = (found.user, found.label);
    if let std::collections::btree_map::Entry::Vacant(vacant) = state.books.entry(key) {
        let Some(opened) = book(key.0, key.1) else {
            return TITLE_CONF_NOT_INSTALLED;
        };
        vacant.insert(opened);
    }
    if let Some(found) = state.contexts.get_mut(&context) {
        found.book = Some(key);
    }
    0
}

/// `sceNpTrophy2RegisterContext(context, handle, options)`: opens the context's set from the
/// title's trophy pack, with what the user earned before. A title without a pack for the label is
/// refused as not installed.
fn register_context(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    if args[2] != 0 {
        return INVALID_ARGUMENT;
    }
    register(&mut trophies(), id(args[0]), load_book)
}

/// `sceNpTrophy2GetGameInfo(context, handle, details *, data *)`: the set's counts and title, and
/// how much of it is earned; either destination may be null, not both.
fn get_game_info(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (details_at, data_at) = (args[2], args[3]);
    if details_at == 0 && data_at == 0 {
        return INVALID_ARGUMENT;
    }
    let state = trophies();
    let book = match registered(&state, id(args[0])) {
        Ok(book) => book,
        Err(error) => return error,
    };
    let (details, data) = game_info(book);
    if write_out(details_at, &details) && write_out(data_at, &data) {
        0
    } else {
        INVALID_ARGUMENT
    }
}

/// `sceNpTrophy2GetTrophyInfo(context, handle, trophy, details *, data *)`: one trophy's
/// definition and whether, and when, it was earned.
fn get_trophy_info(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (trophy, details_at, data_at) = (id(args[2]), args[3], args[4]);
    if details_at == 0 && data_at == 0 {
        return INVALID_ARGUMENT;
    }
    let state = trophies();
    let book = match registered(&state, id(args[0])) {
        Ok(book) => book,
        Err(error) => return error,
    };
    let Some(found) = book.pack.trophies.get(&trophy) else {
        return INVALID_TROPHY_ID;
    };
    let (details, data) = trophy_info(book, found);
    if write_out(details_at, &details) && write_out(data_at, &data) {
        0
    } else {
        INVALID_ARGUMENT
    }
}

/// `sceNpTrophy2GetTrophyInfoArray(context, handle, offset, limit, details *, data *, count *)`:
/// at most `limit` trophies from the `offset`th, in identifier order, into the two arrays, and how
/// many into `*count` - the seventh argument, the first on the stack.
fn get_trophy_info_array(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let area = orbistoun_thunk::stack_arguments();
    // Zero outside a guest call, which reads as no count.
    let count_at = if area == 0 {
        0
    } else {
        // SAFETY: the guest's own stack, the word above the return address of the call running
        // now; read through the checked accessor.
        unsafe { guest::read_u64(area) }.unwrap_or(0)
    };
    trophy_info_array(args, count_at)
}

/// [`get_trophy_info_array`] with its count destination read.
fn trophy_info_array(args: &[u64; GUEST_ARG_REGISTERS], count_at: u64) -> u64 {
    let (offset, limit, details_at, data_at) = (
        args[2] as u32 as usize,
        args[3] as u32 as usize,
        args[4],
        args[5],
    );
    if count_at == 0 || (details_at == 0 && data_at == 0) {
        return INVALID_ARGUMENT;
    }
    let state = trophies();
    let book = match registered(&state, id(args[0])) {
        Ok(book) => book,
        Err(error) => return error,
    };
    let mut written = 0_u32;
    for (index, trophy) in book
        .pack
        .trophies
        .values()
        .skip(offset)
        .take(limit)
        .enumerate()
    {
        let (details, data) = trophy_info(book, trophy);
        let details_slot = details_at.wrapping_add((index * TROPHY_DETAILS_BYTES) as u64);
        let data_slot = data_at.wrapping_add((index * TROPHY_DATA_BYTES) as u64);
        if !write_out(if details_at == 0 { 0 } else { details_slot }, &details)
            || !write_out(if data_at == 0 { 0 } else { data_slot }, &data)
        {
            return INVALID_ARGUMENT;
        }
        written += 1;
    }
    // SAFETY: the guest's count, one 32-bit word by the call's contract.
    if unsafe { guest::write_u32(count_at, written) } {
        0
    } else {
        INVALID_ARGUMENT
    }
}

/// `sceNpTrophy2RegisterUnlockCallback(callback, user data)`: keeps the callback a title asked to
/// hear of earnings through.
fn register_unlock_callback(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    trophies().unlock_callback = Some((args[0], args[1]));
    0
}

/// `sceNpTrophy2AbortHandle(handle)` and `sceNpTrophy2DestroyHandle(handle)`: nothing is pending on
/// an offline console's handle, so each answers 0.
fn handle_done(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    0
}

/// `sceNpTrophy2DestroyContext(context)`: ends a context; its user's record stays open for the next.
fn destroy_context(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    if trophies().contexts.remove(&id(args[0])).is_some() {
        0
    } else {
        INVALID_CONTEXT
    }
}

/// Folds `event`, which `user` posted through the universal data system, into the statistics of
/// the set for `label`, and earns now each trophy a statistic meets, with the platinum it
/// completes; the record is kept when anything moved. The set is opened from the title's packs
/// when no context has opened it. Answers what was newly earned.
pub fn post_event(user: u32, label: u32, event: &Event) -> Vec<i32> {
    let (seconds, nanos) = orbistoun_hle::clocks::wall_clock();
    let tick = crate::rtc::tick_at(seconds, nanos);
    let mut state = trophies();
    let key = (user, label);
    if !state.books.contains_key(&key)
        && let Some(opened) = load_book(user, label)
    {
        state.books.insert(key, opened);
    }
    let Some(book) = state.books.get_mut(&key) else {
        return Vec::new();
    };
    let before = book.stats.clone();
    let newly = book.post(event, tick);
    if !newly.is_empty() || book.stats != before {
        book.keep();
    }
    newly
}

/// Implementations this module provides, by symbol name.
#[must_use]
pub fn implementations() -> &'static [(&'static str, GuestFn)] {
    &[
        ("sceNpTrophy2CreateContext", create_context),
        ("sceNpTrophy2CreateHandle", create_handle),
        ("sceNpTrophy2RegisterContext", register_context),
        ("sceNpTrophy2GetGameInfo", get_game_info),
        ("sceNpTrophy2GetTrophyInfo", get_trophy_info),
        ("sceNpTrophy2GetTrophyInfoArray", get_trophy_info_array),
        (
            "sceNpTrophy2RegisterUnlockCallback",
            register_unlock_callback,
        ),
        ("sceNpTrophy2AbortHandle", handle_done),
        ("sceNpTrophy2DestroyHandle", handle_done),
        ("sceNpTrophy2DestroyContext", destroy_context),
    ]
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A trophy pack holding `entries`, laid out as a title's is.
    pub(crate) fn pack_of(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let table = PACK_HEADER_BYTES + entries.len() * 0x40;
        let mut pack = vec![0_u8; table];
        pack[..4].copy_from_slice(&PACK_MAGIC.to_be_bytes());
        pack[4..8].copy_from_slice(&1_u32.to_be_bytes());
        pack[0x10..0x14].copy_from_slice(&(entries.len() as u32).to_be_bytes());
        pack[0x14..0x18].copy_from_slice(&0x40_u32.to_be_bytes());
        for (index, (name, bytes)) in entries.iter().enumerate() {
            let at = PACK_HEADER_BYTES + index * 0x40;
            let offset = pack.len() as u64;
            pack[at..at + name.len()].copy_from_slice(name.as_bytes());
            pack[at + 0x20..at + 0x28].copy_from_slice(&offset.to_be_bytes());
            pack[at + 0x28..at + 0x30].copy_from_slice(&(bytes.len() as u64).to_be_bytes());
            pack.extend_from_slice(bytes);
        }
        let total = pack.len() as u64;
        pack[8..0x10].copy_from_slice(&total.to_be_bytes());
        pack
    }

    /// A set of a platinum, two bronzes counting towards it in the base game, and a gold in group 1
    /// with a progressive target.
    fn sample_pack() -> Vec<u8> {
        let conf = br#"{"trophyNpCommId":"NPWR00000_00","defaultLanguage":"en-US",
            "groups":[{"id":"0001"}],
            "trophies":[{"id":"0000","grade":"P"},
              {"id":"0001","grade":"B","hidden":true,"platinumTrophyId":"0000",
               "unlockCondition":{"udsStatId":"19","comparator":"ge","targetValue":"1","progressive":false}},
              {"id":"0002","grade":"B","platinumTrophyId":"0000",
               "unlockCondition":{"udsStatId":"20","comparator":"ge","targetValue":"1","progressive":false}},
              {"id":"0003","grade":"G","groupId":"0001","hasReward":true,
               "unlockCondition":{"udsStatId":"40","comparator":"ge","targetValue":"25","progressive":true}}]}"#;
        let meta = br#"{"metadata":{"titleMetadata":{"name":"A Title"},
            "groupMetadata":[{"id":"0001","name":"An Expansion"}],
            "trophyMetadata":[{"id":"0000","name":"All","detail":"Earn every trophy"},
              {"id":"0001","name":"First","detail":"Begin"},
              {"id":"0002","name":"Second","detail":"Go on"},
              {"id":"0003","name":"Third","detail":"Count to 25","reward":"A hat"}]}}"#;
        pack_of(&[
            ("trop0000.png", b"\x89PNG"),
            ("tropconf.json", conf),
            ("tropmeta_en-US.json", meta),
        ])
    }

    /// A pack's entries are found by name, and a pack that is not one finds nothing.
    #[test]
    fn a_trophy_pack_entry_is_found_by_name() {
        let pack = sample_pack();
        assert_eq!(pack_entry(&pack, "trop0000.png"), Some(&b"\x89PNG"[..]));
        assert_eq!(pack_entry(&pack, "trop0001.png"), None);
        assert_eq!(
            pack_entry(b"not a pack at all, not one bit", "tropconf.json"),
            None
        );
    }

    /// A set reads its title, groups and trophies, names in the default language.
    #[test]
    fn a_trophy_set_reads_from_its_pack() {
        let set = read_pack(&sample_pack()).expect("the sample set reads");
        assert_eq!(set.comm_id, "NPWR00000_00");
        assert_eq!(set.title, "A Title");
        assert_eq!(set.groups, BTreeMap::from([(1, "An Expansion".to_owned())]));
        assert_eq!(set.trophies.len(), 4);
        let first = &set.trophies[&1];
        assert_eq!(
            (
                first.grade,
                first.group,
                first.platinum,
                first.hidden,
                first.unlock,
                first.target
            ),
            (
                Grade::Bronze,
                BASE_GROUP,
                Some(0),
                true,
                Some(Unlock {
                    stat: 19,
                    comparator: Comparator::AtLeast,
                    target: 1
                }),
                None
            )
        );
        let third = &set.trophies[&3];
        assert_eq!(
            (third.group, third.target, third.has_reward),
            (1, Some(25), true)
        );
        assert_eq!(
            (third.name.as_str(), third.reward.as_str()),
            ("Third", "A hat")
        );
    }

    /// The sample set's rules: trophies 1 and 2 count `_UnlockTrophy` into statistics 19 and 20,
    /// and trophy 3's progress is the greatest `_UpdateTrophyProgress` into statistic 40.
    fn sample_rules() -> Rules {
        let definitions = br#"{"statDefinitionArray":[
            {"statId":19,"aggregation":"count","maxValue":"1","minValue":"0","initialValue":"0"},
            {"statId":20,"aggregation":"count","maxValue":"1","minValue":"0","initialValue":"0"},
            {"statId":40,"aggregation":"max","initialValue":"0"}]}"#;
        let extraction = br#"{"statsExtractionRuleArray":[
            {"condition":{"eventName":"_UnlockTrophy","property":{"path":"$._trophy_id","comparator":"==","value":"1"}},
             "action":{"output":{"statId":19}}},
            {"condition":{"eventName":"_UnlockTrophy","property":{"path":"$._trophy_id","comparator":"==","value":"2"}},
             "action":{"output":{"statId":20}}},
            {"condition":{"eventName":"_UpdateTrophyProgress","property":{"path":"$._trophy_id","comparator":"==","value":"3"}},
             "action":{"input":"$._trophy_progress","output":{"statId":40}}}]}"#;
        Rules::read(&pack_of(&[
            ("stats_definition.json", definitions),
            ("stats_extraction.json", extraction),
        ]))
        .expect("the sample rules read")
    }

    /// A book over the sample set and its rules, kept nowhere.
    fn sample_book() -> Book {
        Book {
            pack: Arc::new(read_pack(&sample_pack()).expect("the sample set reads")),
            rules: Some(Arc::new(sample_rules())),
            stats: BTreeMap::new(),
            earned: BTreeMap::new(),
            kept_at: None,
        }
    }

    /// An event of `name` with integer properties.
    fn event(name: &str, properties: &[(&str, i64)]) -> Event {
        Event {
            name: name.to_owned(),
            properties: properties
                .iter()
                .map(|(path, value)| ((*path).to_owned(), crate::uds_stats::Value::Int(*value)))
                .collect(),
        }
    }

    /// A trophy is earned when the event its statistic counts is posted, and a progressive one when
    /// its progress reaches the target; what was posted is kept with what was earned.
    #[test]
    fn posted_events_earn_the_trophies_their_statistics_meet() {
        let mut book = sample_book();
        let unlock = |id| event("_UnlockTrophy", &[("$._trophy_id", id)]);
        let progress = |value| {
            event(
                "_UpdateTrophyProgress",
                &[("$._trophy_id", 3), ("$._trophy_progress", value)],
            )
        };
        assert_eq!(book.post(&unlock(9), 1), Vec::<i32>::new());
        assert_eq!(book.post(&unlock(1), 2), vec![1]);
        assert_eq!(book.post(&unlock(1), 3), Vec::<i32>::new());
        assert_eq!(book.post(&progress(24), 4), Vec::<i32>::new());
        assert_eq!(book.post(&progress(25), 5), vec![3]);
        assert_eq!(book.post(&unlock(2), 6), vec![2, 0]);
        let (earned, stats) = Book::kept_from_json(&book.to_json());
        assert_eq!(earned, BTreeMap::from([(0, 6), (1, 2), (2, 6), (3, 5)]));
        assert_eq!(stats, BTreeMap::from([(19, 1), (20, 1), (40, 25)]));
    }

    /// Earning every trophy towards a platinum earns the platinum with the last; a platinum is not
    /// earned directly, and a trophy is earned once.
    #[test]
    fn the_last_trophy_towards_a_platinum_earns_it() {
        let mut book = sample_book();
        assert_eq!(book.earn(0, 5), Vec::<i32>::new());
        assert_eq!(book.earn(1, 10), vec![1]);
        assert_eq!(book.earn(1, 11), Vec::<i32>::new());
        assert_eq!(book.earn(2, 20), vec![2, 0]);
        assert_eq!(book.earned, BTreeMap::from([(0, 20), (1, 10), (2, 20)]));
        let (kept, _) = Book::kept_from_json(&book.to_json());
        assert_eq!(kept, book.earned);
    }

    /// The game's details count each grade and name the title; its data counts what is earned.
    #[test]
    fn game_info_counts_the_set_and_what_is_earned() {
        let mut book = sample_book();
        book.earn(1, 10);
        let (details, data) = game_info(&book);
        let word =
            |bytes: &[u8], at: usize| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
        // Groups 1, trophies 4: platinum 1, gold 1, silver 0, bronze 2.
        let counts: Vec<u32> = (0..6).map(|i| word(&details, i * 4)).collect();
        assert_eq!(counts, [1, 4, 1, 1, 0, 2]);
        assert_eq!(&details[24..32], b"A Title\0");
        let earned: Vec<u32> = (0..6).map(|i| word(&data, i * 4)).collect();
        assert_eq!(earned, [1, 0, 0, 0, 1, 25]);
    }

    /// A trophy's details carry its definition and strings at their offsets, and its data whether
    /// and when it was earned.
    #[test]
    fn trophy_info_lays_out_details_and_data() {
        let mut book = sample_book();
        book.earn(3, 77);
        let trophy = book.pack.trophies[&3].clone();
        let (details, data) = trophy_info(&book, &trophy);
        assert_eq!(details.len(), 0x520);
        assert_eq!(&details[0..14], &[3, 0, 0, 0, 2, 0, 0, 0, 1, 0, 0, 0, 0, 1]);
        assert_eq!(&details[0x10..0x14], &1_i32.to_le_bytes());
        assert_eq!(&details[0x18..0x20], &25_u64.to_le_bytes());
        assert_eq!(&details[0x20..0x26], b"Third\0");
        assert_eq!(&details[0xa0..0xac], b"Count to 25\0");
        assert_eq!(&details[0x4a0..0x4a6], b"A hat\0");
        assert_eq!(data[4], 1);
        assert_eq!(&data[0x10..0x18], &25_u64.to_le_bytes());
        assert_eq!(&data[0x18..0x20], &77_u64.to_le_bytes());
    }

    /// A long string is cut whole characters short of its room, and always terminated.
    #[test]
    fn a_long_string_is_cut_at_a_character() {
        let mut out = [0xff_u8; 8];
        put_text(&mut out, 0, 4, "aé日");
        assert_eq!(&out[..4], &[b'a', 0xc3, 0xa9, 0xff]);
        let mut out = [0_u8; 4];
        put_text(&mut out, 0, 4, "abcdef");
        assert_eq!(&out, b"abc\0");
    }

    /// A context is made once per user and set, registers once with its record, answers queries only
    /// when registered, and is gone once destroyed. One test: the trophies are the process's.
    #[test]
    fn a_context_lives_from_creation_to_destruction() {
        let mut context = 0_u32;
        let out = std::ptr::from_mut(&mut context) as u64;
        let label = 0x7e57;
        assert_eq!(create_context(&[0, 1, label, 0, 0, 0]), INVALID_ARGUMENT);
        assert_eq!(create_context(&[out, 1, label, 1, 0, 0]), INVALID_ARGUMENT);
        assert_eq!(create_context(&[out, 1, label, 0, 0, 0]), 0);
        let made = u64::from(context);
        assert_ne!(made, 0);
        assert_eq!(
            create_context(&[out, 1, label, 0, 0, 0]),
            CONTEXT_ALREADY_EXISTS
        );

        let mut details = [0_u8; GAME_DETAILS_BYTES];
        let details_at = details.as_mut_ptr() as u64;
        assert_eq!(
            get_game_info(&[made, 1, details_at, 0, 0, 0]),
            NOT_REGISTERED
        );
        assert_eq!(get_game_info(&[made, 1, 0, 0, 0, 0]), INVALID_ARGUMENT);
        assert_eq!(
            register(&mut trophies(), id(made), |_, _| None),
            TITLE_CONF_NOT_INSTALLED
        );
        assert_eq!(
            register(&mut trophies(), id(made), |_, _| Some(sample_book())),
            0
        );
        assert_eq!(
            register(&mut trophies(), id(made), |_, _| None),
            ALREADY_REGISTERED
        );
        assert_eq!(get_game_info(&[made, 1, details_at, 0, 0, 0]), 0);
        assert_eq!(&details[24..32], b"A Title\0");

        let unlock = event("_UnlockTrophy", &[("$._trophy_id", 2)]);
        assert_eq!(post_event(1, label as u32, &unlock), vec![2]);
        let mut data = [0_u8; TROPHY_DATA_BYTES];
        let data_at = data.as_mut_ptr() as u64;
        assert_eq!(get_trophy_info(&[made, 1, 2, 0, data_at, 0]), 0);
        assert_eq!(data[4], 1);
        assert_eq!(
            get_trophy_info(&[made, 1, 9, 0, data_at, 0]),
            INVALID_TROPHY_ID
        );

        let mut array = [0_u8; 2 * TROPHY_DATA_BYTES];
        let mut count = 0_u32;
        let (array_at, count_at) = (
            array.as_mut_ptr() as u64,
            std::ptr::from_mut(&mut count) as u64,
        );
        assert_eq!(
            trophy_info_array(&[made, 1, 2, 5, 0, array_at], count_at),
            0
        );
        assert_eq!(count, 2);
        assert_eq!((array[0], array[4], array[32], array[36]), (2, 1, 3, 0));
        assert_eq!(
            trophy_info_array(&[made, 1, 0, 5, 0, array_at], 0),
            INVALID_ARGUMENT
        );

        assert_eq!(destroy_context(&[made, 0, 0, 0, 0, 0]), 0);
        assert_eq!(destroy_context(&[made, 0, 0, 0, 0, 0]), INVALID_CONTEXT);
        assert_eq!(
            get_game_info(&[made, 1, details_at, 0, 0, 0]),
            INVALID_CONTEXT
        );
    }
}
