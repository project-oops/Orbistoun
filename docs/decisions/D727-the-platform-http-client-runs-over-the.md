# D727 - The platform HTTP client runs over the host network

**Status:** assumed
**Date:** 2026-09-26

`libSceHttp`, with the `libSceSsl` contexts and `libSceNet` pools it is handed, is carried
out over the host's network: a guest's request becomes a host HTTP transfer (`reqwest`,
blocking, over `rustls`), and its status, headers and body come back through the calls the
guest reads them with. `sceNetCtlInit` and `sceNetCtlTerm` answer as the library being
ready. The contract implemented is the one oops-sdk's HTTP client relies on while it fetches
through these libraries on hardware; what that client does not exercise is recorded as
assumed in the knowledge files.

**Why:** the HTTP client is a transport, the same layer as the BSD sockets that already map
onto host sockets, not one of the vendor's online services. OOPSy-daisy cannot show its
catalogue without it, and a title that fetches its own content over HTTP gets the content
rather than a refusal it cannot recover from.

**Rejected:**
- Refusing every transfer, as before: the guest's own content is unreachable, and the run
  measures nothing about the title past its first fetch.
- Serving recorded responses: a fixture answers what a past run was told, not what the
  guest asked for now.
- A TLS implementation inside the guest boundary: the certificate checks the guest turns off
  are the host verifier's switches, and the host already carries TLS for other crates.

**Scope:** the vendor's online services (`libSceNp*`) stay out of scope (`docs/SCOPE.md`).
