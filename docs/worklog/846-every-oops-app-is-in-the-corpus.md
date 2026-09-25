# 846. Every oops-apps app is in the corpus

**2026-09-24**. `corpus/sources.toml` now lists oops-apps' titles, utilities, GL and Mesa test titles
and payloads. Titles are named by the title id in their `app.env`; payloads go under `payloads/`.

- **Origins:** each entry lists the sibling checkout's `dist/` package first, then the suite's
  `latest-main` release asset.
- **Left out:** the empty `oops-app-downloader`, and the host tools `gl-host` and `gl-replay`.
- **Tar packages:** some packages are a **GNU tar under a `.zip` name**. That is an oops-apps
  packaging fault: `common/app.mk` falls back to `tar -a -cf` when `zip` is absent, and only bsdtar
  honours the suffix. It is filed as oops-apps `REQ-20260924T1850Z-7e21`. Meanwhile `unpack_title`
  tells the formats apart by their bytes (`PK` or `ustar`), with the same enclosed-path refusal for
  both.
- **Single-file sources:** a source that lists `sources` and is not an `archive` (a payload) is now
  fetched and written as `<root>/<name>/<file>`. Before, such an entry fetched nothing: the
  obSCEne payload entry included.
- **Rolling pins:** a `local` source refreshes its pin whichever origin answered. Its release
  fallback is the rolling `latest-main`, so a changed hash there is a new build, not a moved tag.
- **obSCEne native entries:** both are now marked `archive`, so the Prospero probe unpacks as a
  title folder instead of landing as a zip file.
- **A failing source no longer stops the sync:** `corpus sync` reports it, carries on with the rest,
  and exits non-zero at the end naming every source with no answering origin.

**Result:** 16 of oops-apps' titles, utilities and test titles, and 3 of its payloads, are in the
library. Not built or released yet: armagetron-advanced, craft, retroarch, supertux, supertuxkart,
cxx-throw and porthole. mesa-demos synced from the release earlier and is now 404 there after a
rebuild; the copy in the library stays.
