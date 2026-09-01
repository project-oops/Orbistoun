//! One transport: newline-delimited JSON.
//!
//! Deliberately in its own module, and deliberately simple. The protocol in the
//! parent module is the contract; this is one way of moving it, chosen because a
//! human can read a captured stream with no tooling and because a stalled worker's
//! last message is visible in a pipe dump.
//!
//! Newline framing is safe here because JSON escapes literal newlines inside strings,
//! so no message body can contain the delimiter. That is asserted below rather than
//! assumed, since it is the one property the whole framing rests on.

use std::io::{self, BufRead, Write};

use serde::Serialize;
use serde::de::DeserializeOwned;

/// Writes one message followed by a newline, and flushes.
///
/// Flushing per message is deliberate: an unflushed final message before a crash is
/// exactly the one worth having.
pub fn write_message<W: Write, T: Serialize>(writer: &mut W, message: &T) -> io::Result<()> {
    let line = serde_json::to_string(message).map_err(io::Error::other)?;
    debug_assert!(
        !line.contains('\n'),
        "serialised message contained a raw newline, which would corrupt framing"
    );
    writer.write_all(line.as_bytes())?;
    writer.write_all(b"\n")?;
    writer.flush()
}

/// Reads one message, or `None` at end of stream.
///
/// A malformed line is an error rather than a skip: silently dropping a message the
/// peer believes it sent produces a desynchronised stream and an unfalsifiable bug.
pub fn read_message<R: BufRead, T: DeserializeOwned>(reader: &mut R) -> io::Result<Option<T>> {
    let mut line = String::new();
    if reader.read_line(&mut line)? == 0 {
        return Ok(None);
    }
    let trimmed = line.trim_end_matches(['\r', '\n']);
    if trimmed.is_empty() {
        return Ok(None);
    }
    serde_json::from_str(trimmed).map(Some).map_err(|e| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("malformed message: {e}: {trimmed}"),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::{read_message, write_message};
    use crate::{Event, Outcome, Phase, Request};
    use std::io::{BufReader, Cursor};
    use std::path::PathBuf;

    #[test]
    fn a_message_round_trips_through_the_framing() {
        let mut buf = Vec::new();
        let sent = Request::Survey {
            path: PathBuf::from("/titles/x/eboot.bin"),
        };
        write_message(&mut buf, &sent).expect("write");

        let mut reader = BufReader::new(Cursor::new(buf));
        let got: Option<Request> = read_message(&mut reader).expect("read");
        assert_eq!(got, Some(sent));
    }

    #[test]
    fn several_messages_stream_in_order() {
        let mut buf = Vec::new();
        for phase in [
            Phase::ContainerParsed,
            Phase::ImportsResolved,
            Phase::Mapped,
        ] {
            write_message(&mut buf, &Event::Reached { phase }).expect("write");
        }

        let mut reader = BufReader::new(Cursor::new(buf));
        let mut seen = Vec::new();
        while let Some(Event::Reached { phase }) =
            read_message::<_, Event>(&mut reader).expect("read")
        {
            seen.push(phase);
        }
        assert_eq!(
            seen,
            [
                Phase::ContainerParsed,
                Phase::ImportsResolved,
                Phase::Mapped
            ]
        );
    }

    /// The property the entire framing rests on.
    #[test]
    fn embedded_newlines_cannot_corrupt_the_frame() {
        let mut buf = Vec::new();
        let sent = Event::Failed {
            error: "line one\nline two\r\nline three".to_owned(),
        };
        write_message(&mut buf, &sent).expect("write");

        // Exactly one frame on the wire, despite three newlines in the payload.
        // Expressed as a split rather than a byte count: it states the property
        // directly ("one frame plus an empty tail") instead of inferring it.
        let frames: Vec<_> = buf.split(|b| *b == b'\n').collect();
        assert_eq!(
            frames.len(),
            2,
            "JSON must have escaped the payload newlines, leaving one frame"
        );
        assert!(frames[1].is_empty(), "nothing follows the delimiter");

        let mut reader = BufReader::new(Cursor::new(buf));
        let got: Option<Event> = read_message(&mut reader).expect("read");
        assert_eq!(got, Some(sent), "and the payload survived intact");
    }

    #[test]
    fn end_of_stream_reads_as_none_not_an_error() {
        let mut reader = BufReader::new(Cursor::new(Vec::new()));
        let got: Option<Request> = read_message(&mut reader).expect("clean EOF");
        assert!(got.is_none());
    }

    #[test]
    fn a_malformed_line_is_an_error_not_a_silent_skip() {
        // Dropping a message the peer believes it sent desynchronises the stream and
        // produces a bug nobody can reproduce.
        let mut reader = BufReader::new(Cursor::new(b"{not json}\n".to_vec()));
        let err = read_message::<_, Request>(&mut reader).expect_err("must not skip");
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
    }

    #[test]
    fn terminal_events_survive_the_wire() {
        let mut buf = Vec::new();
        let sent = Event::Terminated {
            outcome: Outcome::Crashed {
                signal: "SIGSEGV".to_owned(),
            },
            reached: Phase::Entered,
        };
        write_message(&mut buf, &sent).expect("write");
        let mut reader = BufReader::new(Cursor::new(buf));
        assert_eq!(
            read_message::<_, Event>(&mut reader).expect("read"),
            Some(sent)
        );
    }
}
