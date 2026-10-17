//! The statistics a title's universal-data events keep on the console, and the trophies they earn.
//!
//! A title does not unlock its trophies directly: it posts events (`_UnlockTrophy` with a
//! `_trophy_id`, `_UpdateTrophyProgress`, or its own activity events), and the console turns each
//! into statistics by the title's extraction rules, `sce_sys/uds/udsNN.ucp`'s
//! `stats_extraction.json` and `stats_definition.json`. A trophy whose unlock condition names a
//! statistic is earned when that statistic meets its target. The console keeps the statistics
//! locally and syncs them later, so an offline console earns trophies the same way.
//!
//! The rules are the title's own data; only numeric statistics a trophy can depend on are kept
//! (`count`, `sum`, `max`, `min`, `latest`). Collections and enumerations, which feed activity
//! cards rather than trophies, are read past.

use std::collections::BTreeMap;

/// A value a posted event carries.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    /// A signed or unsigned integer.
    Int(i64),
    /// A floating-point number.
    Float(f64),
    /// A string.
    Text(String),
}

impl Value {
    /// The value as an integer statistic's input, where it is an integer or a string spelling one.
    fn integer(&self) -> Option<i64> {
        match self {
            Self::Int(value) => Some(*value),
            Self::Float(_) => None,
            Self::Text(text) => text.parse().ok(),
        }
    }

    /// Whether the value compares to `expected`, a rule's condition value, by `comparator`: as
    /// numbers where both are numbers, as strings where both are strings.
    fn meets(&self, comparator: Comparator, expected: &str) -> bool {
        match self {
            Self::Int(value) => expected
                .parse::<i64>()
                .is_ok_and(|expected| comparator.holds(value, &expected)),
            Self::Float(value) => expected
                .parse::<f64>()
                .is_ok_and(|expected| comparator.holds(value, &expected)),
            Self::Text(text) => match (text.parse::<i64>(), expected.parse::<i64>()) {
                (Ok(left), Ok(right)) => comparator.holds(&left, &right),
                _ => comparator.holds(&text.as_str(), &expected),
            },
        }
    }
}

/// An event a title posts: its name, and its properties by JSON path, `$.name`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Event {
    /// The event's name, as the title's event definitions spell it.
    pub name: String,
    /// Each property, by its path from the event's root.
    pub properties: BTreeMap<String, Value>,
}

/// How a condition compares two values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Comparator {
    /// Equal.
    Equal,
    /// Not equal.
    NotEqual,
    /// Greater than or equal.
    AtLeast,
    /// Greater than.
    Above,
    /// Less than or equal.
    AtMost,
    /// Less than.
    Below,
}

impl Comparator {
    /// The comparator a rule or unlock condition spells: `==`, `ge`, `>=` and the rest.
    #[must_use]
    pub fn parse(spelled: &str) -> Option<Self> {
        match spelled {
            "==" | "eq" => Some(Self::Equal),
            "!=" | "ne" => Some(Self::NotEqual),
            ">=" | "ge" => Some(Self::AtLeast),
            ">" | "gt" => Some(Self::Above),
            "<=" | "le" => Some(Self::AtMost),
            "<" | "lt" => Some(Self::Below),
            _ => None,
        }
    }

    /// Whether `left` compares to `right` this way.
    #[must_use]
    pub fn holds<T: PartialOrd>(self, left: &T, right: &T) -> bool {
        match self {
            Self::Equal => left == right,
            Self::NotEqual => left != right,
            Self::AtLeast => left >= right,
            Self::Above => left > right,
            Self::AtMost => left <= right,
            Self::Below => left < right,
        }
    }
}

/// How a statistic folds in each value a rule gives it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Aggregation {
    /// One more for each matching event.
    Count,
    /// The total of the values.
    Sum,
    /// The greatest value.
    Max,
    /// The least value.
    Min,
    /// The last value.
    Latest,
}

/// One numeric statistic's definition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Statistic {
    /// How it folds values in.
    aggregation: Aggregation,
    /// The value it starts at.
    initial: i64,
    /// The least it may hold.
    least: Option<i64>,
    /// The most it may hold.
    most: Option<i64>,
}

/// One extraction rule: the event it matches, an optional property condition, and the statistic
/// it feeds with the value at `input` (nothing, for a count).
#[derive(Debug, Clone, PartialEq)]
struct Rule {
    /// The event's name.
    event: String,
    /// A property the event must carry, how it compares, and to what.
    condition: Option<(String, Comparator, String)>,
    /// The property whose value feeds the statistic.
    input: Option<String>,
    /// The statistic fed.
    stat: u64,
}

/// A title's extraction rules and the statistics they keep.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Rules {
    /// Each rule, in the title's order.
    rules: Vec<Rule>,
    /// Each numeric statistic, by identifier.
    statistics: BTreeMap<u64, Statistic>,
}

/// An integer the title's rules spell as a string or a number.
fn read_integer(value: &serde_json::Value) -> Option<i64> {
    match value {
        serde_json::Value::String(text) => text.parse().ok(),
        serde_json::Value::Number(number) => number.as_i64(),
        _ => None,
    }
}

/// A condition value, as text, however the rule spells it.
fn read_text(value: &serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::String(text) => Some(text.clone()),
        serde_json::Value::Number(number) => Some(number.to_string()),
        serde_json::Value::Bool(flag) => Some(flag.to_string()),
        _ => None,
    }
}

impl Rules {
    /// The rules in `pack`, a universal-data pack, or `None` when it holds no readable rules.
    #[must_use]
    pub fn read(pack: &[u8]) -> Option<Self> {
        let entry = |name: &str| -> Option<serde_json::Value> {
            serde_json::from_slice(crate::trophy::pack_entry(pack, name)?).ok()
        };
        let definitions = entry("stats_definition.json")?;
        let extraction = entry("stats_extraction.json")?;
        let mut rules = Self::default();
        for definition in definitions.get("statDefinitionArray")?.as_array()? {
            let Some(id) = definition.get("statId").and_then(serde_json::Value::as_u64) else {
                continue;
            };
            let aggregation = match definition.get("aggregation").and_then(|a| a.as_str()) {
                Some("count") => Aggregation::Count,
                Some("sum") => Aggregation::Sum,
                Some("max") => Aggregation::Max,
                Some("min") => Aggregation::Min,
                Some("latest") => Aggregation::Latest,
                _ => continue,
            };
            let number = |key: &str| definition.get(key).and_then(read_integer);
            rules.statistics.insert(
                id,
                Statistic {
                    aggregation,
                    initial: number("initialValue").unwrap_or(0),
                    least: number("minValue"),
                    most: number("maxValue"),
                },
            );
        }
        for rule in extraction.get("statsExtractionRuleArray")?.as_array()? {
            let condition = rule.get("condition");
            let action = rule.get("action");
            let Some(stat) = action
                .and_then(|a| a.get("output"))
                .and_then(|o| o.get("statId"))
                .and_then(serde_json::Value::as_u64)
            else {
                continue;
            };
            if !rules.statistics.contains_key(&stat) {
                continue;
            }
            let Some(event) = condition
                .and_then(|c| c.get("eventName"))
                .and_then(|e| e.as_str())
            else {
                continue;
            };
            let property = condition.and_then(|c| c.get("property"));
            let compared = property.and_then(|p| {
                Some((
                    p.get("path")?.as_str()?.to_owned(),
                    Comparator::parse(p.get("comparator")?.as_str()?)?,
                    read_text(p.get("value")?)?,
                ))
            });
            if property.is_some() && compared.is_none() {
                continue;
            }
            rules.rules.push(Rule {
                event: event.to_owned(),
                condition: compared,
                input: action
                    .and_then(|a| a.get("input"))
                    .and_then(|i| i.as_str())
                    .map(str::to_owned),
                stat,
            });
        }
        Some(rules)
    }

    /// Folds `event` into `stats` by every rule it matches, and answers the statistics it changed.
    pub fn apply(&self, stats: &mut BTreeMap<u64, i64>, event: &Event) -> Vec<u64> {
        let mut changed = Vec::new();
        for rule in self.rules.iter().filter(|rule| rule.event == event.name) {
            if let Some((path, comparator, expected)) = &rule.condition {
                let Some(actual) = event.properties.get(path) else {
                    continue;
                };
                if !actual.meets(*comparator, expected) {
                    continue;
                }
            }
            let Some(statistic) = self.statistics.get(&rule.stat) else {
                continue;
            };
            let input = rule
                .input
                .as_ref()
                .and_then(|path| event.properties.get(path))
                .and_then(Value::integer);
            let current = stats.get(&rule.stat).copied().unwrap_or(statistic.initial);
            let next = match (statistic.aggregation, input) {
                (Aggregation::Count, _) => current.saturating_add(1),
                (Aggregation::Sum, Some(value)) => current.saturating_add(value),
                (Aggregation::Max, Some(value)) => current.max(value),
                (Aggregation::Min, Some(value)) => current.min(value),
                (Aggregation::Latest, Some(value)) => value,
                _ => continue,
            };
            let next = statistic.most.map_or(next, |most| next.min(most));
            let next = statistic.least.map_or(next, |least| next.max(least));
            if stats.insert(rule.stat, next) != Some(next) {
                changed.push(rule.stat);
            }
        }
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The rules of a pack holding a trophy-unlock count, a progress maximum, and an activity
    /// collection that is read past.
    fn sample_rules() -> Rules {
        let definitions = br#"{"statDefinitionArray":[
            {"statId":1,"aggregation":"collection","dataType":"enumSet"},
            {"statId":19,"aggregation":"count","maxValue":"1","minValue":"0","initialValue":"0"},
            {"statId":40,"aggregation":"max","initialValue":"0"}]}"#;
        let extraction = br#"{"statsExtractionRuleArray":[
            {"ruleId":1,"condition":{"eventName":"ActivityStart"},"action":{"input":"$.activityId","output":{"statId":1}}},
            {"ruleId":27,"condition":{"eventName":"_UnlockTrophy","property":{"path":"$._trophy_id","comparator":"==","value":"1"}},
             "action":{"output":{"statId":19}}},
            {"ruleId":40,"condition":{"eventName":"_UpdateTrophyProgress","property":{"path":"$._trophy_id","comparator":"==","value":"3"}},
             "action":{"input":"$._trophy_progress","output":{"statId":40}}}]}"#;
        let pack = crate::trophy::tests::pack_of(&[
            ("stats_definition.json", definitions),
            ("stats_extraction.json", extraction),
        ]);
        Rules::read(&pack).expect("the sample rules read")
    }

    /// An event of `name` with integer properties.
    fn event(name: &str, properties: &[(&str, i64)]) -> Event {
        Event {
            name: name.to_owned(),
            properties: properties
                .iter()
                .map(|(path, value)| ((*path).to_owned(), Value::Int(*value)))
                .collect(),
        }
    }

    /// Only the numeric statistics and the rules feeding them are kept.
    #[test]
    fn rules_keep_the_numeric_statistics() {
        let rules = sample_rules();
        assert_eq!(rules.statistics.len(), 2);
        assert_eq!(rules.rules.len(), 2);
    }

    /// An unlock event counts its trophy's statistic once, capped at its most; another trophy's
    /// event leaves it.
    #[test]
    fn an_unlock_event_counts_its_statistic_up_to_its_most() {
        let rules = sample_rules();
        let mut stats = BTreeMap::new();
        assert_eq!(
            rules.apply(&mut stats, &event("_UnlockTrophy", &[("$._trophy_id", 2)])),
            Vec::<u64>::new()
        );
        assert_eq!(
            rules.apply(&mut stats, &event("_UnlockTrophy", &[("$._trophy_id", 1)])),
            vec![19]
        );
        assert_eq!(
            rules.apply(&mut stats, &event("_UnlockTrophy", &[("$._trophy_id", 1)])),
            Vec::<u64>::new()
        );
        assert_eq!(stats, BTreeMap::from([(19, 1)]));
    }

    /// A progress event keeps the greatest progress it was given.
    #[test]
    fn a_progress_event_keeps_its_greatest_value() {
        let rules = sample_rules();
        let mut stats = BTreeMap::new();
        let progress = |value| {
            event(
                "_UpdateTrophyProgress",
                &[("$._trophy_id", 3), ("$._trophy_progress", value)],
            )
        };
        assert_eq!(rules.apply(&mut stats, &progress(10)), vec![40]);
        assert_eq!(rules.apply(&mut stats, &progress(4)), Vec::<u64>::new());
        assert_eq!(rules.apply(&mut stats, &progress(25)), vec![40]);
        assert_eq!(stats[&40], 25);
    }

    /// Comparators read as both the rules and the trophy conditions spell them.
    #[test]
    fn comparators_read_both_spellings() {
        assert_eq!(Comparator::parse("ge"), Some(Comparator::AtLeast));
        assert_eq!(Comparator::parse(">="), Some(Comparator::AtLeast));
        assert!(Comparator::AtLeast.holds(&25, &25));
        assert!(!Comparator::Above.holds(&25, &25));
        assert_eq!(Comparator::parse("~"), None);
    }
}
