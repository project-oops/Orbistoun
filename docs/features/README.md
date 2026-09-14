# Orbistoun Features & Screens

Feature and screen documentation for Orbistoun. Each page covers both the desktop GUI window (`orbistoun-gui`) and the command-line interface (`orbistoun`) side by side.

| Feature / Screen | GUI View | CLI Counterpart | Documentation Page |
| :--- | :--- | :--- | :--- |
| **Library** | Game Library & Title Dashboard | `orbistoun inspect`, `orbistoun status` | [library.md](library.md) |
| **Running** | Execution Runner & Verification | `orbistoun run`, `orbistoun report`, `orbistoun verify` | [running.md](running.md) |
| **Inspector** | Live Call Trace & HLE Inspector | `OOPS_LOG=trace orbistoun run` | [inspector.md](inspector.md) |
| **Memory** | Virtual Memory & Register State | `orbistoun debug` | [memory.md](memory.md) |
| **Graphics** | Vulkan 1.3 & RDNA2 Display Settings | `--renderer vulkan`, `--resolution` | [graphics.md](graphics.md) |
| **Controllers** | Gamepad & Input Mapping | `--pad dualsense`, `--pad xinput` | [controllers.md](controllers.md) |
| **Names & Hashes** | NID Import Resolution | `orbistoun symbols`, `orbistoun names` | [naming.md](naming.md) |
| **Where it Writes** | Paths & Portable Mode | `orbistoun paths` | [paths.md](paths.md) |
| **User Guide** | Player & Tester Guide | `orbistoun --help` | [user-guide.md](user-guide.md) |

For higher-level instructions, see the [User Guide](user-guide.md).

