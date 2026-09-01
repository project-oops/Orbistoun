# D037 - Two user-facing binaries, plus a portable GUI artifact

**decided** · 2026-08-19

`orbistoun-gui` and `orbistoun-cli`. The release workflow additionally publishes a
renamed `orbistoun-portable-gui` so the portable download is the default one, which
picks up D038's filename trigger for free. Requires renaming the current bin target
from `orbistoun` to `orbistoun-cli`.

