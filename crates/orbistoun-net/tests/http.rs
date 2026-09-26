//! `libSceHttp` driven the way oops-sdk's client (`src/net/http.c`) drives it on hardware, against
//! a server on the loopback interface.
//!
//! Guest memory is host memory, so the strings, out-parameters and buffers are this test's own.

use std::ffi::CString;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;

use orbistoun_core::{GUEST_ARG_REGISTERS, GuestFn};

fn implementation(name: &str) -> GuestFn {
    orbistoun_net::implementations()
        .iter()
        .find(|(n, _)| *n == name)
        .map_or_else(|| panic!("{name} is not implemented"), |(_, f)| *f)
}

fn call(name: &str, args: &[u64]) -> u64 {
    let mut regs = [0xDEAD_BEEF_DEAD_BEEF_u64; GUEST_ARG_REGISTERS];
    regs[..args.len()].copy_from_slice(args);
    implementation(name)(&regs)
}

/// A guest `int` answer: negative is failure.
fn int(answer: u64) -> i32 {
    i32::from_ne_bytes((answer as u32).to_ne_bytes())
}

/// Where the library may write a `T`: the address of this test's own variable.
fn at<T>(value: &mut T) -> u64 {
    std::ptr::from_mut(value).expose_provenance() as u64
}

/// A guest string: the address of a NUL-terminated copy.
fn text(value: &CString) -> u64 {
    value.as_ptr().expose_provenance() as u64
}

/// A server that answers each connection with the response `answer` builds from the request
/// line, and hands back every request's head.
fn serve(
    connections: usize,
    answer: fn(&str) -> Vec<u8>,
) -> (u16, std::sync::mpsc::Receiver<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let (heads, seen) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        for stream in listener.incoming().take(connections) {
            let mut stream = stream.unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut head = String::new();
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                if line == "\r\n" || line.is_empty() {
                    break;
                }
                head.push_str(&line);
            }
            let request_line = head.lines().next().unwrap_or_default().to_owned();
            stream.write_all(&answer(&request_line)).unwrap();
            heads.send(head).unwrap();
        }
    });
    (port, seen)
}

/// The client's objects, created in oops-sdk's order.
struct Session {
    pool: u64,
    ssl: u64,
    context: u64,
    template: u64,
}

impl Session {
    fn open(auto_redirect: bool) -> Self {
        let name = CString::new("oops_http").unwrap();
        let pool = call("sceNetPoolCreate", &[text(&name), 32 * 1024, 0]);
        let ssl = call("sceSslInit", &[320 * 1024]);
        let context = call("sceHttpInit", &[pool, ssl, 256 * 1024]);
        assert!(int(pool) > 0 && int(ssl) > 0 && int(context) > 0);
        assert_eq!(call("sceHttpsDisableOption", &[context, 0x3d]), 0);
        let agent = CString::new("OOPSy-daisy/1.0 (Prospero)").unwrap();
        let template = call(
            "sceHttpCreateTemplate",
            &[context, text(&agent), 1, u64::from(auto_redirect)],
        );
        assert!(int(template) > 0);
        Self {
            pool,
            ssl,
            context,
            template,
        }
    }

    /// A sent request for `url`: (connection, request).
    fn get(&self, url: &str, auto_redirect: Option<bool>) -> (u64, u64) {
        let url = CString::new(url).unwrap();
        let method = CString::new("GET").unwrap();
        let connection = call(
            "sceHttpCreateConnectionWithURL",
            &[self.template, text(&url), 1],
        );
        assert!(int(connection) > 0);
        let request = call(
            "sceHttpCreateRequestWithURL2",
            &[connection, text(&method), text(&url), 0],
        );
        assert!(int(request) > 0);
        if let Some(on) = auto_redirect {
            assert_eq!(call("sceHttpSetAutoRedirect", &[request, u64::from(on)]), 0);
        }
        for setter in [
            "sceHttpSetResolveTimeOut",
            "sceHttpSetConnectTimeOut",
            "sceHttpSetSendTimeOut",
        ] {
            assert_eq!(call(setter, &[request, 10_000_000]), 0);
        }
        assert_eq!(call("sceHttpSetRecvTimeOut", &[request, 20_000_000]), 0);
        let (name, value) = (
            CString::new("Accept").unwrap(),
            CString::new("*/*").unwrap(),
        );
        assert_eq!(
            call(
                "sceHttpAddRequestHeader",
                &[request, text(&name), text(&value), 0]
            ),
            0
        );
        assert_eq!(int(call("sceHttpSendRequest", &[request, 0, 0])), 0);
        (connection, request)
    }

    fn close(self) {
        assert_eq!(call("sceHttpDeleteTemplate", &[self.template]), 0);
        assert_eq!(call("sceHttpTerm", &[self.context]), 0);
        assert_eq!(call("sceSslTerm", &[self.ssl]), 0);
        assert_eq!(call("sceNetPoolDestroy", &[self.pool]), 0);
    }
}

fn status(request: u64) -> i32 {
    let mut code = 0_i32;
    assert_eq!(call("sceHttpGetStatusCode", &[request, at(&mut code)]), 0);
    code
}

fn body(request: u64) -> Vec<u8> {
    let mut all = Vec::new();
    let mut chunk = [0_u8; 7];
    loop {
        let n = int(call(
            "sceHttpReadData",
            &[request, at(&mut chunk), chunk.len() as u64],
        ));
        assert!(n >= 0, "read failed: {n:#x}");
        if n == 0 {
            return all;
        }
        all.extend_from_slice(&chunk[..n as usize]);
    }
}

fn content_length(request: u64) -> (i32, u64) {
    let (mut result, mut length) = (-1_i32, u64::MAX);
    assert_eq!(
        call(
            "sceHttpGetResponseContentLength",
            &[request, at(&mut result), at(&mut length)]
        ),
        0
    );
    (result, length)
}

fn delete(connection: u64, request: u64) {
    assert_eq!(call("sceHttpDeleteRequest", &[request]), 0);
    assert_eq!(call("sceHttpDeleteConnection", &[connection]), 0);
}

/// A plain fetch: status, a present content length, the body in pieces, and the headers the
/// template and the request asked for on the wire.
#[test]
fn a_get_answers_its_status_length_and_body() {
    let (port, heads) = serve(1, |_| {
        b"HTTP/1.1 200 OK\r\nContent-Length: 26\r\nConnection: close\r\n\r\n{\"assets\":[\"craft.zip\"]}\r\n"
            .to_vec()
    });
    let session = Session::open(true);
    let (connection, request) = session.get(&format!("http://127.0.0.1:{port}/releases"), None);
    assert_eq!(status(request), 200);
    assert_eq!(content_length(request), (0, 26));
    assert_eq!(body(request), b"{\"assets\":[\"craft.zip\"]}\r\n");
    let head = heads.recv().unwrap().to_ascii_lowercase();
    assert!(head.starts_with("get /releases http/1.1"), "{head}");
    assert!(
        head.contains("user-agent: oopsy-daisy/1.0 (prospero)"),
        "{head}"
    );
    assert!(head.contains("accept: */*"), "{head}");
    delete(connection, request);
    session.close();
}

/// With following off, a redirect is answered as itself, its `Location` in the header block -
/// oops-sdk follows by hand, one connection per host.
#[test]
fn a_redirect_not_followed_hands_its_location_to_the_guest() {
    let (port, _heads) = serve(1, |_| {
        b"HTTP/1.1 302 Found\r\nLocation: https://cdn.example/asset?sig=1\r\nContent-Length: 0\r\n\r\n"
            .to_vec()
    });
    let session = Session::open(true);
    let (connection, request) =
        session.get(&format!("http://127.0.0.1:{port}/download"), Some(false));
    assert_eq!(status(request), 302);
    let (mut block, mut size) = (0_u64, 0_u64);
    assert_eq!(
        call(
            "sceHttpGetAllResponseHeaders",
            &[request, at(&mut block), at(&mut size)]
        ),
        0
    );
    // SAFETY: the library's header block, `size` bytes, live until the request is deleted.
    let headers = unsafe {
        std::slice::from_raw_parts(
            std::ptr::with_exposed_provenance::<u8>(block as usize),
            size as usize,
        )
    };
    let headers = String::from_utf8_lossy(headers).to_ascii_lowercase();
    assert!(
        headers.contains("\r\nlocation: https://cdn.example/asset?sig=1\r\n"),
        "{headers}"
    );
    delete(connection, request);
    session.close();
}

/// With following on, the guest sees only the final answer.
#[test]
fn a_redirect_followed_answers_the_destination() {
    let (port, _heads) = serve(2, |line| {
        if line.contains("/moved") {
            b"HTTP/1.1 301 Moved\r\nLocation: /here\r\nContent-Length: 0\r\n\r\n".to_vec()
        } else {
            b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\n\r\nhere".to_vec()
        }
    });
    let session = Session::open(true);
    let (connection, request) = session.get(&format!("http://127.0.0.1:{port}/moved"), None);
    assert_eq!(status(request), 200);
    assert_eq!(body(request), b"here");
    delete(connection, request);
    session.close();
}

/// A chunked body has no length to give, and arrives decoded.
#[test]
fn a_chunked_body_has_no_length_and_arrives_decoded() {
    let (port, _heads) = serve(1, |_| {
        b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nhello\r\n6\r\n world\r\n0\r\n\r\n"
            .to_vec()
    });
    let session = Session::open(true);
    let (connection, request) = session.get(&format!("http://127.0.0.1:{port}/"), None);
    let (result, _) = content_length(request);
    assert_ne!(result, 0, "no length is present");
    assert_eq!(body(request), b"hello world");
    delete(connection, request);
    session.close();
}

/// A host that is not listening fails the send with a negative code, not a hang or a success.
#[test]
fn an_unreachable_host_fails_the_send() {
    let port = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let session = Session::open(true);
    let url = CString::new(format!("http://127.0.0.1:{port}/")).unwrap();
    let method = CString::new("GET").unwrap();
    let connection = call(
        "sceHttpCreateConnectionWithURL",
        &[session.template, text(&url), 1],
    );
    let request = call(
        "sceHttpCreateRequestWithURL2",
        &[connection, text(&method), text(&url), 0],
    );
    assert!(int(call("sceHttpSendRequest", &[request, 0, 0])) < 0);
    delete(connection, request);
    session.close();
}

/// An id that names nothing is refused.
#[test]
fn a_dead_id_is_refused() {
    assert!(int(call("sceHttpDeleteRequest", &[0x7fff_0000])) < 0);
    assert!(int(call("sceHttpTerm", &[0x7fff_0001])) < 0);
    assert!(int(call("sceSslTerm", &[0x7fff_0002])) < 0);
    assert!(int(call("sceNetPoolDestroy", &[0x7fff_0003])) < 0);
}
