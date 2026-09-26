//! Stamps the commit this binary was built from, through `oops_build::emit`, which is shared with
//! the rest of the collection.
fn main() {
    oops_build::emit();
}
