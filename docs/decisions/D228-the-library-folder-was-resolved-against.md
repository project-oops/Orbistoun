# D228 - The library folder was resolved against the launch directory, which is not a setting


`library.root` defaults to `titles`, and that relative path was handed straight to
`read_dir`. So it resolved against the **process working directory** - which is not a
property of the installation, the settings file, or anything a person chose. It is a
property of how the program happened to be started.

| Started by | Working directory | Library scanned |
|---|---|---|
| `cargo run` from the repository | the repository | `titles/` - the real one |
| double-clicking `target/debug/orbistoun-gui.exe` | `target/debug` | `target/debug/titles` - absent |
| a debugger | whatever its launch configuration says | anyone's guess |

Same binary, same `config.toml`, three different answers, and the window reported the
difference as *no titles here* - which reads as a broken scanner rather than as a path
that meant something different this time.

### The reasoning already existed one crate away

`orbistoun-paths` exists precisely because "where does this program keep things" must not
depend on the working directory (D016, D038). That argument was made for everything
orbistoun **writes** and never applied to the one thing it **reads**.

So `LibrarySettings::resolve` takes the data root as its base:

- **portable** - `<binary>/.portable/titles`, so dropping titles beside the executable
  works with no setup at all, which is the entire point of a portable build
- **installed** - `%APPDATA%/orbistoun/titles`
- **`ORBISTOUN_DATA_DIR` set** - underneath that
- **absolute root** - used exactly as given, which is the ordinary case once somebody has
  pointed the window at their own folder

### The rejected fix, and why

A `.env` beside the debug binary was the first suggestion, and it is the wrong shape
twice over. It fixes the developer's launch and leaves the defect in the shipped
binary - the bug is not "debug builds are special", it is "a relative path means
whatever the launcher decided". And it would be a *file* carrying environment values,
which `orbistoun-env` already has a rule about: settings may come from a file and
diagnostics may not (D221). Introducing that machinery to work around a path resolution
bug spends the rule on the wrong problem.

### Naming the folder is half the fix

`io::Error` does not carry a path, so a missing library reported *the system cannot find
the path specified* and never said which path it meant. It says so now, and the
preferences pane prints the resolved folder under the text box with `(not a folder)`
beside it when there is nothing there.

That is principle 3 in the ordinary case: the window knew exactly where it had looked and
declined to mention it, which is the difference between a setting and a guess.

