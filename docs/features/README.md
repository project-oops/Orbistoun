# Orbistoun Features & Screens

Feature and screen documentation for Orbistoun. Each page covers both the desktop GUI window (`orbistoun-gui`) and the command-line interface (`orbistoun`) side by side.

The binary is `orbistoun-cli` (there is no bare `orbistoun` executable); `orbistoun-gui` is
the separate desktop shell.

| Feature / Screen | GUI View | CLI Counterpart | Documentation Page |
| :--- | :--- | :--- | :--- |
| **Library** | Game Library & Title Dashboard | `orbistoun-cli inspect`, `orbistoun-cli status` | [library.md](library.md) |
| **Running** | Execution Runner & Verification | `orbistoun-cli run`, `orbistoun-cli report`, `orbistoun-cli verify` | [running.md](running.md) |
| **Inspector** | Live Call Trace & HLE Inspector | `OOPS_LOG=trace orbistoun-cli run` | [inspector.md](inspector.md) |
| **Memory** | Virtual Memory & Register State | `orbistoun-cli load`, `OOPS_LOG=orbistoun_mem=debug orbistoun-cli run` | [memory.md](memory.md) |
| **Graphics** | Vulkan device info (presentation is a stub - no settings yet) | none yet - see [graphics.md](graphics.md) | [graphics.md](graphics.md) |
| **Controllers** | Gamepad & Keyboard Mapping (HLE, no host gamepad library) | none - mapping lives in the settings file | [controllers.md](controllers.md) |
| **Names & Hashes** | NID Import Resolution | `orbistoun-cli symbols`, `orbistoun-cli names` | [naming.md](naming.md) |
| **Where it Writes** | Paths & Portable Mode | `orbistoun-cli paths` | [paths.md](paths.md) |
| **User Guide** | Player & Tester Guide | `orbistoun-cli --help` | [user-guide.md](user-guide.md) |

For higher-level instructions, see the [User Guide](user-guide.md).

