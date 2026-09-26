//! Instruction names, for reports.
//!
//! `data/mnemonics.toml` is emitted by `orbistoun-gen fixtures`: every entry is an
//! instruction a compiler emitted and a reference disassembler named, so the table covers
//! exactly what the fixture set exercises. Names make a worklist read as `v_mad_f32`
//! instead of `VOP3:0x1c1`; an instruction with no entry reports as family and opcode
//! rather than an invented name.

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
                // Two names for one opcode is a generator fault; refuse it at load.
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
        // Runs once per distinct blocker, not per instruction, so the key allocation is
        // off any hot path.
        self.names
            .get(&(family.to_owned(), opcode))
            .map(String::as_str)
    }

    /// Every entry, as (family, opcode, name).
    ///
    /// For merging into the encoding table, which dispatches on names.
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

    /// A name is keyed by family and opcode together.
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
        // The same opcode in a different family is a different instruction.
        assert_eq!(table.name("VOP2", 1), None);
    }

    /// A table naming one opcode twice fails to load.
    #[test]
    fn an_opcode_named_twice_is_refused() {
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

    /// An empty mnemonic table loads; reports then show opcode numbers.
    #[test]
    fn an_empty_table_is_allowed() {
        let table = MnemonicTable::load("").expect("empty is fine");
        assert!(table.is_empty());
        assert_eq!(table.name("VOP1", 0), None);
    }

    /// The built-in table loads and holds a known fixture instruction.
    #[test]
    fn the_builtin_table_loads_and_holds_what_the_fixtures_observed() {
        let table = MnemonicTable::builtin().expect("builtin");
        assert!(
            !table.is_empty(),
            "regenerate with tools/shader-fixtures/generate.sh"
        );
        // A real fixture instruction catches a table emitted with the wrong shape.
        assert!(
            table
                .name("VOP1", 1)
                .is_some_and(|n| n.starts_with("v_mov")),
            "VOP1:1 should be a move, got {:?}",
            table.name("VOP1", 1)
        );
    }
}
