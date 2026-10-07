//! `libSceNpWebApi2`'s library and user contexts, by D151: handles the guest only compares and
//! passes back are small integers from one. PPSA28061 initializes it over the HTTP/2 context it was
//! just given and aborts on a placeholder. Assumed, not measured.

use orbistoun_core::GUEST_ARG_REGISTERS;

fn call(name: &str, args: [u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (_, handler) = orbistoun_net::implementations()
        .iter()
        .find(|(n, _)| *n == name)
        .unwrap_or_else(|| panic!("{name} is implemented"));
    handler(&args)
}

#[test]
fn library_and_user_contexts_are_positive_ids_retired_once() {
    let library = call("sceNpWebApi2Initialize", [1, 0x1_0000, 0, 0, 0, 0]);
    assert!((1..0x8000_0000).contains(&library), "{library:#x}");
    let user = call(
        "sceNpWebApi2CreateUserContext",
        [library, 0x1000_0001, 0, 0, 0, 0],
    );
    assert!((1..0x8000_0000).contains(&user), "{user:#x}");

    assert_ne!(
        call("sceNpWebApi2CreateUserContext", [0x7fff, 1, 0, 0, 0, 0]),
        user,
        "under a library never issued, no user context"
    );
    assert_eq!(
        call("sceNpWebApi2DeleteUserContext", [user, 0, 0, 0, 0, 0]),
        0
    );
    assert_ne!(
        call("sceNpWebApi2DeleteUserContext", [user, 0, 0, 0, 0, 0]),
        0
    );
    assert_eq!(call("sceNpWebApi2Terminate", [library, 0, 0, 0, 0, 0]), 0);
    assert_ne!(call("sceNpWebApi2Terminate", [library, 0, 0, 0, 0, 0]), 0);
}
