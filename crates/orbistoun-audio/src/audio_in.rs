//! `libSceAudioIn` - audio capture, as a console with no capture device answers it.
//!
//! orbistoun has no microphone. Every answer here is what the console gave with none attached
//! (`port-count` 0, no DualSense microphone; REQ-20261020T1125Z-au01,
//! `reports/hardware/report-20261020-au01-census.log`, `000-boot/au01-audioin`):
//! - an ordinary port opens and reads silence;
//! - a high-quality port is refused;
//! - type 2 is refused.
//!
//! Only the shapes the census called are answered; any other answers the placeholder.

use orbistoun_core::{GUEST_ARG_REGISTERS, GuestError};
use orbistoun_hle::guest_module;

guest_module! {
    "libSceAudioIn" {
        // (handle).
        "sceAudioInClose" => 1,
        // (handle).
        "sceAudioInGetSilentState" => 1,
        // (user, type, index, length, frequency, param), as measured.
        "sceAudioInHqOpen" => 6,
        // (handle, &buffer).
        "sceAudioInInput" => 2,
        // (user, type, index, length, frequency, param), as measured.
        "sceAudioInOpen" => 6,
    }
}

/// What `sceAudioInOpen` answered for type 2, at either rate and either param.
const TYPE_REFUSED: u64 = 0x8026_0104;

/// What `sceAudioInHqOpen` answered at 16 kHz.
const HQ_REFUSED_16K: u64 = 0x8026_0103;

/// What `sceAudioInHqOpen` answered at 48 kHz.
const HQ_REFUSED_48K: u64 = 0x8026_0102;

/// What a second `sceAudioInClose` of one handle answered.
const NO_SUCH_HANDLE: u64 = 0x8026_0109;

/// A handle's high bits: the census's handles were `0x30000007` and `0x30000006`.
const HANDLE_BASE: u32 = 0x3000_0000;

/// The slot the first port gets. Each later port gets the highest slot still free: with
/// `0x30000007` held, every port opened and closed after it was `0x30000006`. How many slots
/// there are, and the order past the second, are assumed.
const FIRST_SLOT: u32 = 7;

/// The only length the census opened with, in samples per read.
const MEASURED_LENGTH: u64 = 256;

/// The rates the census opened at.
const MEASURED_RATES: [u64; 2] = [16_000, 48_000];

/// An open port: the params it was opened with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Port {
    /// Samples per read.
    length: u64,
    /// The sample format param: 0 or 1.
    param: u64,
}

/// The open ports, by slot.
#[derive(Debug, Default)]
struct Ports {
    /// Open ports by slot.
    open: std::collections::BTreeMap<u32, Port>,
}

impl Ports {
    /// `sceAudioInOpen`'s answer: a handle for types 0 and 1, the refusal for type 2, and `None`
    /// for any shape the census did not call - index other than 0, length other than 256, a rate
    /// other than 16 or 48 kHz, a param other than 0 or 1, another type, or every slot held.
    fn open(&mut self, kind: u64, index: u64, length: u64, rate: u64, param: u64) -> Option<u64> {
        if index != 0 || length != MEASURED_LENGTH || !MEASURED_RATES.contains(&rate) || param > 1 {
            return None;
        }
        match kind {
            0 | 1 => {
                let slot = (0..=FIRST_SLOT)
                    .rev()
                    .find(|s| !self.open.contains_key(s))?;
                self.open.insert(slot, Port { length, param });
                Some(u64::from(HANDLE_BASE | slot))
            }
            2 => Some(TYPE_REFUSED),
            _ => None,
        }
    }

    /// `sceAudioInHqOpen`'s answer: refused, by rate, for types 0 to 2; `None` for any shape the
    /// census did not call.
    fn hq_open(kind: u64, index: u64, length: u64, rate: u64, param: u64) -> Option<u64> {
        if kind > 2 || index != 0 || length != MEASURED_LENGTH || param > 1 {
            return None;
        }
        match rate {
            16_000 => Some(HQ_REFUSED_16K),
            48_000 => Some(HQ_REFUSED_48K),
            _ => None,
        }
    }

    /// The port a handle names, if it is open.
    fn get(&self, handle: u64) -> Option<Port> {
        let slot = u32::try_from(handle).ok()?.checked_sub(HANDLE_BASE)?;
        self.open.get(&slot).copied()
    }

    /// `sceAudioInClose`'s answer: 0 for an open port, which it closes; the refusal for a handle
    /// that is not open, such as one already closed.
    fn close(&mut self, handle: u64) -> u64 {
        let slot = u32::try_from(handle)
            .ok()
            .and_then(|h| h.checked_sub(HANDLE_BASE));
        match slot.and_then(|s| self.open.remove(&s)) {
            Some(_) => 0,
            None => NO_SUCH_HANDLE,
        }
    }

    /// What `sceAudioInInput` writes for an open port, and answers: `length` samples of silence
    /// as 16-bit zeros, and the sample count. `None` for a handle not open, and for param 1,
    /// whose read the census did not make.
    fn input(&self, handle: u64) -> Option<(usize, u64)> {
        let port = self.get(handle)?;
        if port.param != 0 {
            return None;
        }
        Some((usize::try_from(port.length).ok()? * 2, port.length))
    }
}

/// The process's capture ports.
static PORTS: std::sync::Mutex<Option<Ports>> = std::sync::Mutex::new(None);

/// Runs `f` against the ports, answering the placeholder where it answers `None`.
fn with_ports(f: impl FnOnce(&mut Ports) -> Option<u64>) -> u64 {
    let mut held = PORTS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    f(held.get_or_insert_with(Ports::default)).unwrap_or_else(placeholder)
}

/// The placeholder an unmeasured shape answers.
fn placeholder() -> u64 {
    u64::from(GuestError::Unimplemented.as_raw())
}

/// `sceAudioInOpen(user, type, index, length, frequency, param)`: a handle, `0x30000007` first,
/// for types 0 and 1; `0x80260104` for type 2 (`open-t*` rows).
pub(crate) fn open(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    with_ports(|ports| ports.open(args[1], args[2], args[3], args[4], args[5]))
}

/// `sceAudioInHqOpen(user, type, index, length, frequency, param)`: refused with no capture
/// device - `0x80260103` at 16 kHz, `0x80260102` at 48 kHz (`hqopen-t*` rows).
pub(crate) fn hq_open(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    Ports::hq_open(args[1], args[2], args[3], args[4], args[5]).unwrap_or_else(placeholder)
}

/// `sceAudioInGetSilentState(handle)`: 1, silent, for an open port, twice over
/// (`audioin-silent-state`). A handle not open is unmeasured.
pub(crate) fn get_silent_state(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    with_ports(|ports| ports.get(args[0]).map(|_| 1))
}

/// `sceAudioInInput(handle, &buffer)`: writes a read of silence, 512 zero bytes for 256 samples,
/// and answers the sample count, without waiting a read's length of time: the three reads
/// measured took 5.4, 3.0 and 1.2 ms against 16 ms of samples (`audioin-input-call1`-`3`).
pub(crate) fn input(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    with_ports(|ports| {
        let (bytes, samples) = ports.input(args[0])?;
        // SAFETY: the guest's capture buffer, a read's worth of bytes by the call's contract;
        // written through the checked accessor.
        unsafe { orbistoun_mem::guest::write_bytes(args[1], &vec![0; bytes]) }.then_some(samples)
    })
}

/// `sceAudioInClose(handle)`: 0, then `0x80260109` for the same handle (`audioin-close`).
pub(crate) fn close(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    with_ports(|ports| Some(ports.close(args[0])))
}

#[cfg(test)]
mod tests {
    use super::{HQ_REFUSED_16K, HQ_REFUSED_48K, NO_SUCH_HANDLE, Ports, TYPE_REFUSED};

    /// The census's open matrix, in its order: the first port is `0x30000007` and stays open;
    /// each later one is `0x30000006` and is closed again; type 2 is refused.
    #[test]
    fn ports_open_as_the_census_measured() {
        let mut ports = Ports::default();
        assert_eq!(ports.open(0, 0, 256, 16_000, 0), Some(0x3000_0007));
        for (kind, rate, param) in [(0, 16_000, 1), (0, 48_000, 0), (1, 48_000, 1)] {
            let handle = ports.open(kind, 0, 256, rate, param).expect("it opens");
            assert_eq!(handle, 0x3000_0006);
            assert_eq!(ports.close(handle), 0);
        }
        assert_eq!(ports.open(2, 0, 256, 48_000, 1), Some(TYPE_REFUSED));
        assert_eq!(
            ports.open(0, 0, 512, 16_000, 0),
            None,
            "an unmeasured length"
        );
        assert_eq!(ports.open(3, 0, 256, 16_000, 0), None, "an unmeasured type");
    }

    /// A high-quality port is refused, by rate.
    #[test]
    fn high_quality_ports_are_refused_by_rate() {
        assert_eq!(Ports::hq_open(0, 0, 256, 16_000, 0), Some(HQ_REFUSED_16K));
        assert_eq!(Ports::hq_open(2, 0, 256, 48_000, 1), Some(HQ_REFUSED_48K));
        assert_eq!(Ports::hq_open(0, 0, 256, 44_100, 0), None);
    }

    /// A read of a param-0 port is 256 samples of silence, 512 bytes; a closed port answers
    /// the refusal when closed again, and reads nothing.
    #[test]
    fn a_port_reads_silence_until_closed() {
        let mut ports = Ports::default();
        let handle = ports.open(0, 0, 256, 16_000, 0).expect("it opens");
        assert_eq!(ports.input(handle), Some((512, 256)));
        assert_eq!(ports.close(handle), 0);
        assert_eq!(ports.close(handle), NO_SUCH_HANDLE);
        assert_eq!(ports.input(handle), None);
        let param_1 = ports.open(0, 0, 256, 16_000, 1).expect("it opens");
        assert_eq!(ports.input(param_1), None, "a param-1 read is unmeasured");
    }
}
