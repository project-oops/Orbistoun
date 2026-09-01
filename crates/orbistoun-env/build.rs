//! Stamps the commit this binary was built from.
//!
//! The work is `oops_build::emit`, shared with the rest of the collection. This project wrote
//! the better half of it - asking git directly, handling a modified tree, shortening a hash -
//! and prosperous wrote the other half, a readable build time and an assembled line. Each was
//! missing what the other had. See oops-libs D002.
fn main() {
    oops_build::emit();
}
