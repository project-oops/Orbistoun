# The name sweep could not have found the answer


Set out to build the `sceKernel<posix-name>` solver pattern. Measured first: 108,117
candidates from the harvested names against the sixteen unnamed hashes. Zero.

Then ran the control, because a negative from an unvalidated instrument is worth nothing.
`sceKernelMprotect` is a confirmed name of exactly that shape, so the sweep had to be able
to produce it - and it could not, because **`mprotect` is not in the harvested list**
(D200).

Nor are mmap, munmap, lseek, pread, openat, fstat, ioctl, execve, getpid or kill. The
syscall family is largely missing while stdio is complete, so it is a gap in the source
tree that was read rather than a filter rejecting a category. The harvester walks
subdirectories and prints a note for paths it cannot find - which nobody was watching.

So the zero is void, and so is the case against building the pattern. Item one is blocked
on re-harvesting from a complete tree; the command already exists, only the input is
missing.

Third time in two days that validating the instrument mattered more than reading the code -
after the entry convention and the stub policy that was wired to nothing. Always the same
shape: something that looks present from every angle except the one that matters. A harvest
reporting "2,637 symbols" looks like success until you ask it for a symbol you can name
yourself.

