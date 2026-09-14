# Inspector

Live call trace inspection, function call ranking, and HLE resolution monitoring.

Orbistoun's Execution Inspector allows testers and developers to observe every guest call into the operating system, showing parameters, return codes, and whether an answer was grounded in silicon measurements (`known_by: measured`) or unverified approximations (`known_by: assumed`).

---

## GUI: Execution Inspector

Open the **Inspector** tab from the main emulation window (or press `Ctrl+I`).

```text
+-------------------------------------------------------------------------------+
|  Execution Inspector - GLCB00001                                 [_][O][X]    |
+-------------------------------------------------------------------------------+
| Step | Function / Symbol Name        | Known By | Return Code | Latency       |
|------+-------------------------------+----------+-------------+---------------|
| 0001 | sceKernelVirtualQueryInfo     | measured | 0x00000000  | 0.04 ms       |
| 0002 | sceKernelAllocateDirectMemory | measured | 0x00000000  | 0.12 ms       |
| 0003 | sceKernelMapDirectMemory      | measured | 0x00000000  | 0.08 ms       |
| 0004 | sceAgcDriverCreateQueue       | measured | 0x00000000  | 0.45 ms       |
| 0005 | sceAgcSubmitDcb               | measured | 0x00000000  | 0.22 ms       |
+-------------------------------------------------------------------------------+
| [Pause Execution]   [Step Into]   [Compare against Golden Run]                |
+-------------------------------------------------------------------------------+
```

![Orbistoun Execution Inspector](screenshots/inspector.png)
*(Screenshot placeholder: Execution Inspector)*

### GUI Controls:
- **Call List**: Chronological execution history of all platform API calls made by the guest.
- **`Known By` Column**:
  - `measured`: Implementation verified on physical PS5 hardware via obSCEne.
  - `published`: Documented standard POSIX / FreeBSD behavior.
  - `guest-observed`: Inferred from guest code behavior.
  - `assumed`: Unverified placeholder (subject to hardware verification in THE LOOP).
- **Pause & Step**: Halt execution at the next syscall to inspect register arguments.
- **Compare against Golden Run**: Highlight deviations against previously recorded execution traces.

---

## CLI: Trace Logging

To capture per-call trace output in the terminal:

```bash
OOPS_LOG=trace orbistoun run build/title/GLCB00001
```

