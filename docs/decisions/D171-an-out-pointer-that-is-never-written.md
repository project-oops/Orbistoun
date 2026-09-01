# D171 - An out-pointer that is never written has no signature


**decided** · 2026-08-21

`sceSystemServiceParamGetInt` answers a console setting through an out-pointer.
Unimplemented, it wrote **nothing**, and the guest read whatever its stack held there.

That is a worse failure than a wrong return value, and worth separating from it. Every
other unimplemented call in this project answers wrongly but *consistently* - the same
placeholder every run, recognisable in a trace, in a range no real code occupies. An
unwritten out-pointer answers differently on every run, depending on what last used that
stack slot, and leaves **nothing to recognise** because nothing was written.

So `orbistoun-systemservice` exists and always writes. Zero is a stated placeholder rather
than a value: read as an index it lands on the first entry, as a flag it reads as off, as a
count it reads as none - all ordinary states a title must already handle. A non-zero guess
would be picking a behaviour out of the air and calling it a default.

Success is reported rather than an error, deliberately: a guest that checks the return skips
whatever the setting was for, while one that does not check reads the value regardless -
which is why it must be written either way.

**It did not move the wall.** The call is reached, the implementation is correct, and
`image+0x43c4` is unchanged - so the hypothesis was wrong. Recorded because a wrong
hypothesis that was cheap to test and left the code better is a fine outcome, and because
the out-pointer class of failure is real whether or not it was this one.

