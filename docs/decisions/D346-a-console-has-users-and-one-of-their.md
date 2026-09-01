# D346 - A console has users, and one of their names reaches a guest unencoded


**decided** · 2026-08-27 · the settings argument, finally with a caller attached

`libSceUserService` is sixteen imports across the corpus, and reading them is what decided
the shape. A title asks for a name **by identifier**, enumerates who is signed in, and keys
save data on which user - none of which a single `user_name` field can answer. So `Settings`
holds a list.

**`sceUserServiceGetUserName` is the one call in this crate that answers a console setting
unencoded.** Everything else here hands back a number whose meaning is a measurement: an age
band, an accessibility flag, a parameter identifier. A name is a string. The owner types it
in the shell, the guest reads it, and nothing in between has to be guessed. That is the
argument this project made for holding settings at all, with a real caller at the end of it.

Everything else in the library is **declared and not implemented**, for the reason the empty
parameter table exists: a person's answer to "what age level" is not the integer a title
reads, and inventing the encoding would be principle 3's forbidden case with a consumer.

### The size is checked rather than trusted

That `size` is the third argument is an assumption, and a wrong argument position writes past
a caller's buffer - the failure `sceUserServiceGetInitialUser` already carries a warning about
(D210, D272). So it is believed only within sixty-four bytes; anything outside that is treated
as a value that is not a size, and the call is **refused**. A refusal is recoverable and a
smashed stack is not, and the test asserts the refusal rather than the success.

Truncation happens on a character boundary, never a byte one: half a multi-byte character is
a string that is not text.

### Three things that were modelled and inert

**`console::configure` was never called.** Written that morning, so every setting a person
chose stopped at the window and the guest read defaults. The worker now reads `shell.toml`
itself - it is a separate process that already reads its own configuration, and a setting
cannot arrive half-applied because a message was dropped.

**`sceUserServiceGetInitialUser` answered a constant.** It now answers whoever is signed in,
and answers the placeholder when nobody is - which is the honest answer to "who is signed in"
on a machine where the signed-in account was deleted.

**A test caught the identifier reuse before the code did.** `next_id` derived the next number
from the highest *live* identifier, which reuses one the moment its holder is deleted - and
save data is keyed on it, so a new person would silently inherit somebody else's saves. It is
a stored high-water mark now, the same shape as `HandleAllocator`. The test was written first
and failed, which is the only reason it was found.


