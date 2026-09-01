//! Instruction names, for reports.
//!
//! # Generated, and only from what was observed
//!
//! `data/mnemonics.toml` is emitted by `orbistoun-gen fixtures`. Every entry in
//! it was seen: a compiler emitted the instruction and a reference disassembler named
//! it, so the table is verified by construction rather than transcribed and hoped for.
//!
//! That means it is **incomplete by design**, covering only what the fixture set
//! exercises. Widening it is a matter of adding fixtures, which also widens what the
//! differential test proves - the two grow together, which is the right coupling.
//!
//! # Nothing dispatches on a name
//!
//! Names exist so a worklist reads as `v_mad_f32` instead of `VOP3:0x1c1`. An
//! instruction with no entry reports as its family and opcode, which is legible enough
//! to look up. A missing name costs a reader ten seconds; an invented one sends them
//! to the wrong instruction, so absence is reported rather than filled in.

use std::collections::BTreeMap;

use serde::Deserialize;

use crate::ShaderError;

#[derive(Debug, Clone, Deserialize)]
struct Entry {
    family: String,
    opcode: u32,
    name: String,
}

#[derive(Debug, Deserialize, Default)]
struct TableFile {
    #[serde(default)]
    target: String,
    #[serde(default)]
    mnemonic: Vec<Entry>,
}

/// Names for instructions, keyed by family and opcode.
#[derive(Debug, Clone, Default)]
pub struct MnemonicTable {
    names: BTreeMap<(String, u32), String>,
    /// The architecture generation this table was generated against.
    target: String,
}

impl MnemonicTable {
    /// Parses a table from TOML.
    pub fn load(toml_text: &str) -> Result<Self, ShaderError> {
        let file: TableFile =
            toml::from_str(toml_text).map_err(|e| ShaderError::Table(e.to_string()))?;
        let mut names = BTreeMap::new();
        for entry in file.mnemonic {
            if let Some(previous) = names.insert((entry.family.clone(), entry.opcode), entry.name) {
                // Two names for one opcode means the generator's classification is
                // wrong, and whichever won would be arbitrary. Refusing surfaces it at
                // load rather than as a puzzling report weeks later.
                return Err(ShaderError::Table(format!(
                    "{}:{:#x} is named twice, first as {previous}",
                    entry.family, entry.opcode
                )));
            }
        }
        Ok(Self {
            names,
            target: file.target,
        })
    }

    /// The built-in table.
    pub fn builtin() -> Result<Self, ShaderError> {
        Self::load(include_str!("../data/mnemonics.toml"))
    }

    /// The name for an instruction, if one has been observed.
    pub fn name(&self, family: &str, opcode: u32) -> Option<&str> {
        // Allocating a key to look one up is wasteful, but this runs once per distinct
        // blocker in a report rather than once per instruction, so it stays off any
        // hot path.
        self.names
            .get(&(family.to_owned(), opcode))
            .map(String::as_str)
    }

    /// Every entry, as (family, opcode, name).
    ///
    /// For merging into the encoding table, which dispatches on names and needs every
    /// source of them in one place.
    pub fn entries(&self) -> impl Iterator<Item = (&str, u32, &str)> {
        self.names
            .iter()
            .map(|((family, opcode), name)| (family.as_str(), *opcode, name.as_str()))
    }

    /// How many names are known.
    pub fn len(&self) -> usize {
        self.names.len()
    }

    /// Whether the table is empty.
    pub fn is_empty(&self) -> bool {
        self.names.is_empty()
    }

    /// The architecture generation this table was generated against.
    pub fn target(&self) -> &str {
        &self.target
    }
}

#[cfg(test)]
mod tests {
    use super::MnemonicTable;

    #[test]
    fn a_name_is_found_by_family_and_opcode() {
        let table = MnemonicTable::load(
            r#"
            [[mnemonic]]
            family = "VOP1"
            opcode = 1
            name = "v_mov_b32"
            "#,
        )
        .expect("table");
        assert_eq!(table.name("VOP1", 1), Some("v_mov_b32"));
        // The same opcode in a different family is a different instruction entirely.
        assert_eq!(table.name("VOP2", 1), None);
    }

    #[test]
    fn an_opcode_named_twice_is_refused() {
        // It means the generator classified something wrongly, and whichever entry won
        // would be arbitrary. Better to fail at load than to report a wrong name.
        let result = MnemonicTable::load(
            r#"
            [[mnemonic]]
            family = "VOP1"
            opcode = 1
            name = "first"

            [[mnemonic]]
            family = "VOP1"
            opcode = 1
            name = "second"
            "#,
        );
        assert!(result.is_err());
    }

    #[test]
    fn an_empty_table_is_allowed() {
        // Unlike the encoding table, an empty mnemonic table is harmless - reports
        // simply show opcode numbers. Refusing it would make the names a hard
        // dependency of a tool that works without them.
        let table = MnemonicTable::load("").expect("empty is fine");
        assert!(table.is_empty());
        assert_eq!(table.name("VOP1", 0), None);
    }

    #[test]
    fn the_builtin_table_loads_and_holds_what_the_fixtures_observed() {
        let table = MnemonicTable::builtin().expect("builtin");
        assert!(
            !table.is_empty(),
            "regenerate with tools/shader-fixtures/generate.sh"
        );
        // A real instruction from the fixture set, to catch the table being emitted
        // with the wrong shape rather than merely being non-empty.
        assert!(
            table
                .name("VOP1", 1)
                .is_some_and(|n| n.starts_with("v_mov")),
            "VOP1:1 should be a move, got {:?}",
            table.name("VOP1", 1)
        );
    }
}
