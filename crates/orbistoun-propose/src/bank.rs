//! Keeping the words that worked.
//!
//! Proposing vocabulary pays only if it compounds: a word learned once reaches every later
//! title. The bank keeps only words a confirmed name was built from, so a wrong proposal costs
//! nothing, and a right word with no matching import here can be proposed again. It is this
//! crate's own file, separate from `orbistoun-names/data/vendor.toml`, which the name search
//! generates and owns; a caller merges the bank into a grammar in memory, and promoting words
//! into the shipped vocabulary is a deliberate change with a diff.

use std::collections::BTreeSet;
use std::path::PathBuf;

use crate::Error;

/// The header written above the words, so the file explains itself.
const HEADER: &str = concat!(
    "# Words a model proposed that the NID hash then confirmed.\n",
    "#\n",
    "# Every entry here was part of a name that collided with an import a real module\n",
    "# declared. Words that were proposed and led nowhere are not kept - the hash is the\n",
    "# only thing that puts a word in this file.\n",
    "#\n",
    "# Generated. Sorted, one per line, so a diff shows what a run actually learned.\n",
    "# Promoting these into crates/orbistoun-names/data/vendor.toml is a separate and\n",
    "# deliberate act.\n",
);

/// Words kept between runs.
#[derive(Debug, Clone)]
pub struct Bank {
    path: PathBuf,
    words: BTreeSet<String>,
}

impl Bank {
    /// Opens a bank, treating a missing file as an empty one, since the first run has nothing
    /// kept.
    ///
    /// # Errors
    ///
    /// If the file exists and cannot be read.
    pub fn open(path: impl Into<PathBuf>) -> Result<Self, Error> {
        let path = path.into();
        let words = match std::fs::read_to_string(&path) {
            Ok(text) => text
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty() && !line.starts_with('#'))
                .map(str::to_owned)
                .collect(),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => BTreeSet::new(),
            Err(error) => {
                return Err(Error::Reply(format!("reading {}: {error}", path.display())));
            }
        };
        Ok(Self { path, words })
    }

    /// Every word kept so far.
    pub fn words(&self) -> &BTreeSet<String> {
        &self.words
    }

    /// Whether anything has been kept.
    pub fn is_empty(&self) -> bool {
        self.words.is_empty()
    }

    /// How many words are kept.
    pub fn len(&self) -> usize {
        self.words.len()
    }

    /// Adds words, and says how many were new.
    ///
    /// A round that earned names only from words already banked learned nothing, and the count
    /// shows it.
    pub fn add<I, S>(&mut self, words: I) -> usize
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let mut added = 0;
        for word in words {
            if self.words.insert(word.into()) {
                added += 1;
            }
        }
        added
    }

    /// Writes the bank, creating the directory if it is missing.
    ///
    /// # Errors
    ///
    /// If the directory cannot be created or the file cannot be written.
    pub fn save(&self) -> Result<(), Error> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| Error::Reply(format!("creating {}: {e}", parent.display())))?;
        }
        let mut text = String::from(HEADER);
        for word in &self.words {
            text.push_str(word);
            text.push('\n');
        }
        std::fs::write(&self.path, text)
            .map_err(|e| Error::Reply(format!("writing {}: {e}", self.path.display())))
    }
}

#[cfg(test)]
mod tests {
    use super::Bank;

    /// A missing bank is an empty one, not a failure.
    #[test]
    fn a_missing_bank_is_empty() {
        let dir = tempfile::tempdir().expect("temp dir");
        let bank = Bank::open(dir.path().join("nothing-here.txt")).expect("opens");
        assert!(bank.is_empty());
    }

    /// Words survive a round trip, and comments do not become words.
    #[test]
    fn words_survive_a_round_trip() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("nested").join("words.txt");

        let mut bank = Bank::open(&path).expect("opens");
        assert_eq!(bank.add(["Async", "Sema"]), 2);
        bank.save().expect("saves");

        let reopened = Bank::open(&path).expect("reopens");
        assert_eq!(reopened.len(), 2);
        assert!(reopened.words().contains("Async"));
        assert!(
            !reopened.words().iter().any(|w| w.starts_with('#')),
            "a comment became a word"
        );
    }

    /// Adding a word already kept reports zero new.
    #[test]
    fn adding_a_known_word_is_reported_as_nothing_new() {
        let dir = tempfile::tempdir().expect("temp dir");
        let mut bank = Bank::open(dir.path().join("words.txt")).expect("opens");
        assert_eq!(bank.add(["Async"]), 1);
        assert_eq!(bank.add(["Async"]), 0);
        assert_eq!(bank.len(), 1);
    }

    /// The file is sorted, so a diff shows what a run learned.
    #[test]
    fn the_file_is_sorted() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("words.txt");
        let mut bank = Bank::open(&path).expect("opens");
        bank.add(["Zeta", "Alpha", "Mu"]);
        bank.save().expect("saves");

        let text = std::fs::read_to_string(&path).expect("read");
        let words: Vec<&str> = text
            .lines()
            .filter(|l| !l.starts_with('#') && !l.is_empty())
            .collect();
        assert_eq!(words, vec!["Alpha", "Mu", "Zeta"]);
    }

    /// An unreadable bank is an error rather than a silent fresh start that the next save would
    /// overwrite.
    #[test]
    fn an_unreadable_bank_is_an_error() {
        let dir = tempfile::tempdir().expect("temp dir");
        // A directory where a file is expected: readable as an entry, not as text.
        let path = dir.path().join("words.txt");
        std::fs::create_dir(&path).expect("dir");
        assert!(Bank::open(&path).is_err());
    }
}
