//! Where each command queue stands, for the report a stalled run writes: the graphics queue and
//! each asynchronous compute stream stepped aside, each with the packet it stands at and, for a
//! memory wait, what it waits for, what the word holds now and the last release that wrote it.
//! Every lock is tried rather than taken, so a report made while a thread holds one says so rather
//! than joining the stall.

use std::fmt::Write as _;

use super::{GuestMemory, MappedRegions, aside, command_processor, held_streams, queue_stall};
use crate::cp;

/// A memory wait a stream stands at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MemoryWait {
    /// The address it polls.
    pub address: u64,
    /// The value it compares with.
    pub reference: u64,
    /// The mask the word is taken under.
    pub mask: u64,
    /// The packet's function word: memory space in bit 4, the comparison in bits 2:0.
    pub function: u32,
    /// What the word holds now, unmasked, where it is readable.
    pub now: Option<u64>,
    /// The last value a `RELEASE_MEM` wrote there this run, and how many did.
    pub released: Option<(u64, u64)>,
    /// Whether the command processor polls the wait while the title's threads run (D932): only on
    /// its own thread, so a wait a guest thread's store meets is never seen to hold where
    /// submissions run synchronously.
    pub polled: bool,
}

/// Where a queue stands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stands {
    /// Nothing stopped it.
    Running,
    /// Stopped at a packet: its byte offset, header, name, the memory wait it is where it is one,
    /// and why it stopped.
    At {
        /// Byte offset in its stream.
        offset: u32,
        /// The packet's header word.
        header: u32,
        /// The packet's name, or its opcode where the vocabulary has none.
        packet: String,
        /// The memory wait, where it is one.
        wait: Option<MemoryWait>,
        /// Why the command processor stopped there.
        stopped: String,
    },
    /// Stopped, with its stream not kept: why.
    Stopped(String),
    /// Behind an earlier stream of its queue, not yet carried out.
    Behind,
    /// Not known: the lock over it is held now.
    Held,
}

/// One queue, or one stream of a compute queue, as a stall's report names it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueueState {
    /// `graphics`, or `compute queue <handle>`.
    pub queue: String,
    /// Where it stands.
    pub stands: Stands,
    /// Streams handed to it since it stopped, waiting behind it.
    pub behind: usize,
}

/// The header, opcode and body of the packet at byte `offset` of `stream`.
fn packet_at(stream: &[u8], offset: u32) -> Option<(u32, u8, Vec<u32>)> {
    let packet = crate::packet::walk(stream)
        .packets
        .into_iter()
        .find(|packet| packet.offset == offset)?;
    let crate::packet::PacketKind::Command { opcode } = packet.kind else {
        return None;
    };
    let at = offset as usize;
    let header = u32::from_le_bytes(stream.get(at..at + 4)?.try_into().ok()?);
    let start = packet.body_offset() as usize;
    let body = stream
        .get(start..start + packet.body_length() as usize)?
        .chunks_exact(4)
        .map(|w| u32::from_le_bytes([w[0], w[1], w[2], w[3]]))
        .collect();
    Some((header, opcode, body))
}

/// The address, reference, mask and function of a `WAIT_REG_MEM` or `WAIT_REG_MEM64` body
/// (`amd_cp_packets_gfx11.h:1765-1826`) that polls memory.
fn memory_wait(opcode: u8, body: &[u32]) -> Option<(u64, u64, u64, u32)> {
    let wide = |low: u32, high: u32| u64::from(low) | (u64::from(high) << 32);
    let (function, address, reference, mask) = match (opcode, body) {
        (cp::WAIT_REG_MEM, &[function, low, high, reference, mask, ..]) => (
            function,
            wide(low & !3, high),
            u64::from(reference),
            u64::from(mask),
        ),
        (
            cp::WAIT_REG_MEM64,
            &[
                function,
                low,
                high,
                ref_low,
                ref_high,
                mask_low,
                mask_high,
                ..,
            ],
        ) => (
            function,
            wide(low & !7, high),
            wide(ref_low, ref_high),
            wide(mask_low, mask_high),
        ),
        _ => return None,
    };
    // Bit 4 set: memory, not a register.
    (function & 0x10 != 0).then_some((address, reference, mask, function))
}

/// The comparison a wait's function makes, in bits 2:0 (Mesa `sid.h` `WAIT_REG_MEM_*`).
fn comparison(function: u32) -> &'static str {
    ["always", "<", "<=", "==", "!=", ">=", ">", "reserved"][(function & 7) as usize]
}

/// A queue as one line of the report.
#[must_use]
pub fn describe(state: &QueueState) -> String {
    let behind = match state.behind {
        0 => String::new(),
        n => format!("; {n} streams behind it"),
    };
    let stands = match &state.stands {
        Stands::Running => "not stopped".to_owned(),
        Stands::Behind => "behind an earlier stream of its queue, not yet carried out".to_owned(),
        Stands::Held => "not known: its lock is held".to_owned(),
        Stands::Stopped(why) => format!("stopped: {why}"),
        Stands::At {
            offset,
            header,
            packet,
            wait,
            stopped,
        } => {
            let mut line = format!("at byte {offset:#x}, {packet} (header {header:#010x})");
            match wait {
                Some(wait) => {
                    let now = wait.now.map_or_else(
                        || "unreadable".to_owned(),
                        |now| format!("{now:#x} (masked {:#x})", now & wait.mask),
                    );
                    let released = wait.released.map_or_else(
                        || "no RELEASE_MEM has written it".to_owned(),
                        |(value, count)| {
                            format!("the last of {count} RELEASE_MEM wrote {value:#x}")
                        },
                    );
                    let _ = write!(
                        line,
                        ", waiting until [{:#x}] & {:#x} {} {:#x} (function {:#x}); it holds {now}; {released}",
                        wait.address,
                        wait.mask,
                        comparison(wait.function),
                        wait.reference,
                        wait.function
                    );
                    if !wait.polled {
                        line.push_str(concat!(
                            "; nothing polls it, because submissions run synchronously: ",
                            "ORBISTOUN_SUBMIT=async polls it as a title is played (D932)",
                        ));
                    }
                }
                None => {
                    let _ = write!(line, "; {stopped}");
                }
            }
            line
        }
    };
    format!("{}: {stands}{behind}", state.queue)
}

/// The packet the stream `bytes` stands at, at `offset`, with its wait where it is one.
fn standing(bytes: &[u8], offset: u32, stopped: &cp::Stopped) -> Stands {
    let Some((header, opcode, body)) = packet_at(bytes, offset) else {
        return Stands::Stopped(format!("{stopped:?}"));
    };
    let packet = crate::registers::Vocabulary::builtin()
        .ok()
        .and_then(|vocabulary| vocabulary.opcode_name(opcode).map(str::to_owned))
        .unwrap_or_else(|| format!("opcode {opcode:#x}"));
    let wait = memory_wait(opcode, &body).map(|(address, reference, mask, function)| {
        let wide = opcode == cp::WAIT_REG_MEM64;
        let now = MappedRegions::current()
            .read(address, if wide { 8 } else { 4 })
            .map(|bytes| {
                bytes
                    .iter()
                    .rev()
                    .fold(0u64, |word, &byte| (word << 8) | u64::from(byte))
            });
        let released = cp::released_labels()
            .into_iter()
            .find(|&(at, ..)| at == address)
            .map(|(_, value, count)| (value, count));
        MemoryWait {
            address,
            reference,
            mask,
            function,
            now,
            released,
            polled: super::asynchronous(),
        }
    });
    Stands::At {
        offset,
        header,
        packet,
        wait,
        stopped: format!("{stopped:?}"),
    }
}

/// Where a stop names its packet.
const fn stopped_offset(stopped: &cp::Stopped) -> Option<u32> {
    match *stopped {
        cp::Stopped::Completed => None,
        cp::Stopped::NeedsGpu { offset, .. }
        | cp::Stopped::OutOfBounds { offset, .. }
        | cp::Stopped::WaitNeverSatisfied { offset, .. }
        | cp::Stopped::Malformed { offset } => Some(offset),
    }
}

/// The graphics queue's state.
fn graphics() -> QueueState {
    let behind = held_streams().try_lock().map_or(0, |held| held.len());
    let stands = match queue_stall().try_lock() {
        Err(_) => Stands::Held,
        Ok(stall) => match *stall {
            None => Stands::Running,
            Some(execution) => match super::stalled_stream().try_lock() {
                Err(_) => Stands::Held,
                Ok(kept) => match &*kept {
                    Some(stream) => standing(&stream.bytes, stream.from, &execution.stopped),
                    None => Stands::Stopped(format!("{:?}", execution.stopped)),
                },
            },
        },
    };
    QueueState {
        queue: "graphics".to_owned(),
        stands,
        behind,
    }
}

/// Every queue and compute stream as the run stands: the graphics queue first, then each compute
/// stream stepped aside, oldest first, and the command processor's own queue of submissions handed
/// to it and not yet carried out, where it runs apart (D821).
#[must_use]
pub fn queue_states() -> Vec<QueueState> {
    let mut states = vec![graphics()];
    match aside().try_lock() {
        Err(_) => states.push(QueueState {
            queue: "compute".to_owned(),
            stands: Stands::Held,
            behind: 0,
        }),
        Ok(streams) => {
            for stream in streams.iter() {
                let stands = match &stream.stopped {
                    None => Stands::Behind,
                    Some(stopped) => standing(
                        &stream.bytes,
                        stopped_offset(stopped).unwrap_or(stream.from),
                        stopped,
                    ),
                };
                states.push(QueueState {
                    queue: format!("compute queue {:#x}", stream.submission.compute_queue),
                    stands,
                    behind: 0,
                });
            }
        }
    }
    if super::asynchronous() {
        states.push(QueueState {
            queue: "command processor".to_owned(),
            stands: match command_processor::queued() {
                Some(0) => Stands::Running,
                Some(_) => Stands::Stopped("submissions queued, not yet carried out".to_owned()),
                None => Stands::Held,
            },
            behind: command_processor::queued().unwrap_or(0),
        });
    }
    states
}

#[cfg(test)]
mod tests {
    use super::{MemoryWait, QueueState, Stands, comparison, describe, memory_wait, packet_at};
    use crate::cp;

    /// A `WAIT_REG_MEM` stream, as a title's compute queue waits on a label: function 0x15, memory
    /// and greater-or-equal.
    fn waiting_stream() -> Vec<u8> {
        let header = (3u32 << 30) | (5 << 16) | (u32::from(cp::WAIT_REG_MEM) << 8);
        [header, 0x15, 0x0616_dce8, 0x7400, 0x15ff, u32::MAX, 0x10]
            .iter()
            .flat_map(|word| word.to_le_bytes())
            .collect()
    }

    /// The packet a stream stands at is found by its offset, with its header and body.
    #[test]
    fn the_packet_at_an_offset_is_read_whole() {
        let (header, opcode, body) = packet_at(&waiting_stream(), 0).expect("a packet at 0");
        assert_eq!(header, 0xc005_3c00);
        assert_eq!(opcode, cp::WAIT_REG_MEM);
        assert_eq!(body, [0x15, 0x0616_dce8, 0x7400, 0x15ff, u32::MAX, 0x10]);
        assert_eq!(
            packet_at(&waiting_stream(), 4),
            None,
            "no packet starts there"
        );
    }

    /// A memory wait reads its address, reference, mask and function; a register wait is not one.
    #[test]
    fn a_memory_wait_is_read_from_its_body() {
        let (_, opcode, body) = packet_at(&waiting_stream(), 0).expect("a packet");
        assert_eq!(
            memory_wait(opcode, &body),
            Some((0x7400_0616_dce8, 0x15ff, 0xffff_ffff, 0x15))
        );
        let mut register = body;
        register[0] = 0x05;
        assert_eq!(memory_wait(opcode, &register), None);
        assert_eq!(comparison(0x15), ">=");
        assert_eq!(comparison(0x13), "==");
    }

    /// A queue at a wait says what it waits for, what the word holds and what last wrote it.
    #[test]
    fn a_queue_at_a_wait_says_what_it_waits_for() {
        let state = QueueState {
            queue: "compute queue 0x30".to_owned(),
            stands: Stands::At {
                offset: 0x1c0,
                header: 0xc005_3c00,
                packet: "WAIT_REG_MEM".to_owned(),
                wait: Some(MemoryWait {
                    address: 0x7400_0616_dce8,
                    reference: 0x15ff,
                    mask: 0xffff_ffff,
                    function: 0x15,
                    now: Some(0x15be),
                    released: Some((0x15be, 12)),
                    polled: true,
                }),
                stopped: String::new(),
            },
            behind: 2,
        };
        assert_eq!(
            describe(&state),
            "compute queue 0x30: at byte 0x1c0, WAIT_REG_MEM (header 0xc0053c00), waiting until [0x74000616dce8] & 0xffffffff >= 0x15ff (function 0x15); it holds 0x15be (masked 0x15be); the last of 12 RELEASE_MEM wrote 0x15be; 2 streams behind it"
        );
        let running = QueueState {
            queue: "graphics".to_owned(),
            stands: Stands::Running,
            behind: 0,
        };
        assert_eq!(describe(&running), "graphics: not stopped");
    }

    /// A wait nothing polls says so, and what would poll it: run 334 of Grand Theft Auto V stood
    /// 300 s at a wait its word already met, with submissions run synchronously.
    #[test]
    fn a_wait_nothing_polls_says_so() {
        let state = QueueState {
            queue: "graphics".to_owned(),
            stands: Stands::At {
                offset: 0xf0,
                header: 0xc007_9300,
                packet: "WAIT_REG_MEM64".to_owned(),
                wait: Some(MemoryWait {
                    address: 0x7400_4314_5900,
                    reference: 0x20e,
                    mask: u64::MAX,
                    function: 0x400_0015,
                    now: Some(0x20e),
                    released: None,
                    polled: false,
                }),
                stopped: String::new(),
            },
            behind: 1,
        };
        assert!(
            describe(&state).ends_with(
                "no RELEASE_MEM has written it; nothing polls it, because submissions run synchronously: ORBISTOUN_SUBMIT=async polls it as a title is played (D932); 1 streams behind it"
            ),
            "{}",
            describe(&state)
        );
    }
}
