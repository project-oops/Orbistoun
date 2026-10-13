//! `libSceImeDialog` - the system keyboard a title opens for text.
//!
//! With no dialog open, answered as obSCEne measured it on FW 12.40 (`REQ-20261012T1816Z-b874`,
//! `000-boot/system-dialogs`, rows in `reports/hardware/report-20261013-150600.log`): status 0
//! (none); `Init` refused `0x80bc0015` where the common dialog is not initialised; result, abort
//! and terminate refused `0x80bc0107` (not in use) with the result block untouched.
//!
//! Once the common dialog is initialised, `Init` opens the system keyboard (D826): an
//! [`orbistoun_shell::Keyboard`] over the title's own field, driven by the controller - the same
//! pad, script or window, that the title would otherwise read, and that a capture records. While
//! it is open it has the controller and the title reads a neutral pad. Its view goes to whatever
//! observes it (the window draws it over the title, as the console's system software does). When
//! it ends, the text goes into the title's buffer and the status reads finished. The parameter
//! block's layout and the status and end-status codes are oops-sdk's, which opens this dialog on
//! the console (D826, assumed).
//!
//! Each arity is `6`, the trampoline's full capture (D504).

use std::sync::{Mutex, OnceLock};

use orbistoun_core::{GUEST_ARG_REGISTERS, GuestError};
use orbistoun_hle::guest_module;
use orbistoun_shell::keyboard::View;
use orbistoun_shell::{Keyboard, Move, Outcome, Press};

use crate::pad::{Button, PadState};

guest_module! {
    "libSceImeDialog" {
        "sceImeDialogAbort" => 6,
        "sceImeDialogGetResult" => 6,
        "sceImeDialogGetStatus" => 6,
        "sceImeDialogInit" => 6,
        "sceImeDialogTerm" => 6,
    }
}

/// The dialog is not in use: nothing to abort, read or terminate (measured).
const NOT_IN_USE: u64 = 0x80bc_0107;
/// What `Init` answered in the measured process, whose common dialog was not initialised
/// (measured).
const INIT_REFUSED: u64 = 0x80bc_0015;
/// The dialog's status with nothing open (measured).
const STATUS_NONE: u64 = 0;
/// The dialog's status while it is being typed into (D826, assumed).
const STATUS_RUNNING: u64 = 1;
/// The dialog's status once it has ended (D826, assumed).
const STATUS_FINISHED: u64 = 2;
/// The end status of an entry made (D826, assumed).
const END_OK: u32 = 0;
/// The end status of an entry cancelled (D826, assumed).
const END_CANCELLED: u32 = 1;
/// The end status of a dialog the title aborted (D826, assumed).
const END_ABORTED: u32 = 2;

/// Where in the parameter block the most characters the buffer takes lies (D826, assumed).
const MAX_TEXT_LENGTH_AT: u64 = 0x24;
/// Where in the parameter block the title's UTF-16 buffer's address lies (D826, assumed).
const INPUT_TEXT_BUFFER_AT: u64 = 0x28;

/// The controller's buttons and what each asks of the keyboard (D826): the directions move,
/// the lower face button presses the highlighted key, the left deletes, the upper types a space,
/// the right cancels and the right centre button ends the entry.
const PRESSES: [(Button, Press); 9] = [
    (Button::Up, Press::Move(Move::Up)),
    (Button::Down, Press::Move(Move::Down)),
    (Button::Left, Press::Move(Move::Left)),
    (Button::Right, Press::Move(Move::Right)),
    (Button::South, Press::Select),
    (Button::West, Press::Delete),
    (Button::North, Press::Space),
    (Button::East, Press::Cancel),
    (Button::Start, Press::Done),
];

/// The dialog open now: the keyboard, the title's buffer, the pad as last read, and how the
/// dialog ended once it has.
#[derive(Debug)]
struct Open {
    keyboard: Keyboard,
    buffer: u64,
    most: u32,
    last: PadState,
    ended: Option<u32>,
}

/// The one dialog open, if any. A static, like the pad's script: the guest calls from its own
/// threads with no context to carry.
static OPEN: Mutex<Option<Open>> = Mutex::new(None);

/// Where the keyboard's view goes as it changes - the window, through the worker - and `None` once
/// it is gone. First install wins.
static OBSERVER: OnceLock<fn(Option<&View>)> = OnceLock::new();

/// Sends the keyboard's view to `observer` whenever it opens, changes or closes.
pub fn observe(observer: fn(Option<&View>)) {
    let _ = OBSERVER.set(observer);
}

fn tell(view: Option<&View>) {
    if let Some(observer) = OBSERVER.get() {
        observer(view);
    }
}

fn open() -> std::sync::MutexGuard<'static, Option<Open>> {
    OPEN.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Whether the system keyboard has the controller: open and not yet ended. The title is handed a
/// neutral pad meanwhile (D826).
#[must_use]
pub fn has_focus() -> bool {
    open().as_ref().is_some_and(|dialog| dialog.ended.is_none())
}

/// The text in the title's UTF-16 buffer at `buffer`, up to `most` characters or a terminator.
fn read_field(buffer: u64, most: u32) -> String {
    let mut units = Vec::new();
    for index in 0..u64::from(most) {
        let mut unit = [0_u8; 2];
        // SAFETY: the buffer the title named for the dialog, holding its most characters by the
        // call's contract.
        if !unsafe { orbistoun_mem::guest::read_into(buffer + index * 2, &mut unit) } {
            break;
        }
        let unit = u16::from_le_bytes(unit);
        if unit == 0 {
            break;
        }
        units.push(unit);
    }
    String::from_utf16_lossy(&units)
}

/// Writes `text` into the guest's UTF-16 buffer at `buffer`: at most `most` characters, then a
/// terminating zero. `false` where the buffer cannot be written.
fn write_field(buffer: u64, most: u32, text: &str) -> bool {
    let most = usize::try_from(most).unwrap_or(usize::MAX);
    let mut units: Vec<u16> = text.encode_utf16().take(most).collect();
    units.push(0);
    let bytes: Vec<u8> = units.iter().flat_map(|unit| unit.to_le_bytes()).collect();
    // SAFETY: the buffer the title named in its parameter block for exactly this, holding its
    // most characters and a terminator by the call's contract.
    unsafe { orbistoun_mem::guest::write_bytes(buffer, &bytes) }
}

/// Reads the controller into the open keyboard: each button that went down since it was last read
/// is a press. When the entry ends, the text goes into the title's buffer (made) or not
/// (cancelled), and the keyboard closes on the screen.
fn drive(dialog: &mut Open) {
    if dialog.ended.is_some() {
        return;
    }
    let Some(now) = crate::real_pad() else {
        return;
    };
    let before = std::mem::replace(&mut dialog.last, now);
    let mut changed = false;
    for (button, press) in PRESSES {
        if now.is_down(button) && !before.is_down(button) {
            dialog.keyboard.press(press);
            changed = true;
        }
    }
    match dialog.keyboard.outcome() {
        Outcome::Typing => {
            if changed {
                tell(Some(&dialog.keyboard.view()));
            }
        }
        Outcome::Entered => {
            let made = write_field(dialog.buffer, dialog.most, &dialog.keyboard.text());
            dialog.ended = Some(if made { END_OK } else { END_ABORTED });
            tell(None);
        }
        Outcome::Cancelled => {
            dialog.ended = Some(END_CANCELLED);
            tell(None);
        }
    }
}

/// `sceImeDialogGetStatus()`: none with no dialog open (measured); running while the keyboard is
/// being typed into, reading the controller each time, and finished once its entry has ended
/// (D826).
pub(crate) fn get_status(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let mut held = open();
    let Some(dialog) = held.as_mut() else {
        return STATUS_NONE;
    };
    drive(dialog);
    if dialog.ended.is_some() {
        STATUS_FINISHED
    } else {
        STATUS_RUNNING
    }
}

/// `sceImeDialogGetResult(result)`: with no dialog open `0x80bc0107`, writing nothing (measured);
/// once the dialog has ended, its end status in the result's first word and 0 (D826). Before it
/// has ended, unmeasured: the placeholder.
pub(crate) fn get_result(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let held = open();
    let Some(dialog) = held.as_ref() else {
        return NOT_IN_USE;
    };
    let (Some(end), result) = (dialog.ended, args[0]) else {
        return u64::from(GuestError::Unimplemented.as_raw());
    };
    // SAFETY: the guest's result block, by the call's contract.
    if result != 0 && unsafe { orbistoun_mem::guest::write_bytes(result, &end.to_le_bytes()) } {
        0
    } else {
        u64::from(GuestError::Unimplemented.as_raw())
    }
}

/// `sceImeDialogInit(param, extended)`: `0x80bc0015` where the common dialog is not initialised,
/// as measured; once it is, opens the keyboard over the field `param` names - the text already in
/// it kept - and answers 0 (D826). A null block, an unreadable one, or a dialog already open is
/// unmeasured: the placeholder.
pub(crate) fn init(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    if !orbistoun_systemservice::common_dialog::initialized() {
        return INIT_REFUSED;
    }
    let param = args[0];
    let mut held = open();
    if param == 0 || held.is_some() {
        return u64::from(GuestError::Unimplemented.as_raw());
    }
    // SAFETY: the guest's parameter block, by the call's contract.
    let buffer = unsafe { orbistoun_mem::guest::read_u64(param + INPUT_TEXT_BUFFER_AT) };
    // SAFETY: as above.
    let most = unsafe { orbistoun_mem::guest::read_u32(param + MAX_TEXT_LENGTH_AT) };
    let (Some(buffer), Some(most)) = (buffer, most) else {
        return u64::from(GuestError::Unimplemented.as_raw());
    };
    if buffer == 0 {
        return u64::from(GuestError::Unimplemented.as_raw());
    }
    let keyboard = Keyboard::new(
        &read_field(buffer, most),
        usize::try_from(most).unwrap_or(usize::MAX),
    );
    tell(Some(&keyboard.view()));
    // What is held as it opens is not a press: a button still down from the press that opened it
    // must be released first.
    let last = crate::real_pad().unwrap_or_default();
    *held = Some(Open {
        keyboard,
        buffer,
        most,
        last,
        ended: None,
    });
    0
}

/// `sceImeDialogAbort()`: with no dialog open `0x80bc0107` (measured); an open one ends aborted,
/// answering 0 (D826).
pub(crate) fn abort(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    match open().as_mut() {
        Some(dialog) => {
            if dialog.ended.is_none() {
                dialog.ended = Some(END_ABORTED);
                tell(None);
            }
            0
        }
        None => NOT_IN_USE,
    }
}

/// `sceImeDialogTerm()`: with no dialog open `0x80bc0107`, every time (measured); an open one
/// closes, answering 0 (D826).
pub(crate) fn term(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    match open().take() {
        Some(dialog) => {
            if dialog.ended.is_none() {
                tell(None);
            }
            0
        }
        None => NOT_IN_USE,
    }
}

#[cfg(test)]
mod tests {
    use orbistoun_core::GUEST_ARG_REGISTERS;

    use crate::pad::{Button, PadState};

    /// The tests share the one dialog and the one pad, so they run one at a time.
    static ALONE: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// The measured sequence: status none, result refused with the block untouched, abort and
    /// terminate refused, terminate again refused.
    #[test]
    fn a_dialog_that_never_opens_answers_as_measured() {
        let _alone = ALONE
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let none = [0_u64; GUEST_ARG_REGISTERS];
        let block = [0xcc_u8; 0x40];
        let mut with = none;
        with[0] = block.as_ptr() as u64;
        assert_eq!(super::get_status(&none), 0);
        assert_eq!(super::get_result(&with), 0x80bc_0107);
        assert_eq!(block, [0xcc; 0x40], "untouched");
        assert_eq!(super::abort(&none), 0x80bc_0107);
        assert_eq!(super::term(&none), 0x80bc_0107);
        assert_eq!(super::term(&none), 0x80bc_0107);
    }

    /// The keyboard is typed into with the controller the title would read, a press at a time:
    /// while it is open the title reads a neutral pad, and when Start ends the entry the text is in
    /// the title's field, after what the field already held, cut to the most it takes and
    /// terminated, and the dialog has finished OK (D826). SuperTuxKart's "Create User" name field
    /// opens it.
    #[test]
    fn the_keyboard_is_typed_into_with_the_controller() {
        let _alone = ALONE
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        // The common dialog initialised, as every title does before opening the keyboard.
        let (_, initialize) = orbistoun_systemservice::common_dialog::implementations()[0];
        let _ = initialize(&[0; GUEST_ARG_REGISTERS]);
        let pad = |buttons: &[Button]| {
            let mut state = PadState::neutral();
            for &button in buttons {
                state.set(button, true);
            }
            crate::latest::arrived(&[state]);
        };
        pad(&[]);
        let mut buffer = [0xffff_u16; 8];
        buffer[0] = u16::from(b'a');
        buffer[1] = 0;
        let mut param = [0_u8; 96];
        param[0x24..0x28].copy_from_slice(&4_u32.to_le_bytes());
        param[0x28..0x30].copy_from_slice(&(buffer.as_mut_ptr() as u64).to_le_bytes());
        let mut args = [0_u64; GUEST_ARG_REGISTERS];
        args[0] = param.as_ptr() as u64;
        assert_eq!(super::init(&args), 0, "opened");
        assert!(super::has_focus(), "the keyboard has the controller");
        // Right four keys along the top row, a press on "5", a space, then "5" twice more.
        let tap = |button: Button| {
            pad(&[button]);
            assert_eq!(super::get_status(&args), 1, "running");
            pad(&[]);
            assert_eq!(super::get_status(&args), 1, "running");
        };
        for _ in 0..4 {
            tap(Button::Right);
        }
        tap(Button::South);
        tap(Button::North);
        tap(Button::South);
        tap(Button::South);
        pad(&[Button::Start]);
        assert_eq!(super::get_status(&args), 2, "finished");
        assert!(!super::has_focus(), "the title has the controller again");
        assert_eq!(
            String::from_utf16_lossy(&buffer[..5]),
            "a5 5\u{0}",
            "the field's own text, then three characters typed to its four, and a terminator"
        );
        assert_eq!(buffer[5], 0xffff, "nothing past the terminator");
        let mut result = [0xcc_u8; 16];
        args[0] = result.as_mut_ptr() as u64;
        assert_eq!(super::get_result(&args), 0);
        assert_eq!(result[..4], [0, 0, 0, 0], "ended OK");
        assert_eq!(super::term(&args), 0, "closed");
        assert_eq!(super::term(&args), 0x80bc_0107, "and only once");
        pad(&[]);
    }
}
