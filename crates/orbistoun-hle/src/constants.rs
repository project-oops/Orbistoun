//! The numbers a FreeBSD header states, read rather than remembered.
//!
//! # Why this is a crate everything can reach
//!
//! It lived in `orbistoun-libc`, which is near the top of the tree - so `orbistoun-fs`, which
//! `orbistoun-libc` depends on, could not read it and wrote its own constants down by hand
//! with a citation in a comment. That is exactly the retyping the harvest exists to prevent:
//! **a value typed in by a person is one nobody can trace back to a header**, and a comment
//! saying where it came from is a claim rather than a link.
//!
//! `orbistoun-hle` is below both, so the table is readable from either side and there is one
//! copy of every number (D385).
//!
//! # What is in it and what is not
//!
//! Only `#define NAME <number>` - no expressions, nothing evaluated. What was skipped is
//! **counted and named** by the harvest rather than dropped in silence, because three times
//! a rule about spelling has quietly taken the wrong set and only a count noticed: the
//! upper-case rule took one constant of six hundred from `syscall.h` (D378), C octal made
//! the whole table unparseable (D374), and requiring bare digits took none of the fifteen
//! event filters (D385).

/// A constant harvested from a FreeBSD header, by section and name.
///
/// # Why this is read rather than written down
///
/// The values are in `data/abi-constants.toml`, generated from the headers and carrying the
/// commit they came from. Retyping one into Rust would make it **untraceable** - a reader
/// could no longer tell a harvested value from a remembered one, which is the whole
/// distinction `known_by` exists to keep (D351).
///
/// It also stops them being wrong. `SOL_SOCKET` is `0xffff` on this platform and `1` on
/// several others; a value recalled rather than read is the failure that shows up as a
/// socket option silently doing nothing.
///
/// # Panics
///
/// Never in practice - the file is embedded and a test walks every name this code asks for.
/// A miss returns [`None`] rather than a default, because a wrong constant is worse than an
/// absent one.
#[must_use]
pub fn abi_constant(section: &str, name: &str) -> Option<i64> {
    constants()
        .get(section)?
        .as_table()?
        .get(name)?
        .as_integer()
}

/// Every constant in one harvested section, by name.
///
/// **For the tables that are read whole rather than asked one question.** A syscall
/// dispatcher needs all six hundred numbers at once, and asking for them by name would mean
/// writing the six hundred names down - which is the retyping this whole arrangement exists
/// to avoid (D378).
#[must_use]
pub fn abi_constants_in(section: &str) -> Vec<(String, i64)> {
    let Some(table) = constants().get(section).and_then(toml::Value::as_table) else {
        return Vec::new();
    };
    table
        .iter()
        .filter_map(|(name, value)| Some((name.clone(), value.as_integer()?)))
        .collect()
}

/// Constants the target has that FreeBSD does not, kept in their own table.
///
/// # Why these are not merged into the harvested ones
///
/// [`abi_constants_in`] reads a file generated from headers, whose own comment forbids
/// hand-editing on the grounds that a typed-in value cannot be traced back to a source. These
/// have no header to be traced to - they are numbers a guest was watched asking for. Putting
/// them in the same table would make the generated file's guarantee false for some of its rows
/// and there would be no way to tell which (D403).
#[must_use]
pub fn vendor_constants_in(section: &str) -> Vec<(String, i64)> {
    let Some(table) = vendor().get(section).and_then(toml::Value::as_table) else {
        return Vec::new();
    };
    table
        .iter()
        .filter_map(|(name, value)| Some((name.clone(), value.as_integer()?)))
        .collect()
}

/// The observed table, parsed once.
fn vendor() -> &'static toml::Table {
    use std::sync::OnceLock;
    static TABLE: OnceLock<toml::Table> = OnceLock::new();
    TABLE.get_or_init(|| {
        include_str!("../data/vendor-syscalls.toml")
            .parse::<toml::Table>()
            .expect("the observed constants must parse")
    })
}

/// The harvested table, parsed once.
fn constants() -> &'static toml::Table {
    use std::sync::OnceLock;
    static TABLE: OnceLock<toml::Table> = OnceLock::new();
    TABLE.get_or_init(|| {
        include_str!("../data/abi-constants.toml")
            .parse::<toml::Table>()
            .expect("the harvested constants must parse")
    })
}
