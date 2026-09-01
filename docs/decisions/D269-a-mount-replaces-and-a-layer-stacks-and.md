# D269 - A mount replaces and a layer stacks, and the order cost a title its textures


**decided** · 2026-08-25 · caught by bisecting a regression rather than by reasoning

Installing the console's base filesystem after mounting the title pointed `/app0` at an
empty directory, because `mount` replaces every root at a prefix. PPSA28061 lost its
textures and died in early setup instead of at its usual wall - 933 calls down to 790.

Found by bisection, not inspection: the first suspect was the trampoline change landing the
same afternoon, and it was innocent. The fix is one word - `mount_title` **layers** rather
than mounts - and the base goes in first.

That is an argument for the layering design rather than against it: once a mount is a stack,
"whose file wins" is answered by the order things are installed in, and the failure was
being unable to express it at all.

