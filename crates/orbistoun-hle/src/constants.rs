//! The numbers a FreeBSD header states, read rather than remembered.
//!
//! In `orbistoun-hle`, below both `orbistoun-fs` and `orbistoun-libc`, so either side reads one
//! copy of every number and none is retyped by hand. Only `#define NAME <number>` is taken,
//! with nothing evaluated; the harvest names and counts every candidate it skips, because a
//! spelling rule can silently take the wrong set (D385).

/// A constant harvested from a FreeBSD header, by section and name.
///
/// The values are in `data/abi-constants.toml`, generated from the headers with the commit they
/// came from (D352). Values differ between platforms (`SOL_SOCKET` is `0xffff` here and `1` on
/// several others), so none is recalled from memory.
///
/// # Panics
///
/// Never in practice: the file is embedded and a test walks every name this code asks for. A
/// miss returns [`None`] rather than a default, because a wrong constant is worse than none.
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
/// For tables read whole: a syscall dispatcher needs every number at once without writing
/// the names down (D378).
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
/// These have no header to trace to; they are numbers a guest was observed asking for.
/// Kept apart from [`abi_constants_in`], whose file is generated from headers and never
/// hand-edited, so that every row there stays traceable.
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
