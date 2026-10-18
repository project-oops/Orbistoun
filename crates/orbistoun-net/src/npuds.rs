//! `libSceNpUniversalDataSystem` - the platform's event and statistics reporting.
//!
//! The names come from real import tables (D504); an unmeasured arity is `6`, the trampoline's
//! full capture, not a claim about the argument count. `sceNpUniversalDataSystemInitialize` and
//! `sceNpUniversalDataSystemCreateContext`, `sceNpUniversalDataSystemCreateHandle` and
//! `sceNpUniversalDataSystemCreateEventPropertyObject`, `sceNpUniversalDataSystemCreateEvent`,
//! `sceNpUniversalDataSystemRegisterContext`, `sceNpUniversalDataSystemPostEvent`,
//! `sceNpUniversalDataSystemDestroyEvent`, `sceNpUniversalDataSystemDestroyEventPropertyObject`,
//! `sceNpUniversalDataSystemDestroyHandle` and `sceNpUniversalDataSystemEventPropertyObjectSetString`
//! are implemented, as measured.

use orbistoun_core::{GUEST_ARG_REGISTERS, GuestError, GuestFn};
use orbistoun_hle::guest_module;
use orbistoun_systemservice::uds_stats::{Event, Value};

guest_module! {
    "libSceNpUniversalDataSystem" {
        // (param)
        // (&ctx, user, 0, 0)
        "sceNpUniversalDataSystemCreateContext" => 4,
        "sceNpUniversalDataSystemInitialize" => 1,
        // (&handle)
        "sceNpUniversalDataSystemCreateHandle" => 1,
        // (&object)
        "sceNpUniversalDataSystemCreateEventPropertyObject" => 1,
        // (name, prop, &event, &own)
        "sceNpUniversalDataSystemCreateEvent" => 4,
        // (ctx, handle, 0)
        "sceNpUniversalDataSystemRegisterContext" => 3,
        // (ctx, handle, event, 0)
        "sceNpUniversalDataSystemPostEvent" => 4,
        // (event)
        "sceNpUniversalDataSystemDestroyEvent" => 1,
        // (object)
        "sceNpUniversalDataSystemDestroyEventPropertyObject" => 1,
        // (handle)
        "sceNpUniversalDataSystemDestroyHandle" => 1,
        // (object, key, value)
        "sceNpUniversalDataSystemEventPropertyObjectSetString" => 3,
        // (object, key, value)
        "sceNpUniversalDataSystemEventPropertyObjectSetInt32" => 3,
    }
}

/// Implementations this module provides, by symbol name.
#[must_use]
pub fn implementations() -> &'static [(&'static str, GuestFn)] {
    &[
        ("sceNpUniversalDataSystemInitialize", initialize),
        ("sceNpUniversalDataSystemCreateContext", create_context),
        ("sceNpUniversalDataSystemCreateHandle", create_handle),
        (
            "sceNpUniversalDataSystemCreateEventPropertyObject",
            create_event_property_object,
        ),
        ("sceNpUniversalDataSystemCreateEvent", create_event),
        ("sceNpUniversalDataSystemRegisterContext", register_context),
        ("sceNpUniversalDataSystemPostEvent", post_event),
        ("sceNpUniversalDataSystemDestroyEvent", destroy_event),
        (
            "sceNpUniversalDataSystemDestroyEventPropertyObject",
            destroy_event_property_object,
        ),
        ("sceNpUniversalDataSystemDestroyHandle", destroy_handle),
        (
            "sceNpUniversalDataSystemEventPropertyObjectSetString",
            set_string,
        ),
        (
            "sceNpUniversalDataSystemEventPropertyObjectSetInt32",
            set_int32,
        ),
    ]
}

/// What the console keeps of a title's universal-data calls: whose each context is, each property
/// object's values, and each event's name and object - what a posted event carries to the
/// statistics that earn trophies.
#[derive(Default)]
struct Kept {
    /// Each context, by number, with the user and service label it was made for.
    contexts: std::collections::BTreeMap<u32, (u32, u32)>,
    /// Each property object's values, by its address, keyed by path, `$.name`.
    objects: std::collections::BTreeMap<u64, std::collections::BTreeMap<String, Value>>,
    /// Each event, by its address, with its name and the property object it was made with.
    events: std::collections::BTreeMap<u64, (String, u64)>,
    /// The names of the events posted so far, each reported the first time it is posted.
    posted: std::collections::BTreeSet<String>,
}

impl Kept {
    /// Whether `name` is posted for the first time, which the run reports; a title that posts an
    /// event every frame is reported once per name.
    fn first_post(&mut self, name: &str) -> bool {
        self.posted.insert(name.to_owned())
    }
}

/// The process's universal-data state.
static KEPT: std::sync::LazyLock<std::sync::Mutex<Kept>> =
    std::sync::LazyLock::new(std::sync::Mutex::default);

/// The kept state, held.
fn kept() -> std::sync::MutexGuard<'static, Kept> {
    KEPT.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// The guest path of the title's universal-data pack, whose rules a posted event is counted by.
const UDS_PACK: &str = "/app0/sce_sys/uds/uds00.ucp";

/// Whether the title ships a universal-data pack. The console's measured refusals of
/// `RegisterContext` and `PostEvent` (`0x80553104`) came from a probe package that ships none; a
/// title that does registers and posts (assumed, as the reference emulators answer it).
fn title_has_rules() -> bool {
    orbistoun_fs::mount::resolve_existing(UDS_PACK).is_some()
}

/// A property key the guest passed, as the path its rules name it by, `$.key`.
fn property_path(key: u64) -> Option<String> {
    // SAFETY: the guest's key, a NUL-terminated string by the call's contract; read through the
    // checked accessor.
    let bytes = unsafe { orbistoun_mem::guest::read_cstr(key, 256) }?;
    Some(format!("$.{}", String::from_utf8(bytes).ok()?))
}

/// Keeps `value` at `key` of the property object `object`.
fn keep_property(object: u64, key: u64, value: Value) {
    if let Some(path) = property_path(key) {
        kept()
            .objects
            .entry(object)
            .or_default()
            .insert(path, value);
    }
}

/// `sceNpUniversalDataSystemInitialize(param)`: with `param` u64 0x10 (its size), u64 0x40000 (a
/// memory size), a pointer and u64 0x10, as PPSA02664's plugin passes it, answers 0 and leaves the
/// block as it was (REQ-np02, two sweeps: `20261011-083754-eboot.obs.log` 2933-2960 and
/// `20261011-1791715554-eboot.obs.log` 2987-3014, after `sceSysmoduleLoadModule(0x0105)`). A null
/// block is unmeasured and answers the placeholder.
fn initialize(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    if args[0] == 0 {
        return u64::from(GuestError::Unimplemented.as_raw());
    }
    0
}

/// The context a fresh process's first `sceNpUniversalDataSystemCreateContext` gave (REQ-np03,
/// `uds-create-context-call1`); each later one is the next.
static NEXT_CONTEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0x0004_00d3);

/// `sceNpUniversalDataSystemCreateContext(&ctx, user, 0, 0)`: writes the next context number to
/// the low 32 bits of `ctx`, leaving the high half as it was, and answers 0 - `0x400d3` then
/// `0x400d4` on the console (REQ-np03, `000-boot/np-appcontent-startup`, `report-1791758178.txt`
/// 223-266). A null `ctx` is unmeasured and answers the placeholder.
fn create_context(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    if args[0] == 0 {
        return u64::from(GuestError::Unimplemented.as_raw());
    }
    let number = NEXT_CONTEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    // SAFETY: the guest's context slot, by the call's contract; written through the checked
    // accessor.
    if unsafe { orbistoun_mem::guest::write_u32(args[0], number) } {
        kept()
            .contexts
            .insert(number, (args[1] as u32, args[2] as u32));
        0
    } else {
        u64::from(GuestError::Unimplemented.as_raw())
    }
}

/// The handle a fresh process's first `sceNpUniversalDataSystemCreateHandle` gave (REQ-pu01,
/// `000-boot/pad-ime-uds`, `report-1791801495.txt` 417-421); each later one is the next.
static NEXT_HANDLE: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0x0002_0176);

/// `sceNpUniversalDataSystemCreateHandle(&handle)`: writes the next handle number to the low 32
/// bits of `handle`, leaving the high half as it was, and answers 0 - `0x20176` then `0x20177` on
/// the console, after a context was made. A null `handle` is unmeasured and answers the
/// placeholder.
fn create_handle(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    if args[0] == 0 {
        return u64::from(GuestError::Unimplemented.as_raw());
    }
    let number = NEXT_HANDLE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    // SAFETY: the guest's handle slot, by the call's contract; written through the checked
    // accessor.
    if unsafe { orbistoun_mem::guest::write_u32(args[0], number) } {
        0
    } else {
        u64::from(GuestError::Unimplemented.as_raw())
    }
}

/// Handles `sceNpUniversalDataSystemDestroyHandle` has ended.
static DESTROYED_HANDLES: std::sync::Mutex<std::collections::BTreeSet<u32>> =
    std::sync::Mutex::new(std::collections::BTreeSet::new());

/// What a second `sceNpUniversalDataSystemDestroyHandle` of one handle answered (REQ-ud01).
const INVALID_HANDLE: u64 = 0x8055_3106;

/// `sceNpUniversalDataSystemDestroyHandle(handle)`: 0 for a handle `CreateHandle` gave, then
/// [`INVALID_HANDLE`] once it is gone (REQ-ud01, `000-boot/ux01-calls`,
/// `report-20261015-batch1.log` 2031-2032). A handle never given is unmeasured and answers the
/// placeholder. PPSA28061 destroys three.
fn destroy_handle(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let handle = args[0] as u32;
    let issued = 0x0002_0176..NEXT_HANDLE.load(std::sync::atomic::Ordering::Relaxed);
    if !issued.contains(&handle) {
        return u64::from(GuestError::Unimplemented.as_raw());
    }
    let mut destroyed = DESTROYED_HANDLES
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if destroyed.insert(handle) {
        0
    } else {
        INVALID_HANDLE
    }
}

/// What `sceNpUniversalDataSystemEventPropertyObjectSetString` answered for a null value
/// (REQ-ud01).
const INVALID_VALUE: u64 = 0x8055_3115;

/// `sceNpUniversalDataSystemEventPropertyObjectSetString(object, key, value)`: 0 for a key and a
/// value, again when the key is set over, and [`INVALID_VALUE`] for a null value (REQ-ud01,
/// `000-boot/ux01-calls`, `report-20261015-batch1.log` 2028-2030). The value is kept for the event
/// made with the object. A null object or key is unmeasured and answers the placeholder. PPSA28061
/// sets three.
fn set_string(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    match args {
        [0, ..] | [_, 0, ..] => u64::from(GuestError::Unimplemented.as_raw()),
        [_, _, 0, ..] => INVALID_VALUE,
        [object, key, value, ..] => {
            // SAFETY: the guest's value, a NUL-terminated string by the call's contract; read
            // through the checked accessor.
            if let Some(text) = unsafe { orbistoun_mem::guest::read_cstr(*value, 1024) } {
                let text = String::from_utf8_lossy(&text).into_owned();
                keep_property(*object, *key, Value::Text(text));
            }
            0
        }
    }
}

/// `sceNpUniversalDataSystemEventPropertyObjectSetInt32(object, key, value)`: keeps a 32-bit
/// integer at `key` and answers 0, as `SetString` answers a string (assumed: the setter of the
/// same family). PPSA04263 sets `_trophy_id` with it on the `_UnlockTrophy` event its trophies
/// are earned by. A null object or key answers the placeholder.
fn set_int32(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    match args {
        [0, ..] | [_, 0, ..] => u64::from(GuestError::Unimplemented.as_raw()),
        [object, key, value, ..] => {
            keep_property(*object, *key, Value::Int(i64::from(*value as u32 as i32)));
            0
        }
    }
}

/// The answer for a null object slot (REQ-rf02).
const INVALID_ARGUMENT: u64 = 0x8055_3102;

/// Bytes behind an event property object or an event. Assumed: the console's object is its own,
/// and nothing a title was seen to read from either directly; zeroed, as a fresh object holds
/// nothing.
const OBJECT_BYTES: usize = 0x100;

/// Writes a fresh object's address, all eight bytes, through `slot`, and answers 0, or
/// [`INVALID_ARGUMENT`] for a null slot: what both the property object and the event calls do.
fn new_object(slot: u64) -> u64 {
    if slot == 0 {
        return INVALID_ARGUMENT;
    }
    // Guest code runs in this address space, so a host block is a guest object. Leaked: the
    // destroy calls are not served, and the console's object outlives the call that made it.
    let object = Box::leak(vec![0_u8; OBJECT_BYTES].into_boxed_slice());
    let address = object.as_mut_ptr() as u64;
    // SAFETY: the guest's object slot, by the call's contract; written through the checked
    // accessor.
    if unsafe { orbistoun_mem::guest::write_u64(slot, address) } {
        0
    } else {
        u64::from(GuestError::Unimplemented.as_raw())
    }
}

/// `sceNpUniversalDataSystemCreateEventPropertyObject(&object)`: writes a new object's address,
/// all eight bytes, and answers 0; a null slot answers `0x80553102` (REQ-20261013T1930Z-rf02,
/// `000-boot/ux01-calls`, after `Initialize`, `CreateContext` and `CreateHandle`). PPSA02664 and
/// PPSA03416 call it once a frame; the five registers after the slot are left over from the
/// caller, not arguments.
///
/// Before `Initialize` the console answers `0x80553117` (REQ-pd02); orbistoun does not track
/// initialisation, and every title seen initialises first.
fn create_event_property_object(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    new_object(args[0])
}

/// `sceNpUniversalDataSystemCreateEvent(name, prop, &event, &own)`: writes a new event's address to
/// the first eight bytes of `event`, leaving the rest of the block as it was, and answers 0, with a
/// property object or a null one; a null `event` answers `0x80553102`. A second call gives another
/// event (REQ-20261014T0430Z-ue01, `000-boot/uds-create-event`, `report-20261014-batch4.log`
/// 2855-2863, with `own` null). PPSA02664 calls it once a frame.
///
/// A non-null `own` takes the event's own property object, holding `prop`'s values, for the title
/// to fill before posting (assumed, as the reference emulators read the fourth argument).
/// PPSA04263 creates its trophy events so - `CreateEvent(name, 0, &event, &own)` - fills `own`
/// and posts only if the fill succeeded (image+0x2d6912f-0x2d6918e).
fn create_event(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let answer = new_object(args[2]);
    if answer != 0 {
        return answer;
    }
    // SAFETY: the event slot `new_object` just wrote; read through the checked accessor.
    let event = unsafe { orbistoun_mem::guest::read_u64(args[2]) };
    // SAFETY: the guest's event name, a NUL-terminated string by the call's contract; read
    // through the checked accessor.
    let name = unsafe { orbistoun_mem::guest::read_cstr(args[0], 256) };
    let object = if args[3] == 0 {
        args[1]
    } else {
        if new_object(args[3]) != 0 {
            return u64::from(GuestError::Unimplemented.as_raw());
        }
        // SAFETY: the slot `new_object` just wrote; read through the checked accessor.
        let own = unsafe { orbistoun_mem::guest::read_u64(args[3]) }.unwrap_or(0);
        let mut state = kept();
        let seeded = state.objects.get(&args[1]).cloned().unwrap_or_default();
        state.objects.insert(own, seeded);
        own
    };
    if let (Some(event), Some(name)) = (event, name) {
        let name = String::from_utf8_lossy(&name).into_owned();
        kept().events.insert(event, (name, object));
    }
    0
}

/// What `sceNpUniversalDataSystemRegisterContext` answered for a context and handle just made,
/// both times it was called (REQ-ue01, `000-boot/uds-create-event` 2851-2852).
const REGISTER_CONTEXT_ANSWER: u64 = 0x8055_3104;

/// `sceNpUniversalDataSystemRegisterContext(ctx, handle, 0)`: answers `0x80553104`, as the console
/// did for the context and handle it had just made in a probe package with no universal-data
/// pack, a second time too; a title that ships one registers, and answers 0 (assumed). Every
/// commercial title in the corpus calls it.
fn register_context(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    if title_has_rules() {
        0
    } else {
        REGISTER_CONTEXT_ANSWER
    }
}

/// `sceNpUniversalDataSystemPostEvent(ctx, handle, event, 0)`: answers `0x80553104` for an event
/// `CreateEvent` made, as the console did in a probe package with no universal-data pack - the
/// answer `RegisterContext` gives on the same console (REQ-20261014T1110Z-rf05,
/// `000-boot/uds-create-event`, `report-20261014-batch6.log` 2181). A title that ships one has
/// the event counted into its statistics for the context's user, earning the trophies they meet,
/// and answers 0 (assumed). PPSA02664 posts an event every frame.
fn post_event(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    if !title_has_rules() {
        return REGISTER_CONTEXT_ANSWER;
    }
    let posted = {
        let state = kept();
        let owner = state.contexts.get(&(args[0] as u32)).copied();
        let event = state.events.get(&args[2]).map(|(name, object)| Event {
            name: name.clone(),
            properties: state.objects.get(object).cloned().unwrap_or_default(),
        });
        owner.zip(event)
    };
    if let Some(((user, label), event)) = posted {
        let earned = orbistoun_systemservice::trophy::post_event(user, label, &event);
        if kept().first_post(&event.name) {
            tracing::info!(
                event = %event.name,
                properties = ?event.properties,
                ?earned,
                "a universal-data event posted, the first of its name"
            );
        } else if !earned.is_empty() {
            tracing::info!(?earned, event = %event.name, "trophies earned");
        }
    }
    0
}

/// `sceNpUniversalDataSystemDestroyEvent(event)`: answers 0 (REQ-rf05, batch6 2182). The event's
/// block is not freed: [`new_object`] leaked it, as the console's object is its own.
const fn destroy_event(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    0
}

/// `sceNpUniversalDataSystemDestroyEventPropertyObject(object)`: answers 0 (REQ-rf05, batch6
/// 2183), its block kept as [`destroy_event`] keeps an event's.
const fn destroy_event_property_object(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    0
}

#[cfg(test)]
mod tests {
    use orbistoun_core::GUEST_ARG_REGISTERS;

    /// Held by every test whose answer depends on what `/app0` holds: the mounts are the
    /// process's.
    static APP0: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// `APP0`, held.
    fn app0() -> std::sync::MutexGuard<'static, ()> {
        APP0.lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// A trophy or universal-data pack holding `entries`, laid out as a title's is: a big-endian
    /// header, then 0x40-byte entries from 0x60 of a 32-byte name, an offset and a size.
    fn pack_of(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut pack = vec![0_u8; 0x60 + entries.len() * 0x40];
        pack[..4].copy_from_slice(&0xb228_c60a_u32.to_be_bytes());
        pack[0x10..0x14].copy_from_slice(&(entries.len() as u32).to_be_bytes());
        pack[0x14..0x18].copy_from_slice(&0x40_u32.to_be_bytes());
        for (index, (name, bytes)) in entries.iter().enumerate() {
            let at = 0x60 + index * 0x40;
            let offset = pack.len() as u64;
            pack[at..at + name.len()].copy_from_slice(name.as_bytes());
            pack[at + 0x20..at + 0x28].copy_from_slice(&offset.to_be_bytes());
            pack[at + 0x28..at + 0x30].copy_from_slice(&(bytes.len() as u64).to_be_bytes());
            pack.extend_from_slice(bytes);
        }
        pack
    }

    /// Each event name is reported the first time it is posted, and only then.
    #[test]
    fn an_event_name_is_reported_once() {
        let mut kept = super::Kept::default();
        assert!(kept.first_post("_UnlockTrophy"));
        assert!(!kept.first_post("_UnlockTrophy"));
        assert!(kept.first_post("ActivityStart"));
    }

    /// In a title that ships its trophy and universal-data packs, an `_UnlockTrophy` event whose
    /// `_trophy_id` an `Int32` setter put on its property object registers, posts with 0, and earns
    /// the trophy into the user's kept record.
    #[test]
    fn a_posted_unlock_event_earns_its_trophy() {
        let _alone = app0();
        let root = std::env::temp_dir().join(format!("orbistoun-uds-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let (app, data) = (root.join("app0"), root.join("data"));
        std::fs::create_dir_all(app.join("sce_sys/trophy2")).unwrap();
        std::fs::create_dir_all(app.join("sce_sys/uds")).unwrap();
        let conf = br#"{"trophyNpCommId":"NPWR99999_00","defaultLanguage":"en-US",
            "trophies":[{"id":"0001","grade":"B",
              "unlockCondition":{"udsStatId":"19","comparator":"ge","targetValue":"1"}}]}"#;
        let meta = br#"{"metadata":{"titleMetadata":{"name":"T"},"trophyMetadata":[]}}"#;
        std::fs::write(
            app.join("sce_sys/trophy2/trophy00.ucp"),
            pack_of(&[("tropconf.json", conf), ("tropmeta_en-US.json", meta)]),
        )
        .unwrap();
        let definitions =
            br#"{"statDefinitionArray":[{"statId":19,"aggregation":"count","maxValue":"1"}]}"#;
        let extraction =
            br#"{"statsExtractionRuleArray":[{"condition":{"eventName":"_UnlockTrophy",
            "property":{"path":"$._trophy_id","comparator":"==","value":"1"}},
            "action":{"output":{"statId":19}}}]}"#;
        std::fs::write(
            app.join("sce_sys/uds/uds00.ucp"),
            pack_of(&[
                ("stats_definition.json", definitions),
                ("stats_extraction.json", extraction),
            ]),
        )
        .unwrap();
        orbistoun_fs::mount::mount("/app0", app);
        orbistoun_fs::mount::mount_data(data.clone());
        orbistoun_fs::mount::allow_writes(orbistoun_fs::mount::DATA_MOUNT);

        let user = orbistoun_systemservice::signed_in_user();
        let (mut context, mut object, mut event) = (0_u64, 0_u64, [0_u64; 4]);
        let at = |slot: &mut u64| std::ptr::from_mut(slot) as u64;
        let mut args = [0_u64; GUEST_ARG_REGISTERS];
        args[..3].copy_from_slice(&[at(&mut context), u64::from(user), 0]);
        assert_eq!(super::create_context(&args), 0);
        assert_eq!(super::register_context(&[context, 0x2_0176, 0, 0, 0, 0]), 0);
        // As PPSA04263 makes it: no property object passed, the event's own taken and filled.
        let name = c"_UnlockTrophy";
        let event_at = event.as_mut_ptr() as u64;
        let own_at = at(&mut object);
        assert_eq!(
            super::create_event(&[name.as_ptr() as u64, 0, event_at, own_at, 0, 0]),
            0
        );
        assert_ne!(object, 0, "the event's own property object");
        let key = c"_trophy_id";
        assert_eq!(
            super::set_int32(&[object, key.as_ptr() as u64, 1, 0, 0, 0]),
            0
        );
        assert_eq!(
            super::post_event(&[context, 0x2_0176, event[0], 0, 0, 0]),
            0
        );

        let record = data.join(format!("sce_trophy2/NPWR99999_00/user-{user}.json"));
        let kept = std::fs::read_to_string(&record).expect("the earned trophy is kept");
        assert!(kept.contains(r#""earned":{"1":"#), "{kept}");
        orbistoun_fs::mount::unmount("/app0");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Handles count up from `0x20176` in the low four bytes, the rest of the slot as it was
    /// (REQ-pu01).
    #[test]
    fn handles_count_up_from_the_measured_first() {
        let mut slot = [0xcc_u8; 8];
        let mut args = [0_u64; GUEST_ARG_REGISTERS];
        args[0] = slot.as_mut_ptr() as u64;
        assert_eq!(super::create_handle(&args), 0);
        assert_eq!(slot, [0x76, 0x01, 0x02, 0x00, 0xcc, 0xcc, 0xcc, 0xcc]);
        assert_eq!(super::create_handle(&args), 0);
        assert_eq!(slot[..4], [0x77, 0x01, 0x02, 0x00]);
    }

    /// A new object's address fills all eight bytes of the slot, and a null slot is refused
    /// (REQ-rf02).
    #[test]
    fn event_property_objects_are_written_through_the_slot() {
        let mut slot = [0xcc_u8; 16];
        let mut args = [0_u64; GUEST_ARG_REGISTERS];
        args[0] = slot.as_mut_ptr() as u64;
        assert_eq!(super::create_event_property_object(&args), 0);
        let object = u64::from_le_bytes(slot[..8].try_into().unwrap());
        assert_ne!(object, 0);
        assert_eq!(slot[8..], [0xcc; 8]);
        assert_eq!(super::create_event_property_object(&args), 0);
        assert_ne!(u64::from_le_bytes(slot[..8].try_into().unwrap()), object);
        args[0] = 0;
        assert_eq!(super::create_event_property_object(&args), 0x8055_3102);
    }

    /// `CreateEvent(name, prop, &event, 0)` writes a new event's address to the first eight bytes
    /// of `event`, with or without a property object, and refuses a null slot (REQ-ue01). A repeat
    /// gives another event.
    #[test]
    fn events_are_written_through_the_third_argument() {
        let mut event = [0xcc_u8; 0x20];
        let mut args = [0_u64; GUEST_ARG_REGISTERS];
        args[0] = c"activityTerminate".as_ptr() as u64;
        args[1] = 0x1000;
        args[2] = event.as_mut_ptr() as u64;
        assert_eq!(super::create_event(&args), 0);
        let first = u64::from_le_bytes(event[..8].try_into().unwrap());
        assert_ne!(first, 0);
        assert_eq!(
            event[8..],
            [0xcc; 0x18],
            "the rest of the block is untouched"
        );
        args[1] = 0;
        assert_eq!(
            super::create_event(&args),
            0,
            "a null property object is accepted"
        );
        assert_ne!(u64::from_le_bytes(event[..8].try_into().unwrap()), first);
        args[2] = 0;
        assert_eq!(super::create_event(&args), 0x8055_3102);
    }

    /// An event made by `CreateEvent` is posted with `0x80553104`, as `RegisterContext` answers on
    /// the same console, and destroyed with 0, as is its property object (REQ-rf05).
    #[test]
    fn events_are_posted_and_destroyed_as_measured() {
        let _alone = app0();
        let mut args = [0_u64; GUEST_ARG_REGISTERS];
        args[0] = 0x4_007b;
        args[1] = 0x2_0098;
        args[2] = 0x1000;
        assert_eq!(super::post_event(&args), 0x8055_3104);
        args[0] = 0x1000;
        assert_eq!(super::destroy_event(&args), 0);
        assert_eq!(super::destroy_event_property_object(&args), 0);
    }

    /// `EventPropertyObjectSetString(object, key, value)` answers 0, again over the same key, and
    /// `0x80553115` for a null value; `DestroyHandle(handle)` answers 0, then `0x80553106` for the
    /// same handle (REQ-ud01, `report-20261015-batch1.log` 2027-2032).
    #[test]
    fn strings_are_set_and_handles_destroyed_as_measured() {
        let key = c"activityId";
        let mut args = [0_u64; GUEST_ARG_REGISTERS];
        args[0] = 0x1000;
        args[1] = key.as_ptr() as u64;
        args[2] = c"a1".as_ptr() as u64;
        assert_eq!(super::set_string(&args), 0);
        args[2] = c"a2".as_ptr() as u64;
        assert_eq!(super::set_string(&args), 0);
        args[2] = 0;
        assert_eq!(super::set_string(&args), 0x8055_3115);

        let mut handle = 0_u64;
        let mut args = [0_u64; GUEST_ARG_REGISTERS];
        args[0] = std::ptr::from_mut(&mut handle).expose_provenance() as u64;
        assert_eq!(super::create_handle(&args), 0);
        args[0] = handle & 0xffff_ffff;
        assert_eq!(super::destroy_handle(&args), 0);
        assert_eq!(super::destroy_handle(&args), 0x8055_3106);
    }

    /// `RegisterContext(ctx, handle, 0)` answers `0x80553104` on the console, a second time too
    /// (REQ-ue01).
    #[test]
    fn registering_a_context_answers_as_measured() {
        let _alone = app0();
        let mut args = [0_u64; GUEST_ARG_REGISTERS];
        args[0] = 0x4_007b;
        args[1] = 0x2_0098;
        assert_eq!(super::register_context(&args), 0x8055_3104);
        assert_eq!(super::register_context(&args), 0x8055_3104);
    }
}
