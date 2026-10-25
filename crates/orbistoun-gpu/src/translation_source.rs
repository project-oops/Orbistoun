//! What a dispatch's module was translated from: the guest program's bytes, up to its end as the
//! pipeline decoded it, and the translation context the dispatch path built for it. The pipeline
//! notes it for each dispatch it prepares, by the module it gave, so a capture of the dispatch can
//! keep it beside the module ([`crate::translations::Context`] is the kept translations' own,
//! serialised the same way), and a test or a command can translate the program again under the
//! same context - whole, or cut after its first instructions - and compare.

use std::sync::Mutex;

use orbistoun_shader::{EncodingTable, OperandTable, decode_program};
use orbistoun_translate::translate_with_user_data;

use crate::translations::Context;

/// A guest program and the context it was translated under.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Source {
    /// Where the program lies in guest memory.
    pub address: u64,
    /// Its bytes, end-of-program included.
    pub program: Vec<u8>,
    /// What it was translated against.
    pub context: Context,
}

impl Source {
    /// Its context as the text a capture keeps.
    ///
    /// # Errors
    ///
    /// The context could not be written, which a context of plain values never is.
    pub fn context_text(&self) -> Result<String, String> {
        serde_json::to_string_pretty(&self.context).map_err(|e| e.to_string())
    }

    /// A source from the program's address and bytes and its context's text.
    ///
    /// # Errors
    ///
    /// The text is not a context.
    pub fn from_parts(address: u64, program: Vec<u8>, context: &str) -> Result<Self, String> {
        Ok(Self {
            address,
            program,
            context: serde_json::from_str(context).map_err(|e| e.to_string())?,
        })
    }

    /// The program translated again under its context, as SPIR-V words: whole, or with only its
    /// first `until` instructions where that is given.
    ///
    /// # Errors
    ///
    /// The decoding tables could not be read, or the translator refused the program.
    pub fn translate(&self, until: Option<usize>) -> Result<Vec<u32>, String> {
        let encodings = EncodingTable::builtin().map_err(|e| e.to_string())?;
        let operands = OperandTable::builtin().map_err(|e| e.to_string())?;
        let mut decoded = decode_program(&self.program, &encodings, &operands);
        if let Some(until) = until {
            decoded.instructions.truncate(until);
        }
        translate_with_user_data(
            &decoded,
            &encodings,
            self.context.strategy,
            (self.context.stage, self.context.primitive),
            self.context.window,
            self.context.user_data,
        )
        .map(|translated| translated.module)
        .map_err(|e| e.to_string())
    }
}

/// The latest note: the module hash it is for, and the source.
static NOTED: Mutex<Option<(u64, Source)>> = Mutex::new(None);

/// Notes that the module hashed `module` was translated from `program` at `address` under
/// `context`. The note is kept in place, so a program no longer than one before reuses its room.
pub fn note(module: u64, address: u64, program: &[u8], context: &Context) {
    let mut noted = NOTED
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    match noted.as_mut() {
        Some((hash, source)) => {
            *hash = module;
            source.address = address;
            source.program.clear();
            source.program.extend_from_slice(program);
            source.context = *context;
        }
        None => {
            *noted = Some((
                module,
                Source {
                    address,
                    program: program.to_vec(),
                    context: *context,
                },
            ));
        }
    }
}

/// What the module hashed `module` was translated from; `None` where the latest note is for
/// another module.
pub fn noted(module: u64) -> Option<Source> {
    NOTED
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .as_ref()
        .filter(|(hash, _)| *hash == module)
        .map(|(_, source)| source.clone())
}
