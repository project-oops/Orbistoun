# The library folder is a setting now, and refresh is a button


Two small things that turned out to be one thing.

The library was already scanned at startup - but the default folder is a *relative* path,
so it resolved only when the program happened to be started from the repository. Launched
any other way it found nothing, which reads as a broken scanner rather than a setting
pointing at the wrong place. So the folder and the run limit moved into the persisted
settings (`[library]`), and are written back when saved. Loaded on load, from wherever it
was last pointed.

Refresh is now a toolbar button rather than only a menu item. The library is scanned once
at startup, so noticing a title that appeared since is a thing somebody wants to do without
hunting through a menu. It is applied *after* the strip is drawn, because rescanning
replaces the list the buttons were drawn from.

The toolbar's right-hand side now also says how many titles the last scan found, so an
empty library is visibly a scan that ran and found nothing rather than one that never
happened.

Verified: 30 service tests, no lints across gui and service, CLI unchanged (46 imports, 799
conforming calls).

