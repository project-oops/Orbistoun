//! `libSceSysmodule`'s unwind query answers as libkernel's does (D778).

/// The two queries answer an address in no placed module the same way, with the same block: the
/// sysmodule spelling is the kernel's query under another library's name. PPSA02664's libc asks it
/// 1100 times a run.
#[test]
fn the_sysmodule_unwind_query_is_the_kernels() {
    let kernel = orbistoun_service::implementation_named("sceKernelGetModuleInfoForUnwind")
        .expect("the kernel's query is implemented");
    let sysmodule = orbistoun_service::implementation_named("sceSysmoduleGetModuleInfoForUnwind")
        .expect("the sysmodule spelling is implemented");
    // The block states its full size first, as the unwinder fills it.
    let answer = |query: orbistoun_core::GuestFn| {
        let mut block = vec![0_u64; 0x30];
        block[0] = 0x130;
        let at = block.as_mut_ptr().expose_provenance() as u64;
        let mut args = [0_u64; orbistoun_core::GUEST_ARG_REGISTERS];
        (args[0], args[1], args[2]) = (0x10, 0, at);
        (query(&args), block)
    };
    assert_eq!(answer(sysmodule), answer(kernel));
}
