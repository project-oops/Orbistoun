//! A call spec, run end to end: a [`Script`] driven through a [`Client`] against a [`Responder`],
//! and the transcript it leaves read back as `orbistoun-cli probe` reads one.
//!
//! The two ends talk over an in-memory pipe, so no socket is opened (D016).

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::Duration;

use orbistoun_probe::client::Client;
use orbistoun_probe::respond::{Answers, Responder};
use orbistoun_probe::script::{self, Script};
use orbistoun_probe::{Capability, Outcome, Record, Refusal, Transcript};

/// One end of a duplex pipe: what is written arrives at the other end's reads.
struct End {
    send: Sender<Vec<u8>>,
    receive: Receiver<Vec<u8>>,
    pending: Vec<u8>,
}

fn pipe() -> (End, End) {
    let (to_b, from_a) = channel();
    let (to_a, from_b) = channel();
    (
        End {
            send: to_b,
            receive: from_b,
            pending: Vec::new(),
        },
        End {
            send: to_a,
            receive: from_a,
            pending: Vec::new(),
        },
    )
}

impl Read for End {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        if self.pending.is_empty() {
            // A closed far end reads as the end of the stream, as a closed socket does.
            match self.receive.recv() {
                Ok(bytes) => self.pending = bytes,
                Err(_) => return Ok(0),
            }
        }
        let n = buffer.len().min(self.pending.len());
        buffer[..n].copy_from_slice(&self.pending[..n]);
        self.pending.drain(..n);
        Ok(n)
    }
}

impl Write for End {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        let _ = self.send.send(buffer.to_vec());
        Ok(buffer.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Where the one function this backend serves lives.
const STORE: u64 = 0x1000;

/// A backend with an address space of its own: one function, `store(destination, word)`, which
/// writes a little-endian word into a scratch block, and the blocks it hands out.
#[derive(Default)]
struct Bench {
    blocks: BTreeMap<u64, Vec<u8>>,
}

impl Bench {
    fn block(&mut self, address: u64) -> Option<(&mut Vec<u8>, usize)> {
        let (&base, block) = self.blocks.range_mut(..=address).next_back()?;
        let offset = usize::try_from(address - base).ok()?;
        (offset < block.len()).then_some((block, offset))
    }
}

impl Answers for Bench {
    fn capabilities(&self) -> Vec<Capability> {
        vec![
            Capability::Call,
            Capability::Resolve,
            Capability::Read,
            Capability::Scratch,
        ]
    }

    fn session(&self) -> String {
        "bench".to_owned()
    }

    fn resolve(&mut self, library: &str, symbol: &str) -> Result<Option<u64>, Refusal> {
        Ok((library, symbol)
            .eq(&("libBench", "store"))
            .then_some(STORE))
    }

    fn scratch(&mut self, size: u64, fill: u8) -> Result<u64, Refusal> {
        let address = 0x8000_0000 + 0x1_0000 * self.blocks.len() as u64;
        let size = usize::try_from(size).map_err(|_| Refusal::BadArgument)?;
        self.blocks.insert(address, vec![fill; size]);
        Ok(address)
    }

    fn call(&mut self, address: u64, arguments: &[u64]) -> Result<(Outcome, String), Refusal> {
        if address != STORE {
            return Err(Refusal::Unmapped);
        }
        let [destination, word, ..] = *arguments else {
            return Err(Refusal::BadArgument);
        };
        let (block, offset) = self.block(destination).ok_or(Refusal::Unmapped)?;
        let word = u32::try_from(word).map_err(|_| Refusal::BadArgument)?;
        block[offset..offset + 4].copy_from_slice(&word.to_le_bytes());
        Ok((Outcome::Returned(0), String::new()))
    }

    fn read(&mut self, address: u64, length: u64) -> Result<Vec<u8>, Refusal> {
        let (block, offset) = self.block(address).ok_or(Refusal::Unmapped)?;
        let end = offset + usize::try_from(length).map_err(|_| Refusal::BadArgument)?;
        block
            .get(offset..end)
            .map(<[u8]>::to_vec)
            .ok_or(Refusal::Unmapped)
    }
}

/// A spec resolves the function, takes a block filled 0xcc, calls the function into it, and reads
/// it back; the transcript shows the sentinel where nothing was written and the word where it was.
#[test]
fn a_spec_runs_and_its_transcript_reads_back() {
    let spec = Script::parse(concat!(
        "[[step]]\nresolve = { library = \"libBench\", symbol = \"store\" }\nas = \"store\"\n\n",
        "[[step]]\nscratch = { bytes = 8, fill = 0xcc }\nas = \"out\"\n\n",
        "[[step]]\ncall = \"store\"\nargs = [\"out\", \"0x11223344\"]\n\n",
        "[[step]]\nread = \"out\"\nbytes = 8\n",
    ))
    .expect("the spec parses");

    let (driver, far) = pipe();
    let responder = std::thread::spawn(move || {
        let mut responder = Responder::new(far, Bench::default());
        responder.serve().expect("the responder serves");
    });
    let mut client = Client::new(driver, Duration::from_secs(5));
    client
        .hello(orbistoun_probe::VERSION, None)
        .expect("negotiated");
    let answers = script::run(&mut client, &spec).expect("the spec runs");
    client.bye().expect("closed");
    let transcript = client.transcript().join("\n");
    drop(client);
    responder.join().expect("the responder ends");

    assert_eq!(answers.len(), 4);
    assert_eq!(answers[0].outcome, Outcome::Returned(STORE));
    let read = Transcript::read(&transcript).expect("the transcript parses");
    let dumped = read
        .exchanges
        .iter()
        .filter(|exchange| exchange.verb == "read")
        .flat_map(|exchange| &exchange.records)
        .find_map(|record| match record {
            Record::Bytes { hex, .. } => Some(hex.clone()),
            _ => None,
        })
        .expect("the read's bytes are in the transcript");
    assert_eq!(
        dumped, "44332211cccccccc",
        "the word, then the untouched sentinel"
    );
}

/// A name nothing bound stops the script at that step, with what was sent before it kept.
#[test]
fn a_name_nothing_bound_stops_the_script_there() {
    let spec = Script::parse("[[step]]\nread = \"nowhere\"\nbytes = 4\n").expect("parses");
    let (driver, far) = pipe();
    let responder = std::thread::spawn(move || {
        let _ = Responder::new(far, Bench::default()).serve();
    });
    let mut client = Client::new(driver, Duration::from_secs(5));
    client
        .hello(orbistoun_probe::VERSION, None)
        .expect("negotiated");
    let stopped = script::run(&mut client, &spec);
    assert!(
        matches!(&stopped, Err(script::ScriptError::Unbound { step: 1, value }) if value == "nowhere"),
        "{stopped:?}"
    );
    client.bye().expect("closed");
    drop(client);
    responder.join().expect("the responder ends");
}
