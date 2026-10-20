//! Event queues: what `sceKernelCreateEqueue` makes and `sceKernelWaitEqueue` waits on (D930).
//!
//! `sceKernelWaitEqueue` is `kevent(2)` on a FreeBSD-derived kernel, so a queue keeps its events as
//! FreeBSD's kqueue keeps them (`sys/kern/kern_event.c`). An event a guest registers is a knote
//! keyed by filter and identifier. A queue holds at most one knote per key, and the knote is on the
//! ready list at most once, so repeated triggers coalesce. `EV_CLEAR` resets it once delivered,
//! `EV_ONESHOT` drops it, `EV_DISPATCH` disables it, and a knote with none of these stays ready and
//! is delivered again by every wait.
//!
//! A subsystem that completes work for the guest (a flip, an end-of-pipe release) registers a
//! route instead and posts finished events to it. Those events are delivered once each, in the
//! order they were posted, among the knotes that became ready around them.

use std::collections::{BTreeMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, OnceLock};
use std::time::Instant;

use crate::sync::Blocking;

/// What a guest holds for an event queue.
pub type EqueueHandle = u64;

/// The user-event filter, `EVFILT_USER` (`sys/sys/event.h:45`). PPSA25872 compares a delivered
/// event's filter against -11 beside -13, the measured video-out filter (image+0x11c4200).
pub const EVFILT_USER: i16 = -11;

/// Add the event (`sys/sys/event.h:138`).
pub const EV_ADD: u16 = 0x0001;
/// Remove it (`event.h:139`).
pub const EV_DELETE: u16 = 0x0002;
/// Let it be reported (`event.h:140`).
pub const EV_ENABLE: u16 = 0x0004;
/// Keep it but report nothing (`event.h:141`).
pub const EV_DISABLE: u16 = 0x0008;
/// Report one occurrence, then drop the event (`event.h:146`).
pub const EV_ONESHOT: u16 = 0x0010;
/// Reset the event's state once it has been reported (`event.h:147`).
pub const EV_CLEAR: u16 = 0x0020;
/// Disable the event once it has been reported (`event.h:149`).
pub const EV_DISPATCH: u16 = 0x0080;
/// Turn on `EV_ONESHOT` and report the event now (`event.h:142`).
pub const EV_FORCEONESHOT: u16 = 0x0100;
/// Keep the registered `udata` rather than take the change's (`event.h:143`).
pub const EV_KEEPUDATA: u16 = 0x0200;

/// The flags that are actions on a registration and are never stored with it
/// (`kern_event.c:1842-1843`).
const ACTIONS: u16 = EV_ADD | EV_DELETE | EV_ENABLE | EV_DISABLE | EV_FORCEONESHOT;

/// The `fflags` bit that triggers a user event (`sys/sys/event.h:175`).
pub const NOTE_TRIGGER: u32 = 0x0100_0000;
/// AND the low twenty-four bits into the stored `fflags` (`event.h:168`).
pub const NOTE_FFAND: u32 = 0x4000_0000;
/// OR them in (`event.h:169`).
pub const NOTE_FFOR: u32 = 0x8000_0000;
/// Replace the stored `fflags` with them (`event.h:170`).
pub const NOTE_FFCOPY: u32 = 0xc000_0000;
/// The two bits that say how a change's `fflags` apply (`event.h:171`).
const NOTE_FFCTRLMASK: u32 = 0xc000_0000;
/// The bits a change's `fflags` carry as a value (`event.h:172`).
const NOTE_FFLAGSMASK: u32 = 0x00ff_ffff;

/// One event, in the fields a guest reads back out of a delivered one.
///
/// The layout is FreeBSD's `struct kevent` (D560): `ident` u64 at 0x00, `filter` i16 at 0x08,
/// `flags` u16 at 0x0a, `fflags` u32 at 0x0c, `data` i64 at 0x10, `udata` u64 at 0x18. FreeBSD 12
/// appended `uint64_t ext[4]`, making it 0x40 bytes; the fields here are common to both, and
/// [`EVENT_BYTES`] uses the 0x20 form.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PendingEvent {
    /// What the event is about - for a flip, the video-out port handle.
    pub ident: u64,
    /// Which kind of event this is.
    pub filter: i16,
    /// Action flags.
    pub flags: u16,
    /// Filter-specific flags.
    pub fflags: u32,
    /// Filter-specific data.
    pub data: i64,
    /// The opaque word the caller supplied when it registered.
    pub udata: u64,
    /// The logical time the event stands for, in nanoseconds since the guest started, or 0 for an
    /// event that carries none. A flip completion carries the display's time, and the thread whose
    /// wait takes it moves its clock up to it (D786). Not part of what the guest reads.
    pub display_nanos: u64,
}

/// How many bytes one delivered event occupies: the pre-FreeBSD-12 `struct kevent`.
///
/// The 0x40-byte form is the alternative if a guest reads past 0x20.
pub const EVENT_BYTES: usize = 0x20;

impl PendingEvent {
    /// The event as the guest reads it, in the layout [`PendingEvent`] documents.
    #[must_use]
    pub fn to_bytes(self) -> [u8; EVENT_BYTES] {
        let mut out = [0u8; EVENT_BYTES];
        out[0x00..0x08].copy_from_slice(&self.ident.to_le_bytes());
        out[0x08..0x0a].copy_from_slice(&self.filter.to_le_bytes());
        out[0x0a..0x0c].copy_from_slice(&self.flags.to_le_bytes());
        out[0x0c..0x10].copy_from_slice(&self.fflags.to_le_bytes());
        out[0x10..0x18].copy_from_slice(&self.data.to_le_bytes());
        out[0x18..0x20].copy_from_slice(&self.udata.to_le_bytes());
        out
    }
}

/// One change a guest asks of a queue: FreeBSD's `struct kevent` as `kevent(2)` takes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Change {
    /// Which event.
    pub ident: u64,
    /// Of which filter.
    pub filter: i16,
    /// The actions and the flags to store.
    pub flags: u16,
    /// The filter's flags; for a user event, `NOTE_TRIGGER` and the `NOTE_FF*` controls.
    pub fflags: u32,
    /// The filter's data.
    pub data: i64,
    /// The caller's opaque word.
    pub udata: u64,
}

/// Why a queue refused a change or a wait.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// The handle names no queue, or names one deleted while the caller waited on it.
    NoQueue,
    /// No such event is registered and the change did not add one: FreeBSD's `ENOENT`
    /// (`kern_event.c:1873-1876`).
    NoEvent,
    /// The change is malformed, or names a filter orbistoun does not keep: FreeBSD's `EINVAL`
    /// (`kern_event.c:1675-1676, 1687-1688, 1692-1695`).
    Invalid,
    /// Nothing became ready within the caller's patience.
    TimedOut,
}

/// A registered event: FreeBSD's knote, in the fields the filters orbistoun keeps read.
#[derive(Debug, Clone, Copy)]
struct Knote {
    /// The flags it was added with, less the actions (`kern_event.c:1841-1843`).
    flags: u16,
    /// The filter's stored flags, FreeBSD's `kn_sfflags`.
    fflags: u32,
    /// The filter's stored data, FreeBSD's `kn_sdata`.
    data: i64,
    /// The caller's opaque word.
    udata: u64,
    /// Whether the filter reports it: for a user event, FreeBSD's `kn_hookid`.
    fired: bool,
    /// Kept but not reported, FreeBSD's `KN_DISABLED`.
    disabled: bool,
    /// On the ready list, FreeBSD's `KN_QUEUED`.
    queued: bool,
}

/// Where a subsystem's completions go: a plain identifier, or one of a filter's identifiers, kept
/// apart so a subsystem posting by filter reaches only what registered for that filter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Route {
    /// Registered by identifier alone, as `sceVideoOutAddFlipEvent` registers a port.
    Ident(u64),
    /// Registered under a filter, as `sceAgcDriverAddEqEvent` registers an end-of-pipe id.
    Filter(i16, u64),
}

/// What the ready list holds, oldest first.
#[derive(Debug, Clone, Copy)]
enum Ready {
    /// A subsystem's completion, delivered once.
    Posted(Route, PendingEvent),
    /// A knote, by its key; it is read when delivered.
    Knote(i16, u64),
}

/// A queue's state, under one lock so a change and a wait see the same thing.
#[derive(Debug, Default)]
struct State {
    /// Subsystem routes with the caller's `udata`, in the order they were registered.
    routes: Vec<(Route, u64)>,
    /// Registered events by filter and identifier.
    knotes: BTreeMap<(i16, u64), Knote>,
    /// What a wait delivers, oldest first.
    ready: VecDeque<Ready>,
}

/// A guest event queue: a named place events are delivered to and waited on.
struct GuestEqueue {
    name: String,
    state: Mutex<State>,
    /// Set when the queue is deleted, so a thread waiting on it stops waiting.
    deleted: AtomicBool,
    /// Waits begun against this queue.
    ///
    /// A queue waited on and never posted to is a stuck thread, and without this count it looks like
    /// an unused queue.
    waited: AtomicU64,
    /// Events actually handed to a caller.
    delivered: AtomicU64,
    /// Signalled when something becomes ready or the queue is deleted.
    arrived: Condvar,
}

impl GuestEqueue {
    fn state(&self) -> Option<MutexGuard<'_, State>> {
        self.state.lock().ok()
    }

    /// Adds `ready` to the list and wakes the queue's waiters.
    fn push(&self, state: &mut State, ready: Ready) {
        state.ready.push_back(ready);
        self.arrived.notify_all();
    }
}

fn equeues() -> &'static Mutex<BTreeMap<EqueueHandle, Arc<GuestEqueue>>> {
    static TABLE: OnceLock<Mutex<BTreeMap<EqueueHandle, Arc<GuestEqueue>>>> = OnceLock::new();
    TABLE.get_or_init(|| Mutex::new(BTreeMap::new()))
}

fn queue(handle: EqueueHandle) -> Option<Arc<GuestEqueue>> {
    equeues().lock().ok()?.get(&handle).map(Arc::clone)
}

/// Creates an event queue under a caller-supplied name.
pub fn create_equeue(name: &str) -> EqueueHandle {
    let handle = crate::sync::new_handle();
    if let Ok(mut table) = equeues().lock() {
        table.insert(
            handle,
            Arc::new(GuestEqueue {
                name: name.to_owned(),
                state: Mutex::new(State::default()),
                deleted: AtomicBool::new(false),
                waited: AtomicU64::new(0),
                delivered: AtomicU64::new(0),
                arrived: Condvar::new(),
            }),
        );
    }
    handle
}

/// Deletes a queue, answering whether there was one.
///
/// Its events go with it, as FreeBSD's `kqueue_drain` drops every knote (`kern_event.c:2429-2440`).
/// A thread waiting on it is woken and told the queue is gone.
pub fn delete_equeue(handle: EqueueHandle) -> bool {
    let Some(queue) = equeues().lock().ok().and_then(|mut t| t.remove(&handle)) else {
        return false;
    };
    queue.deleted.store(true, Ordering::Release);
    if let Some(mut state) = queue.state() {
        *state = State::default();
    }
    queue.arrived.notify_all();
    true
}

/// Whether a queue exists at all, so a wait on a handle nobody was given can be refused.
#[must_use]
pub fn equeue_exists(handle: EqueueHandle) -> bool {
    equeues().lock().is_ok_and(|t| t.contains_key(&handle))
}

/// Applies one change to a queue, as FreeBSD's `kqueue_register` does (`kern_event.c:1665-1952`).
///
/// # Errors
///
/// [`Refusal::NoQueue`] for a handle naming no queue, [`Refusal::NoEvent`] for a change to an
/// event nobody added, and [`Refusal::Invalid`] for a malformed change or a filter orbistoun does
/// not keep as knotes.
pub fn apply(handle: EqueueHandle, change: Change) -> Result<(), Refusal> {
    let queue = queue(handle).ok_or(Refusal::NoQueue)?;
    let mut state = queue.state().ok_or(Refusal::NoQueue)?;
    let result = register(&mut state, change);
    if result.is_ok() && matches!(state.ready.back(), Some(Ready::Knote(..))) {
        queue.arrived.notify_all();
    }
    result
}

/// The knote half of [`apply`], on the state it locked.
fn register(state: &mut State, change: Change) -> Result<(), Refusal> {
    // Both at once is refused before anything is looked up (`kern_event.c:1675-1676`).
    if change.flags & (EV_ENABLE | EV_DISABLE) == EV_ENABLE | EV_DISABLE {
        return Err(Refusal::Invalid);
    }
    // Only the user filter is kept as knotes; any other is a filter not modelled here.
    if change.filter != EVFILT_USER {
        return Err(Refusal::Invalid);
    }
    // `kern_event.c:1692-1695`.
    if change.flags & EV_ADD != 0 && change.flags & EV_KEEPUDATA != 0 {
        return Err(Refusal::Invalid);
    }
    let key = (change.filter, change.ident);
    let Some(knote) = state.knotes.get_mut(&key) else {
        if change.flags & EV_ADD == 0 {
            return Err(Refusal::NoEvent);
        }
        // A new knote stores the change's flags less the actions and its fflags and data as
        // given; `filt_userattach` fires it when the change triggers (`kern_event.c:1837-1846,
        // 1119-1131`).
        let knote = Knote {
            flags: change.flags & !ACTIONS,
            fflags: change.fflags,
            data: change.data,
            udata: change.udata,
            fired: change.fflags & NOTE_TRIGGER != 0,
            disabled: change.flags & EV_DISABLE != 0,
            queued: false,
        };
        state.knotes.insert(key, knote);
        enqueue_if_ready(state, key);
        return Ok(());
    };
    if change.flags & EV_DELETE != 0 {
        state.knotes.remove(&key);
        state
            .ready
            .retain(|r| !matches!(r, Ready::Knote(f, i) if (*f, *i) == key));
        return Ok(());
    }
    // `kern_event.c:1887-1890`.
    if change.flags & EV_FORCEONESHOT != 0 {
        knote.flags |= EV_ONESHOT;
        knote.fired = true;
    }
    if change.flags & EV_ENABLE != 0 {
        knote.disabled = false;
    } else if change.flags & EV_DISABLE != 0 {
        knote.disabled = true;
    }
    if change.flags & EV_KEEPUDATA == 0 {
        knote.udata = change.udata;
    }
    touch_user(knote, change);
    enqueue_if_ready(state, key);
    Ok(())
}

/// `filt_usertouch` for a registration (`kern_event.c:1155-1187`): a trigger fires the event, the
/// `NOTE_FF*` control applies the change's low twenty-four bits to the stored flags, the data is
/// replaced, and a change carrying `EV_CLEAR` resets it.
fn touch_user(knote: &mut Knote, change: Change) {
    if change.fflags & NOTE_TRIGGER != 0 {
        knote.fired = true;
    }
    let value = change.fflags & NOTE_FFLAGSMASK;
    match change.fflags & NOTE_FFCTRLMASK {
        NOTE_FFAND => knote.fflags &= value,
        NOTE_FFOR => knote.fflags |= value,
        NOTE_FFCOPY => knote.fflags = value,
        // `NOTE_FFNOP`.
        _ => {}
    }
    knote.data = change.data;
    if change.flags & EV_CLEAR != 0 {
        knote.fired = false;
    }
}

/// Puts a knote that the filter reports on the ready list once (`kern_event.c:1926-1936`).
fn enqueue_if_ready(state: &mut State, key: (i16, u64)) {
    let Some(knote) = state.knotes.get_mut(&key) else {
        return;
    };
    if knote.fired && !knote.disabled && !knote.queued {
        knote.queued = true;
        state.ready.push_back(Ready::Knote(key.0, key.1));
    }
}

/// Takes up to `wanted` events off the ready list, as FreeBSD's `kqueue_scan` does
/// (`kern_event.c:2186-2306`): each entry is visited at most once, a disabled knote leaves the list,
/// one the filter no longer reports leaves it, a one-shot knote is delivered and dropped, a
/// clearing or dispatching one is delivered and leaves it, and any other is delivered and goes to
/// the back to be delivered again by the next wait.
fn scan(state: &mut State, wanted: usize) -> Vec<PendingEvent> {
    let mut out = Vec::new();
    // FreeBSD's marker: what was on the list when the scan began.
    let mut visit = state.ready.len();
    while visit > 0 && out.len() < wanted {
        visit -= 1;
        let Some(ready) = state.ready.pop_front() else {
            break;
        };
        let (filter, ident) = match ready {
            Ready::Posted(_, event) => {
                out.push(event);
                continue;
            }
            Ready::Knote(filter, ident) => (filter, ident),
        };
        let Some(knote) = state.knotes.get_mut(&(filter, ident)) else {
            continue;
        };
        knote.queued = false;
        if knote.disabled {
            continue;
        }
        if knote.flags & EV_ONESHOT != 0 {
            // The knote's own kevent, whose fflags and data were zeroed when it was added
            // (`kern_event.c:1839-1841, 2229-2241`).
            out.push(PendingEvent {
                ident,
                filter,
                flags: knote.flags,
                fflags: 0,
                data: 0,
                udata: knote.udata,
                display_nanos: 0,
            });
            state.knotes.remove(&(filter, ident));
            continue;
        }
        if !knote.fired {
            continue;
        }
        // `filt_usertouch` for a delivery (`kern_event.c:1189-1198`).
        out.push(PendingEvent {
            ident,
            filter,
            flags: knote.flags,
            fflags: knote.fflags,
            data: knote.data,
            udata: knote.udata,
            display_nanos: 0,
        });
        if knote.flags & EV_CLEAR != 0 {
            knote.fired = false;
        }
        if knote.flags & (EV_CLEAR | EV_DISPATCH) != 0 {
            if knote.flags & EV_DISPATCH != 0 {
                knote.disabled = true;
            }
        } else {
            knote.queued = true;
            state.ready.push_back(Ready::Knote(filter, ident));
        }
    }
    out
}

/// Collects up to `wanted` events, oldest first, waiting for the first one as `until` allows.
///
/// `kevent(2)` blocks until at least one event is ready or the timeout elapses, so a guest looping
/// until an event arrives blocks rather than spins.
///
/// # Errors
///
/// [`Refusal::NoQueue`] for a handle naming no queue, or one deleted during the wait, and
/// [`Refusal::TimedOut`] when nothing became ready in time.
pub fn wait_events(
    handle: EqueueHandle,
    wanted: usize,
    until: Blocking,
) -> Result<Vec<PendingEvent>, Refusal> {
    let queue = queue(handle).ok_or(Refusal::NoQueue)?;
    // Counted before the wait, so a wait that never returns is still counted.
    queue.waited.fetch_add(1, Ordering::Relaxed);
    let mut state = queue.state().ok_or(Refusal::NoQueue)?;
    loop {
        if queue.deleted.load(Ordering::Acquire) {
            return Err(Refusal::NoQueue);
        }
        let events = scan(&mut state, wanted);
        if !events.is_empty() {
            queue
                .delivered
                .fetch_add(events.len() as u64, Ordering::Relaxed);
            return Ok(events);
        }
        state = match until {
            Blocking::Never => return Err(Refusal::TimedOut),
            Blocking::Forever => queue.arrived.wait(state).map_err(|_| Refusal::NoQueue)?,
            Blocking::Until(deadline) => {
                let now = Instant::now();
                if now >= deadline {
                    return Err(Refusal::TimedOut);
                }
                // Recomputed from the fixed deadline each turn, so an empty wake does not restart it.
                queue
                    .arrived
                    .wait_timeout(state, deadline - now)
                    .map_err(|_| Refusal::NoQueue)?
                    .0
            }
        };
    }
}

/// Collects up to `wanted` events from a queue, oldest first, without blocking.
///
/// Empty when the queue has none, which is not an error: `kevent` with nothing ready reports
/// zero delivered.
pub fn take_events(handle: EqueueHandle, wanted: usize) -> Vec<PendingEvent> {
    wait_events(handle, wanted, Blocking::Never).unwrap_or_default()
}

/// Adds a route to a queue, answering whether the queue exists. A route already there keeps its
/// place and takes the new `udata`, as a second `EV_ADD` updates a knote rather than adding one.
fn add_route(handle: EqueueHandle, route: Route, udata: u64) -> bool {
    let Some(queue) = queue(handle) else {
        return false;
    };
    let Some(mut state) = queue.state() else {
        return false;
    };
    match state.routes.iter_mut().find(|(r, _)| *r == route) {
        Some(entry) => entry.1 = udata,
        None => state.routes.push((route, udata)),
    }
    true
}

/// Removes a route and any completion posted to it and not yet collected, as `EV_DELETE` drops a
/// knote off the ready list. Answers whether the route was there.
fn remove_route(handle: EqueueHandle, route: Route) -> bool {
    let Some(queue) = queue(handle) else {
        return false;
    };
    let Some(mut state) = queue.state() else {
        return false;
    };
    let before = state.routes.len();
    state.routes.retain(|(r, _)| *r != route);
    state
        .ready
        .retain(|ready| !matches!(ready, Ready::Posted(r, _) if *r == route));
    state.routes.len() != before
}

/// Registers `identifier` against a queue, answering whether the queue exists.
///
/// A registration against a queue nobody created is refused, rather than reporting success for
/// a queue that will never deliver anything (D524).
pub fn register_event(handle: EqueueHandle, identifier: u64) -> bool {
    register_event_with_udata(handle, identifier, 0)
}

/// The same, carrying the caller's opaque word so a delivery can echo it.
pub fn register_event_with_udata(handle: EqueueHandle, identifier: u64, udata: u64) -> bool {
    add_route(handle, Route::Ident(identifier), udata)
}

/// Removes what [`register_event_with_udata`] registered, answering whether it was there.
pub fn unregister_event(handle: EqueueHandle, identifier: u64) -> bool {
    remove_route(handle, Route::Ident(identifier))
}

/// Registers `filter`'s event `id` against a queue, carrying the caller's opaque word, answering
/// whether the queue exists. Kept apart from plain identifiers, so a subsystem that posts by
/// filter ([`post_filtered_event`]) reaches only what registered for it.
pub fn register_filtered_event(handle: EqueueHandle, filter: i16, id: u64, udata: u64) -> bool {
    add_route(handle, Route::Filter(filter, id), udata)
}

/// Removes what [`register_filtered_event`] registered, answering whether it was there.
pub fn unregister_filtered_event(handle: EqueueHandle, filter: i16, id: u64) -> bool {
    remove_route(handle, Route::Filter(filter, id))
}

/// Posts `event(id)` to every queue that registered one of `filter`'s events, for each `id` it
/// registered, echoing its `udata`; answers how many took one.
pub fn post_filtered_event(filter: i16, event: impl Fn(u64) -> PendingEvent) -> usize {
    let Ok(table) = equeues().lock() else {
        return 0;
    };
    let mut posted = 0;
    for queue in table.values() {
        let Some(mut state) = queue.state() else {
            continue;
        };
        let mine: Vec<(Route, u64)> = state
            .routes
            .iter()
            .filter(|(route, _)| matches!(route, Route::Filter(f, _) if *f == filter))
            .copied()
            .collect();
        for (route, udata) in mine {
            let Route::Filter(_, id) = route else {
                continue;
            };
            queue.push(
                &mut state,
                Ready::Posted(route, PendingEvent { udata, ..event(id) }),
            );
            posted += 1;
        }
    }
    posted
}

/// Posts an event to every queue that registered `ident`, answering how many took it.
///
/// Routing by registered identifier lets the completing subsystem (a flip, for example) post
/// without knowing which queue a title waits on. Zero means nothing had registered, so the
/// completion had no reader.
pub fn post_event(ident: u64, event: PendingEvent) -> usize {
    let Ok(table) = equeues().lock() else {
        return 0;
    };
    let route = Route::Ident(ident);
    let mut posted = 0;
    for queue in table.values() {
        let Some(mut state) = queue.state() else {
            continue;
        };
        let Some(&(_, udata)) = state.routes.iter().find(|(r, _)| *r == route) else {
            continue;
        };
        // The caller's own word, echoed back the way `kevent` echoes it.
        queue.push(
            &mut state,
            Ready::Posted(route, PendingEvent { udata, ..event }),
        );
        posted += 1;
    }
    posted
}

/// Posts an event to every queue, ignoring what is registered.
///
/// Only for the `ORBISTOUN_FLIP_TO_ALL` diagnostic, which asks what a guest blocked on a queue
/// nothing feeds would do if that wait completed. Returns how many queues it reached, like
/// [`post_event`].
pub fn post_event_everywhere(event: PendingEvent) -> usize {
    let Ok(table) = equeues().lock() else {
        return 0;
    };
    let mut posted = 0;
    for queue in table.values() {
        let Some(mut state) = queue.state() else {
            continue;
        };
        // The first route's `udata` where there is one; a queue with none gets the event as given.
        let (route, udata) = state
            .routes
            .first()
            .copied()
            .unwrap_or((Route::Ident(event.ident), event.udata));
        queue.push(
            &mut state,
            Ready::Posted(route, PendingEvent { udata, ..event }),
        );
        posted += 1;
    }
    posted
}

/// What a queue is called and what has been registered against it, for a run report.
#[must_use]
pub fn equeue_summary() -> Vec<EqueueTraffic> {
    let Ok(table) = equeues().lock() else {
        return Vec::new();
    };
    table
        .iter()
        .map(|(handle, q)| EqueueTraffic {
            handle: *handle,
            name: q.name.clone(),
            registered: q.state().map_or(0, |s| s.routes.len() + s.knotes.len()),
            waited: q.waited.load(Ordering::Relaxed),
            delivered: q.delivered.load(Ordering::Relaxed),
        })
        .collect()
}

/// What one event queue was used for, for a run report.
///
/// Registrations with no waits is a queue set up and unused; waits with no deliveries is a stuck
/// thread; both is a working queue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EqueueTraffic {
    /// The handle the guest holds, which is what every argument dump shows, so the two records join.
    pub handle: EqueueHandle,
    /// What the guest called it.
    pub name: String,
    /// Events registered against it.
    pub registered: usize,
    /// Waits begun on it.
    pub waited: u64,
    /// Events handed to a caller out of it.
    pub delivered: u64,
}

#[cfg(test)]
mod tests {
    use super::{
        Blocking, Change, EV_ADD, EV_CLEAR, EV_DELETE, EV_DISABLE, EV_DISPATCH, EV_ENABLE,
        EV_ONESHOT, EVFILT_USER, NOTE_FFAND, NOTE_FFCOPY, NOTE_FFOR, NOTE_TRIGGER, PendingEvent,
        Refusal,
    };

    /// A user-event change with nothing but these set.
    fn user(ident: u64, actions: u16, note: u32, udata: u64) -> Change {
        Change {
            ident,
            filter: EVFILT_USER,
            flags: actions,
            fflags: note,
            data: 0,
            udata,
        }
    }

    fn trigger(queue: u64, ident: u64, udata: u64) -> Result<(), Refusal> {
        super::apply(queue, user(ident, 0, NOTE_TRIGGER, udata))
    }

    fn idents(events: &[PendingEvent]) -> Vec<u64> {
        events.iter().map(|e| e.ident).collect()
    }

    /// A delivered event puts each field where FreeBSD's `struct kevent` puts it.
    ///
    /// Asserted field by field at its offset, since a round trip through this project's own encoder
    /// would agree with itself whatever the offsets were. It pins what orbistoun writes against the
    /// reference, not that the hardware agrees.
    #[test]
    fn a_delivered_event_matches_the_published_kevent_layout() {
        let bytes = PendingEvent {
            ident: 0x1122_3344_5566_7788,
            filter: -3,
            flags: 0xbeef,
            fflags: 0xdead_0001,
            data: -2,
            udata: 0x0102_0304_0506_0708,
            display_nanos: 0,
        }
        .to_bytes();

        assert_eq!(&bytes[0x00..0x08], &0x1122_3344_5566_7788u64.to_le_bytes());
        assert_eq!(&bytes[0x08..0x0a], &(-3i16).to_le_bytes(), "filter at 0x08");
        assert_eq!(
            &bytes[0x0a..0x0c],
            &0xbeefu16.to_le_bytes(),
            "flags at 0x0a"
        );
        assert_eq!(&bytes[0x0c..0x10], &0xdead_0001u32.to_le_bytes());
        assert_eq!(&bytes[0x10..0x18], &(-2i64).to_le_bytes(), "data at 0x10");
        assert_eq!(&bytes[0x18..0x20], &0x0102_0304_0506_0708u64.to_le_bytes());
        assert_eq!(bytes.len(), super::EVENT_BYTES);
    }

    fn completion(ident: u64, data: i64, udata: u64) -> PendingEvent {
        PendingEvent {
            ident,
            filter: 0,
            flags: 0,
            fflags: 0,
            data,
            udata,
            display_nanos: 0,
        }
    }

    /// A completion reaches only the queues that registered for it.
    ///
    /// A flip knows its port, not which queue a title waits on, so registration decides; a post to
    /// every queue would deliver completions to queues used for something else.
    #[test]
    fn a_posted_event_reaches_only_queues_that_registered_for_it() {
        let listening = super::create_equeue("listening");
        let indifferent = super::create_equeue("indifferent");
        let ident = 0x5150;
        assert!(super::register_event(listening, ident));
        assert_eq!(
            super::post_event(ident, completion(ident, 7, 0)),
            1,
            "one queue wanted it"
        );

        assert_eq!(super::take_events(listening, 4).len(), 1);
        assert!(
            super::take_events(indifferent, 4).is_empty(),
            "a queue that registered nothing was handed an event anyway"
        );
    }

    /// A post nobody registered for reaches no queue, and says so.
    #[test]
    fn a_completion_with_no_reader_posts_to_nothing() {
        let ident = 0x5151;
        assert_eq!(
            super::post_event(ident, completion(ident, 0, 0)),
            0,
            "an unregistered completion must report that it had no reader"
        );
    }

    /// The caller's own word is echoed from its registration, not from the poster.
    ///
    /// The poster does not know `udata`, so taking it from the post would always return zero.
    #[test]
    fn the_registrations_udata_is_what_comes_back() {
        let queue = super::create_equeue("echo");
        let ident = 0x5152;
        assert!(super::register_event_with_udata(queue, ident, 0xabcd_ef01));
        // Deliberately wrong: the poster does not know this and must not decide it.
        super::post_event(ident, completion(ident, 0, 0xdead_dead));
        let delivered = super::take_events(queue, 1);
        assert_eq!(delivered.len(), 1);
        assert_eq!(
            delivered[0].udata, 0xabcd_ef01,
            "the poster's word was handed back instead of the registration's"
        );
    }

    /// Events come back oldest first, and no more than were asked for.
    #[test]
    fn events_are_delivered_oldest_first_and_bounded_by_the_request() {
        let queue = super::create_equeue("ordered");
        let ident = 0x5153;
        assert!(super::register_event(queue, ident));
        for data in 1..=3i64 {
            super::post_event(ident, completion(ident, data, 0));
        }

        let first = super::take_events(queue, 1);
        assert_eq!(first.len(), 1, "one was asked for");
        assert_eq!(first[0].data, 1, "and it must be the oldest");
        let rest = super::take_events(queue, 8);
        assert_eq!(
            rest.iter().map(|e| e.data).collect::<Vec<_>>(),
            vec![2, 3],
            "the remainder, still in order, and asking for eight does not invent five"
        );
        assert!(
            super::take_events(queue, 8).is_empty(),
            "a drained queue keeps delivering events"
        );
    }

    /// A wait on a queue nobody created is not an empty queue.
    ///
    /// Nothing ready is an ordinary poll; an unknown handle is a value orbistoun never issued.
    #[test]
    fn an_unknown_queue_is_distinguishable_from_an_empty_one() {
        let real = super::create_equeue("real");
        assert!(super::equeue_exists(real));
        assert_eq!(
            super::wait_events(real, 1, Blocking::Never),
            Err(Refusal::TimedOut),
            "real but empty"
        );
        assert_eq!(
            super::wait_events(0xdead_beef_0bad_0bad, 1, Blocking::Never),
            Err(Refusal::NoQueue),
            "a handle nothing issued must not look like a queue"
        );
    }

    /// A user event registered with `EV_CLEAR` is delivered once per trigger, with the trigger's
    /// `udata`, and repeated triggers before a wait coalesce into one delivery
    /// (`kern_event.c:1155-1198`, the knote queued once).
    #[test]
    fn a_clearing_user_event_coalesces_its_triggers() {
        let queue = super::create_equeue("edge");
        super::apply(queue, user(0x22, EV_ADD | EV_CLEAR, 0, 0)).expect("added");
        assert!(super::take_events(queue, 4).is_empty(), "not triggered yet");
        for udata in 1..=3 {
            trigger(queue, 0x22, udata).expect("triggered");
        }
        let events = super::take_events(queue, 4);
        assert_eq!(events.len(), 1, "three triggers, one knote: {events:?}");
        assert_eq!(events[0].filter, EVFILT_USER);
        assert_eq!(events[0].flags, EV_CLEAR, "the stored flags, less EV_ADD");
        assert_eq!(events[0].udata, 3, "the last trigger's word");
        assert!(
            super::take_events(queue, 4).is_empty(),
            "cleared once delivered"
        );
        trigger(queue, 0x22, 4).expect("triggered again");
        assert_eq!(idents(&super::take_events(queue, 4)), vec![0x22]);
    }

    /// A user event without `EV_CLEAR` stays fired once triggered, and every wait delivers it again.
    #[test]
    fn a_level_user_event_is_delivered_by_every_wait() {
        let queue = super::create_equeue("level");
        super::apply(queue, user(0x11, EV_ADD, 0, 0)).expect("added");
        trigger(queue, 0x11, 0xaa).expect("triggered");
        for _ in 0..3 {
            let events = super::take_events(queue, 4);
            assert_eq!(idents(&events), vec![0x11], "once per wait, not more");
        }
        super::apply(queue, user(0x11, EV_DELETE, 0, 0)).expect("deleted");
        assert!(
            super::take_events(queue, 4).is_empty(),
            "gone with its knote"
        );
    }

    /// One-shot events are delivered once and dropped; dispatching ones are disabled until enabled.
    #[test]
    fn one_shot_and_dispatch_events_stop_after_one_delivery() {
        let queue = super::create_equeue("once");
        super::apply(queue, user(1, EV_ADD | EV_ONESHOT, NOTE_TRIGGER, 0)).expect("one-shot");
        super::apply(queue, user(2, EV_ADD | EV_DISPATCH, NOTE_TRIGGER, 0)).expect("dispatch");
        assert_eq!(idents(&super::take_events(queue, 4)), vec![1, 2]);
        assert!(super::take_events(queue, 4).is_empty());
        assert_eq!(
            trigger(queue, 1, 0),
            Err(Refusal::NoEvent),
            "the one-shot was dropped"
        );
        trigger(queue, 2, 0).expect("still registered");
        assert!(
            super::take_events(queue, 4).is_empty(),
            "disabled by its dispatch"
        );
        super::apply(queue, user(2, EV_ENABLE, 0, 0)).expect("enabled");
        assert_eq!(idents(&super::take_events(queue, 4)), vec![2]);
    }

    /// A disabled event keeps its trigger and reports it once enabled.
    #[test]
    fn a_disabled_event_reports_once_enabled() {
        let queue = super::create_equeue("disabled");
        super::apply(queue, user(5, EV_ADD | EV_CLEAR | EV_DISABLE, 0, 0)).expect("added");
        trigger(queue, 5, 0).expect("triggered");
        assert!(super::take_events(queue, 4).is_empty());
        super::apply(queue, user(5, EV_ENABLE, 0, 0)).expect("enabled");
        assert_eq!(idents(&super::take_events(queue, 4)), vec![5]);
        assert_eq!(
            super::apply(queue, user(5, EV_ENABLE | EV_DISABLE, 0, 0)),
            Err(Refusal::Invalid)
        );
    }

    /// The `NOTE_FF*` controls apply a change's low twenty-four bits to the stored flags.
    #[test]
    fn user_fflags_follow_the_note_controls() {
        let queue = super::create_equeue("fflags");
        super::apply(queue, user(9, EV_ADD | EV_CLEAR, NOTE_FFCOPY | 0x0f, 0)).expect("added");
        super::apply(queue, user(9, 0, NOTE_FFOR | 0x30, 0)).expect("or");
        super::apply(queue, user(9, 0, NOTE_FFAND | 0x3c, 0)).expect("and");
        trigger(queue, 9, 0).expect("triggered");
        let events = super::take_events(queue, 1);
        // The add stored its fflags raw, control bits and all (`kern_event.c:1837`): 0xc000_000f,
        // then OR 0x30 makes 0xc000_003f, and AND 0x3c leaves 0x3c.
        assert_eq!(events[0].fflags, 0x3c);
    }

    /// Events are delivered in the order they became ready, not the order they were added.
    #[test]
    fn events_come_back_in_the_order_they_fired() {
        let queue = super::create_equeue("order");
        for id in [3, 1, 2] {
            super::apply(queue, user(id, EV_ADD | EV_CLEAR, 0, 0)).expect("added");
        }
        for id in [2, 3, 1] {
            trigger(queue, id, 0).expect("triggered");
        }
        assert_eq!(idents(&super::take_events(queue, 1)), vec![2]);
        assert_eq!(idents(&super::take_events(queue, 8)), vec![3, 1]);
    }

    /// Changes to events nobody added, and to queues nobody created, are refused by name.
    #[test]
    fn changes_to_missing_events_and_queues_are_refused() {
        let queue = super::create_equeue("refusals");
        assert_eq!(trigger(queue, 0x99, 0), Err(Refusal::NoEvent));
        assert_eq!(
            super::apply(queue, user(0x99, EV_DELETE, 0, 0)),
            Err(Refusal::NoEvent)
        );
        assert_eq!(trigger(0xdead_beef, 1, 0), Err(Refusal::NoQueue));
        let other = Change {
            filter: -7,
            ..user(1, EV_ADD, 0, 0)
        };
        assert_eq!(
            super::apply(queue, other),
            Err(Refusal::Invalid),
            "not kept as a knote"
        );
    }

    /// A trigger wakes a thread already waiting.
    #[test]
    fn a_trigger_wakes_a_waiter() {
        let queue = super::create_equeue("wake");
        super::apply(queue, user(7, EV_ADD | EV_CLEAR, 0, 0)).expect("added");
        let waiter = std::thread::spawn(move || super::wait_events(queue, 1, Blocking::Forever));
        std::thread::sleep(std::time::Duration::from_millis(20));
        trigger(queue, 7, 0x77).expect("triggered");
        let events = waiter
            .join()
            .expect("the waiter returns")
            .expect("an event");
        assert_eq!(events[0].udata, 0x77);
    }

    /// A wait with a deadline gives up at it.
    #[test]
    fn a_wait_times_out_at_its_deadline() {
        let queue = super::create_equeue("deadline");
        let start = std::time::Instant::now();
        let until = Blocking::Until(start + std::time::Duration::from_millis(20));
        assert_eq!(super::wait_events(queue, 1, until), Err(Refusal::TimedOut));
        assert!(start.elapsed() >= std::time::Duration::from_millis(20));
    }

    /// Deleting a queue refuses everything after, and wakes a thread waiting on it.
    #[test]
    fn a_deleted_queue_wakes_its_waiter_and_refuses_after() {
        let queue = super::create_equeue("deleted");
        let waiter = std::thread::spawn(move || super::wait_events(queue, 1, Blocking::Forever));
        std::thread::sleep(std::time::Duration::from_millis(20));
        assert!(super::delete_equeue(queue));
        assert_eq!(waiter.join().expect("returns"), Err(Refusal::NoQueue));
        assert!(!super::delete_equeue(queue), "only once");
        assert!(!super::register_event(queue, 1));
        assert_eq!(trigger(queue, 1, 0), Err(Refusal::NoQueue));
    }

    /// Removing a route stops its completions, including one posted and not yet collected.
    #[test]
    fn an_unregistered_route_takes_its_pending_completions_with_it() {
        let queue = super::create_equeue("unroute");
        assert!(super::register_filtered_event(queue, -14, 0, 0x61));
        assert_eq!(
            super::post_filtered_event(-14, |id| completion(id, 1, 0)),
            1
        );
        assert!(super::unregister_filtered_event(queue, -14, 0));
        assert!(
            !super::unregister_filtered_event(queue, -14, 0),
            "only once"
        );
        assert!(super::take_events(queue, 4).is_empty());
        assert_eq!(
            super::post_filtered_event(-14, |id| completion(id, 1, 0)),
            0
        );
    }

    /// Posted completions and user events share one ready list, in the order they arrived.
    #[test]
    fn completions_and_user_events_interleave_by_arrival() {
        let queue = super::create_equeue("mixed");
        let port = 0x5154;
        assert!(super::register_event(queue, port));
        super::apply(queue, user(1, EV_ADD | EV_CLEAR, 0, 0)).expect("added");
        super::post_event(port, completion(port, 0, 0));
        trigger(queue, 1, 0).expect("triggered");
        super::post_event(port, completion(port, 1, 0));
        assert_eq!(idents(&super::take_events(queue, 8)), vec![port, 1, port]);
    }
}
