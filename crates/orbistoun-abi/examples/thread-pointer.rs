//! Whether this machine can install a guest thread pointer.
//!
//! `CPUID` says whether the processor has the feature; the operating system must also
//! have enabled it, and nothing in user code can observe that. So the only honest check
//! is to write the base and read it back - which is what this does, on a scratch block,
//! restoring whatever the host was using afterwards.
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
        // The host owns this thread; leaving its base pointing at a stack array would
        // break whatever runs on it next.
        // SAFETY: restoring a value this thread was already using.
        unsafe {
            let _ = thread_pointer::install(previous);
        }
    }
}
