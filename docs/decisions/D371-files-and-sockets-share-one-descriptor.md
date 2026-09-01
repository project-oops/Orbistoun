# D371 - Files and sockets share one descriptor table, because a guest has one


**decided** - 2026-08-29

Sockets are the milestone rather than another subsystem, and the reason is arithmetic:
`pros check` - the independent tool this project wants as its grader - does exactly one thing
per service, `TcpStream::connect_timeout(...).is_ok()`. No handshake, no protocol. **A service
reads as up the moment the guest has a listening socket on its port**, so for `klogsrv` that
is reaching its `listen` call and nothing more.

And the deeper commands need no protocol work either. `ftpsrv` implements FTP; `klogsrv`
writes the log. orbistoun owes them sockets and file calls, and the guest brings its own
protocol - which is the property that makes the grader worth having, because every byte
Prosperous sees was produced by guest code executing.

### One table

A guest closes a socket with `close`, reads one with `read`, writes one with `write`. Two
tables would mean two numbering spaces and a descriptor that means different things depending
on which call is holding it - so `Target` grew a `Socket` variant beside its `File` one, and
those three calls take either without being told which.

That also makes a server that reads a request with `read` and one that reads it with `recv`
the same program to everything below the dispatch layer, which is what a guest expects and
what a second table would have made accidentally different.

### A socket exists before it has anything to do

`socket()` answers a descriptor that is not yet a host object: the host makes a listener by
binding *and* listening in one step, and a stream by connecting. So a descriptor starts
pending, remembers what `bind` was told, and becomes a host object at `listen` or `connect`.

Bookkeeping rather than a claim. The guest sees the sequence it wrote; the host sees the
sequence it accepts. Binding at `bind` and rebuilding at `listen` would hold the port twice.

### The byte that catches people

```text
struct sockaddr_in {                     sys/netinet/in.h
    uint8_t     sin_len;      offset 0
    sa_family_t sin_family;   offset 1
    in_port_t   sin_port;     offset 2   network byte order
    struct in_addr sin_addr;  offset 4
    char        sin_zero[8];  offset 8
};
```

`sin_len` is not on most platforms. It is on this one, and a shim written from memory of Linux
reads offset zero and gets a length where it wanted a family. It is in the checkout, so it was
read rather than recalled - and there is a test that asserts the family is taken from offset
one.

### `setsockopt` is accepted and applied to nothing

A server's first act after `socket` is `setsockopt(SO_REUSEADDR)`, and **failing it stops the
server**: a correct program checks, reports and exits. Refusing outright would end every
payload measured before it reached `bind`.

Applying it is a different question. `SO_REUSEADDR` is what the host's listener does by
default on the platforms this runs on, so honouring it changes nothing; the rest - timeouts,
buffer sizes, keepalive - would need a per-option mapping nothing here can verify, and a wrong
one is a socket behaving differently from what the guest asked for with nothing saying so.

Accepted, recorded as not applied, and the knowledge file says which. That is the honest shape
of *the call succeeded and the option did nothing*.

### Where they are declared

Eight of the eleven were measured being imported from `libScePosix` by a title. `accept`,
`listen` and `getpeername` were not - a title connects rather than listens - so they are
declared there **by inference from their siblings**, and each says so in its own knowledge
entry rather than being passed off as measured (D367).

