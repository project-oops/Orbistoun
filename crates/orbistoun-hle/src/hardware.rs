//! What a conformance run measured on the console, as data this project can be checked against.
//!
//! # Why this is a separate file from the knowledge base
//!
//! A knowledge entry describes a function. A measurement describes **one observation of one
//! condition**, and the difference is the whole point: an entry can be broadly right while a
//! specific value is wrong, and only the second kind of claim can be turned into a test that
//! passes or fails.
//!
//! The record shape comes from the probe - `check | subject | condition | observation | kind` -
//! and carries the four things a checkable claim needs. That is what `res` records lack, which
//! is why those are recorded as prose on a function and these are recorded as data.
//!
//! # Constant and varying, decided by the runs rather than by the kind
//!
//! Three captures of the same suite disagree on twenty-four of the two hundred and twenty-four
//! measurements - timestamps, mapped addresses, elapsed microseconds, module handles, and the
//! counter frequency, which is calibrated per boot. A measurement is marked
//! [`Measurement::constant`] when **every run that took it agreed**, and that is an empirical
//! answer rather than a judgement about what `ticks` ought to mean.
//!
//! **Only a constant measurement can be asserted.** A varying one is still recorded, because
//! "this value moves between runs" is a fact worth keeping and no single run shows it.

use serde::{Deserialize, Serialize};

/// One thing a conformance run observed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Measurement {
    /// `check:subject:condition`, unique, and what an assertion names to claim it.
    pub id: String,
    /// The probe check that made the observation. The condition is written down there.
    pub check: String,
    /// What was measured - usually a function, sometimes a knob name.
    pub subject: String,
    /// Which of the check's observations this is.
    pub condition: String,
    /// The probe's own type tag: `code`, `hz`, `bytes`, `ticks`, `handle` and so on.
    pub kind: String,
    /// The value, exactly as the run reported it.
    pub observation: String,
    /// Every run that reported this value.
    pub sources: Vec<String>,
    /// Whether every run that took this measurement agreed on it.
    ///
    /// **False does not mean wrong.** It means the value is not a property of the platform,
    /// so nothing may assert it - see [`Self::disagreed`] for what the other runs said.
    pub constant: bool,
    /// The other values seen, when the runs disagreed. Empty when they did not.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub disagreed: Vec<String>,
}

impl Measurement {
    /// The value as a number, when it is written as one.
    #[must_use]
    pub fn value(&self) -> Option<u64> {
        parse_number(&self.observation)
    }

    /// Every value any run reported for this, as numbers.
    ///
    /// **For the measurements that are real but not constant.** A per-boot calibration is
    /// not a property of the platform, so nothing may assert one particular value - but
    /// "orbistoun answers a number no console ever reported" is still a defect, and asserting
    /// membership of this set catches it while permitting the variation.
    ///
    /// The observation first, then whatever the other runs saw, in the order the generator
    /// wrote them.
    #[must_use]
    pub fn values(&self) -> Vec<u64> {
        std::iter::once(self.observation.as_str())
            .chain(self.disagreed.iter().map(String::as_str))
            .filter_map(parse_number)
            .collect()
    }
}

/// Reads the leading number out of a field, hexadecimal when it says so and decimal otherwise.
///
/// A `disagreed` entry carries its provenance after the value - `0x5f259b8e in ps5-full.txt`
/// - so this takes the first whitespace-delimited token rather than the whole string.
///
/// # Why decimal is read at all
///
/// **It required the prefix, and the probe does not always write one.** `000-hw/sw-version`
/// records `0` and `000-hw/tsc-frequency` records `1596300187`, so every measurement from
/// those checks answered `None` - which a caller writing `.expect("a number")` finds
/// immediately and a caller writing `.unwrap_or(0)` never finds at all. The second is the
/// dangerous shape and is the reason this is a parse rather than a guess: `0x10` is sixteen,
/// `10` is ten, and a field that is a word is still `None` (D611).
fn parse_number(text: &str) -> Option<u64> {
    let first = text.split_whitespace().next()?;
    first.strip_prefix("0x").map_or_else(
        || first.parse().ok(),
        |hex| u64::from_str_radix(hex, 16).ok(),
    )
}

/// Every measurement a set of runs produced.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Measurements {
    /// In the order the generator wrote them, which is capture order.
    #[serde(default, rename = "measurement")]
    pub measurements: Vec<Measurement>,
}

/// The generated table, as shipped.
const EMBEDDED: &str = include_str!("../data/hardware.toml");

impl Measurements {
    /// Parses a table.
    ///
    /// # Errors
    ///
    /// If the text is not the table format.
    pub fn parse(text: &str) -> Result<Self, toml::de::Error> {
        toml::from_str(text)
    }

    /// Renders one back, for the generator to write.
    ///
    /// # Errors
    ///
    /// If a measurement cannot be serialised, which the type makes impossible.
    pub fn render(&self) -> Result<String, toml::ser::Error> {
        let body = toml::to_string_pretty(self)?;
        Ok(format!(
            concat!(
                "# What a conformance run measured on the console.\n",
                "#\n",
                "# Generated by `orbistoun-gen measurements` from the capture files in the ",
                "sibling\n",
                "# conformance-probe repository. Do not edit: regenerate.\n",
                "#\n",
                "# `constant = false` means the runs disagreed, so nothing may assert it.\n\n{}"
            ),
            body
        ))
    }

    /// Loads what ships with the tool.
    ///
    /// # Panics
    ///
    /// If the shipped table is malformed, which a test in this crate rules out.
    #[must_use]
    pub fn builtin() -> Self {
        Self::parse(EMBEDDED).unwrap_or_else(|e| panic!("shipped hardware table is malformed: {e}"))
    }

    /// The measurement with this id.
    #[must_use]
    pub fn get(&self, id: &str) -> Option<&Measurement> {
        self.measurements.iter().find(|m| m.id == id)
    }

    /// Every measurement no run has contradicted, which are the ones an assertion may name.
    pub fn constants(&self) -> impl Iterator<Item = &Measurement> {
        self.measurements.iter().filter(|m| m.constant)
    }
}

#[cfg(test)]
mod tests {
    use super::Measurements;

    /// A value written without a `0x` is decimal, and a value that is a word is neither.
    ///
    /// **Asserting on the refusal and on the base.** Reading `10` as sixteen would be a wrong
    /// measurement that looks exactly like a right one, and reading a word as zero would be
    /// worse - so the failing cases are what this pins (D611).
    #[test]
    fn a_measurement_is_read_in_the_base_it_was_written_in() {
        use super::parse_number;

        assert_eq!(parse_number("0x10"), Some(16), "a prefix means hexadecimal");
        assert_eq!(
            parse_number("10"),
            Some(10),
            "and its absence means decimal"
        );
        assert_eq!(parse_number("0"), Some(0), "including plain zero");
        assert_eq!(
            parse_number("1596300187"),
            Some(1_596_300_187),
            "the frequency check writes decimal, and dropping it lost the measurement"
        );
        assert_eq!(parse_number("FreeBSD"), None, "a word is not a number");
        assert_eq!(parse_number(""), None, "and nor is nothing");
        assert_eq!(
            parse_number("0x5f259b8e in ps5-full.txt"),
            Some(0x5f25_9b8e),
            "a disagreement carries its provenance after the value"
        );
    }

    /// The shipped table parses, so [`Measurements::builtin`] cannot panic in a caller.
    #[test]
    fn the_shipped_table_is_well_formed() {
        let table = Measurements::builtin();
        assert!(
            !table.measurements.is_empty(),
            "an empty table would pass every coverage gate silently"
        );
        for measurement in &table.measurements {
            assert!(!measurement.id.is_empty());
            assert!(!measurement.subject.is_empty());
            assert!(
                !measurement.sources.is_empty(),
                "{}: a measurement with no run behind it is not a measurement",
                measurement.id
            );
            assert_eq!(
                measurement.constant,
                measurement.disagreed.is_empty(),
                "{}: constant and disagreed must not contradict each other",
                measurement.id
            );
        }
    }

    /// Ids are unique, or an assertion naming one would claim an arbitrary measurement.
    #[test]
    fn every_measurement_has_its_own_id() {
        let table = Measurements::builtin();
        let mut seen = std::collections::BTreeSet::new();
        for measurement in &table.measurements {
            assert!(
                seen.insert(measurement.id.clone()),
                "{} appears twice",
                measurement.id
            );
        }
    }

    /// A hex observation reads back as a number; anything else answers `None` rather than zero.
    #[test]
    fn a_value_that_is_not_a_number_is_not_read_as_zero() {
        let table = Measurements::builtin();
        for measurement in table.constants() {
            if measurement.observation.starts_with("0x") {
                assert!(
                    measurement.value().is_some(),
                    "{} looks numeric and does not parse",
                    measurement.id
                );
            }
        }
    }
}
