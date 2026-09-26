//! Whether this machine can install a guest thread pointer.
//!
//! `CPUID` reports the processor feature, but whether the operating system enabled it is not
//! observable from user code. This writes the base on a scratch block, reads it back, and
//! restores the host's value.
//!
//! ```text
//! cargo run -p orbistoun-abi --example thread-pointer
//! ```

use orbistoun_abi::thread_pointer;

fn main() {
    println!(
        "processor reports the feature   {}",
        thread_pointer::processor_supports_base_writes()
    );

    let mut block = [0_u64; 8];
    let address = block.as_mut_ptr() as usize as u64;
    let restore = thread_pointer::current();

    // SAFETY: a live, correctly aligned local that outlives this call, and nothing here
    // reads a thread-local through the base while it is pointed at the block.
    let outcome = unsafe { thread_pointer::install(address) };

    match outcome {
        Ok(()) => {
            let read_back = thread_pointer::current();
            println!("install                         accepted");
            println!("reads back                      {read_back:?}");
            println!(
                "verdict                         {}",
                if read_back == Some(address) {
                    "usable - guest thread-local storage will work"
                } else {
                    "REPORTED SUCCESS BUT DID NOT TAKE - do not rely on it"
                }
            );
        }
        Err(e) => println!("install                         refused: {e}"),
    }

    if let Some(previous) = restore {
        // The host owns this thread; its base must not be left pointing at a stack array.
        // SAFETY: restoring a value this thread was already using.
        unsafe {
            let _ = thread_pointer::install(previous);
        }
    }
}
