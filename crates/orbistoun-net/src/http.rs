//! `libSceHttp` - the first-generation HTTP client, carried out over the host's network.
//!
//! A guest builds a chain of objects - context, template, connection, request - and sends the
//! request; the transfer itself is the host's (`reqwest`, blocking, over `rustls`), the way the
//! socket calls map onto host sockets (D727). Every id is drawn from one counter, so an id names
//! exactly one object whatever kind it is, and the settings calls (`sceHttpSetAutoRedirect`, the
//! timeouts, `sceHttpAddRequestHeader`, `sceHttpsDisableOption`) take any of them. A child copies
//! its parent's settings when it is created.
//!
//! The contract is what oops-sdk's HTTP client (`src/net/http.c`), which fetches over this library
//! on hardware, relies on: positive ids, zero for success and a negative code for failure, a
//! status code, a header block holding the response's `Location`, a content-length answer whose
//! result word is zero when the length is present, and `sceHttpReadData` counting bytes down to
//! zero at the end of the body. What that code does not exercise - the other content-length
//! result words, the exact layout of the header block, the platform's own error codes - is
//! written down as assumed in `libSceHttp.toml`.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::io::Read;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use orbistoun_core::{GUEST_ARG_REGISTERS, GuestError, GuestFn};
use orbistoun_hle::guest_module;
use orbistoun_mem::guest;

guest_module! {
    "libSceHttp" {
        // (id, name, value, mode)
        "sceHttpAddRequestHeader" => 4,
        // (template, url, keep-alive)
        "sceHttpCreateConnectionWithURL" => 3,
        // (connection, method, url, content length)
        "sceHttpCreateRequestWithURL2" => 4,
        // (context, user agent, HTTP version, auto-redirect)
        "sceHttpCreateTemplate" => 4,
        "sceHttpDeleteConnection" => 1,
        "sceHttpDeleteRequest" => 1,
        "sceHttpDeleteTemplate" => 1,
        // (request, char **header, size_t *size)
        "sceHttpGetAllResponseHeaders" => 3,
        // (request, int *result, uint64_t *length)
        "sceHttpGetResponseContentLength" => 3,
        // (request, int *status)
        "sceHttpGetStatusCode" => 2,
        // (net pool, SSL context, pool size)
        "sceHttpInit" => 3,
        // (request, buffer, size)
        "sceHttpReadData" => 3,
        // (request, post data, size)
        "sceHttpSendRequest" => 3,
        "sceHttpSetAutoRedirect" => 2,
        "sceHttpSetConnectTimeOut" => 2,
        "sceHttpSetRecvTimeOut" => 2,
        "sceHttpSetResolveTimeOut" => 2,
        "sceHttpSetSendTimeOut" => 2,
        "sceHttpTerm" => 1,
        "sceHttpUriParse" => 6,
        // (id, flags)
        "sceHttpsDisableOption" => 2,
    }
}

/// Implementations this module provides, by symbol name.
#[must_use]
pub fn implementations() -> &'static [(&'static str, GuestFn)] {
    &[
        ("sceHttpInit", init),
        ("sceHttpTerm", term),
        ("sceHttpCreateTemplate", create_template),
        ("sceHttpDeleteTemplate", delete_object),
        ("sceHttpCreateConnectionWithURL", create_connection),
        ("sceHttpDeleteConnection", delete_object),
        ("sceHttpCreateRequestWithURL2", create_request),
        ("sceHttpDeleteRequest", delete_request),
        ("sceHttpAddRequestHeader", add_request_header),
        ("sceHttpSetAutoRedirect", set_auto_redirect),
        ("sceHttpSetResolveTimeOut", set_resolve_timeout),
        ("sceHttpSetConnectTimeOut", set_connect_timeout),
        ("sceHttpSetSendTimeOut", set_send_timeout),
        ("sceHttpSetRecvTimeOut", set_recv_timeout),
        ("sceHttpsDisableOption", https_disable_option),
        ("sceHttpSendRequest", send_request),
        ("sceHttpGetStatusCode", get_status_code),
        ("sceHttpGetAllResponseHeaders", get_all_response_headers),
        (
            "sceHttpGetResponseContentLength",
            get_response_content_length,
        ),
        ("sceHttpReadData", read_data),
    ]
}

/// `sceHttpsDisableOption` bits, as oops-sdk names them for the hardware it runs on: the
/// certificate-chain checks, which the host verifier folds into one switch...
const SSL_CHAIN_CHECKS: u32 = 0x01 | 0x08 | 0x10 | 0x20;
/// ...and the host-name check, which it keeps separate.
const SSL_CN_CHECK: u32 = 0x04;

/// `sceHttpAddRequestHeader` mode: replace a header of the same name.
const HEADER_OVERWRITE: u64 = 0;
/// ...or add another beside it.
const HEADER_ADD: u64 = 1;

/// `sceHttpGetResponseContentLength` result words: the length is present...
const CONTENT_LENGTH_EXISTS: u32 = 0;
/// ...the response carries none...
const CONTENT_LENGTH_NOT_FOUND: u32 = 1;
/// ...or the body is chunked.
const CONTENT_LENGTH_CHUNKED: u32 = 2;

/// What `sceHttpDeleteRequest` and `sceHttpTerm` answer for an id never issued, on hardware
/// (obSCEne `102-net/http-lifecycle`, sweep 20260927-093727).
const ERROR_UNISSUED_ID: u32 = 0x8043_1100;
/// What `sceHttpAddRequestHeader` answers for mode 2 on hardware (the same sweep).
const ERROR_HEADER_MODE: u32 = 0x8043_11fe;
/// `ECONNREFUSED`, FreeBSD `sys/sys/errno.h`: `sceHttpSendRequest` to a refused port answered
/// libSceNet's `0x8041_013d` on hardware (the same sweep).
const ECONNREFUSED: u32 = 61;

/// The longest string read from the guest: a signed CDN URL runs to a few kilobytes.
const LONGEST_STRING: usize = 16 * 1024;

/// What a child inherits from its parent at creation, and what the settings calls change.
#[derive(Clone, Debug, Default)]
struct Settings {
    auto_redirect: bool,
    /// Microseconds; zero is the host's default.
    resolve_timeout: u32,
    connect_timeout: u32,
    send_timeout: u32,
    recv_timeout: u32,
    /// `sceHttpsDisableOption` bits in force.
    ssl_disabled: u32,
    headers: Vec<(String, String)>,
}

/// A received response, held until the request is deleted.
struct Response {
    status: u16,
    /// The header block `sceHttpGetAllResponseHeaders` hands out, NUL-terminated. Its address is
    /// the guest's until the request is deleted, so it is never reallocated.
    header_block: Vec<u8>,
    content_length: Option<u64>,
    chunked: bool,
    body: Box<dyn Read + Send>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Context,
    Template,
    Connection,
    Request,
}

struct Object {
    kind: Kind,
    settings: Settings,
    /// The connection's URL, or the request's.
    url: Option<reqwest::Url>,
    method: String,
    /// Shared so a transfer runs without the table's lock held.
    response: Option<Arc<Mutex<Response>>>,
}

/// Every live object, by id.
fn table() -> MutexGuard<'static, HashMap<u32, Object>> {
    static TABLE: std::sync::OnceLock<Mutex<HashMap<u32, Object>>> = std::sync::OnceLock::new();
    TABLE
        .get_or_init(Mutex::default)
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
}

/// The next id: positive as the guest's `int`, never reused in a run.
fn next_id() -> u32 {
    static NEXT: AtomicU32 = AtomicU32::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

fn ok() -> u64 {
    0
}

fn fail(error: GuestError) -> u64 {
    u64::from(error.as_raw())
}

/// An id register as the `int` the guest passed, in the unsigned form ids are kept in.
fn id_of(register: u64) -> u32 {
    // The low half of the register is the `int`; the guest leaves the high half unspecified.
    register as u32
}

fn string_at(at: u64) -> Option<String> {
    if at == 0 {
        return None;
    }
    // SAFETY: an address the guest passed as a string for this call, valid by its contract.
    let bytes = unsafe { guest::read_cstr(at, LONGEST_STRING) }?;
    String::from_utf8(bytes).ok()
}

/// Creates the object `make` builds from a copy of `parent`'s settings, answering its id.
fn create_child(parent: u32, parent_kind: Kind, make: impl FnOnce(Settings) -> Object) -> u64 {
    let mut objects = table();
    let Some(settings) = objects
        .get(&parent)
        .filter(|p| p.kind == parent_kind)
        .map(|p| p.settings.clone())
    else {
        return fail(GuestError::InvalidHandle);
    };
    let id = next_id();
    objects.insert(id, make(settings));
    answer_id(id)
}

/// An id as the guest's `int` return value.
fn answer_id(id: u32) -> u64 {
    u64::from(id)
}

/// `sceHttpInit(netPoolId, sslCtxId, poolSize)`: a context. The pools are the platform's memory
/// accounting; the host transfer allocates its own.
fn init(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let id = next_id();
    table().insert(
        id,
        Object {
            kind: Kind::Context,
            // Following redirects is the library's default; a template states its own.
            settings: Settings {
                auto_redirect: true,
                ..Settings::default()
            },
            url: None,
            method: String::new(),
            response: None,
        },
    );
    answer_id(id)
}

/// `sceHttpTerm(context)`.
fn term(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    delete_kind(
        id_of(args[0]),
        Kind::Context,
        GuestError::Raw(ERROR_UNISSUED_ID),
    )
}

/// `sceHttpDeleteRequest(request)`.
fn delete_request(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    delete_kind(
        id_of(args[0]),
        Kind::Request,
        GuestError::Raw(ERROR_UNISSUED_ID),
    )
}

/// `sceHttpDelete{Template,Connection}(id)`. What either answers for a dead id is unmeasured.
fn delete_object(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    if table().remove(&id_of(args[0])).is_some() {
        ok()
    } else {
        fail(GuestError::InvalidHandle)
    }
}

/// Deletes `id` if it names a live object of `kind`, answering `refusal` otherwise.
fn delete_kind(id: u32, kind: Kind, refusal: GuestError) -> u64 {
    let mut objects = table();
    if objects.get(&id).is_some_and(|o| o.kind == kind) {
        objects.remove(&id);
        ok()
    } else {
        fail(refusal)
    }
}

/// `sceHttpCreateTemplate(context, userAgent, httpVersion, autoRedirect)`.
fn create_template(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let agent = string_at(args[1]);
    let auto_redirect = args[3] as u32 != 0;
    create_child(id_of(args[0]), Kind::Context, |mut settings| {
        settings.auto_redirect = auto_redirect;
        if let Some(agent) = agent {
            settings
                .headers
                .retain(|(name, _)| !name.eq_ignore_ascii_case("User-Agent"));
            settings.headers.push(("User-Agent".to_owned(), agent));
        }
        Object {
            kind: Kind::Template,
            settings,
            url: None,
            method: String::new(),
            response: None,
        }
    })
}

/// `sceHttpCreateConnectionWithURL(template, url, keepAlive)`.
fn create_connection(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let Some(url) = string_at(args[1]).and_then(|u| reqwest::Url::parse(&u).ok()) else {
        return fail(GuestError::InvalidArgument);
    };
    create_child(id_of(args[0]), Kind::Template, |settings| Object {
        kind: Kind::Connection,
        settings,
        url: Some(url),
        method: String::new(),
        response: None,
    })
}

/// `sceHttpCreateRequestWithURL2(connection, method, url, contentLength)`. The URL may be whole or
/// a path on the connection's host.
fn create_request(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let connection = id_of(args[0]);
    let (Some(method), Some(target)) = (string_at(args[1]), string_at(args[2])) else {
        return fail(GuestError::InvalidArgument);
    };
    let base = match table().get(&connection) {
        Some(c) if c.kind == Kind::Connection => c.url.clone(),
        _ => return fail(GuestError::InvalidHandle),
    };
    let url = reqwest::Url::parse(&target)
        .ok()
        .or_else(|| base.and_then(|b| b.join(&target).ok()));
    let Some(url) = url else {
        return fail(GuestError::InvalidArgument);
    };
    create_child(connection, Kind::Connection, |settings| Object {
        kind: Kind::Request,
        settings,
        url: Some(url),
        method,
        response: None,
    })
}

/// Changes the settings of whatever `id` names.
fn with_settings(id: u32, change: impl FnOnce(&mut Settings)) -> u64 {
    match table().get_mut(&id) {
        Some(object) => {
            change(&mut object.settings);
            ok()
        }
        None => fail(GuestError::InvalidHandle),
    }
}

/// `sceHttpAddRequestHeader(id, name, value, mode)`.
fn add_request_header(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (Some(name), Some(value)) = (string_at(args[1]), string_at(args[2])) else {
        return fail(GuestError::InvalidArgument);
    };
    let mode = args[3] & 0xFFFF_FFFF;
    if mode != HEADER_OVERWRITE && mode != HEADER_ADD {
        return fail(GuestError::Raw(ERROR_HEADER_MODE));
    }
    with_settings(id_of(args[0]), |settings| {
        if mode == HEADER_OVERWRITE {
            settings
                .headers
                .retain(|(held, _)| !held.eq_ignore_ascii_case(&name));
        }
        settings.headers.push((name, value));
    })
}

/// `sceHttpSetAutoRedirect(id, enable)`.
fn set_auto_redirect(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let enable = args[1] as u32 != 0;
    with_settings(id_of(args[0]), |s| s.auto_redirect = enable)
}

fn set_resolve_timeout(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let usec = args[1] as u32;
    with_settings(id_of(args[0]), |s| s.resolve_timeout = usec)
}

fn set_connect_timeout(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let usec = args[1] as u32;
    with_settings(id_of(args[0]), |s| s.connect_timeout = usec)
}

fn set_send_timeout(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let usec = args[1] as u32;
    with_settings(id_of(args[0]), |s| s.send_timeout = usec)
}

fn set_recv_timeout(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let usec = args[1] as u32;
    with_settings(id_of(args[0]), |s| s.recv_timeout = usec)
}

/// `sceHttpsDisableOption(id, flags)`: turns certificate checks off.
fn https_disable_option(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let flags = args[1] as u32;
    with_settings(id_of(args[0]), |s| s.ssl_disabled |= flags)
}

fn micros(usec: u32) -> Option<Duration> {
    (usec != 0).then(|| Duration::from_micros(u64::from(usec)))
}

/// The host client a request's settings describe.
fn client_for(settings: &Settings) -> reqwest::Result<reqwest::blocking::Client> {
    let redirects = if settings.auto_redirect {
        reqwest::redirect::Policy::default()
    } else {
        reqwest::redirect::Policy::none()
    };
    // Resolving and connecting are one phase to the host.
    let connect = match (
        micros(settings.resolve_timeout),
        micros(settings.connect_timeout),
    ) {
        (Some(a), Some(b)) => Some(a + b),
        (a, b) => a.or(b),
    };
    // The host's timeout bounds waiting for the response and then each read of the body: the
    // receive timeout, or the send timeout while nothing has been received.
    let waiting = micros(settings.recv_timeout).or(micros(settings.send_timeout));
    let mut builder = reqwest::blocking::Client::builder()
        .redirect(redirects)
        .timeout(waiting)
        .danger_accept_invalid_certs(settings.ssl_disabled & SSL_CHAIN_CHECKS != 0)
        .danger_accept_invalid_hostnames(settings.ssl_disabled & SSL_CN_CHECK != 0);
    if let Some(connect) = connect {
        builder = builder.connect_timeout(connect);
    }
    builder.build()
}

/// `sceHttpSendRequest(request, postData, size)`: carries out the transfer, up to the response's
/// headers; the body is read by `sceHttpReadData`.
fn send_request(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let id = id_of(args[0]);
    let (settings, url, method) = {
        let objects = table();
        match objects.get(&id) {
            Some(r) if r.kind == Kind::Request => {
                (r.settings.clone(), r.url.clone(), r.method.clone())
            }
            _ => return fail(GuestError::InvalidHandle),
        }
    };
    let (Some(url), Ok(method)) = (url, reqwest::Method::from_bytes(method.as_bytes())) else {
        return fail(GuestError::InvalidArgument);
    };
    let body = match (args[1], usize::try_from(args[2])) {
        (0, _) | (_, Ok(0)) => None,
        (at, Ok(size)) => {
            let Ok(at) = usize::try_from(at) else {
                return fail(GuestError::InvalidArgument);
            };
            // SAFETY: the guest's post data, `size` bytes at `at` by the call's contract.
            let data = unsafe {
                std::slice::from_raw_parts(std::ptr::with_exposed_provenance::<u8>(at), size)
            };
            Some(data.to_vec())
        }
        (_, Err(_)) => return fail(GuestError::InvalidArgument),
    };
    let client = match client_for(&settings) {
        Ok(client) => client,
        Err(error) => {
            tracing::warn!("sceHttpSendRequest: no host client for {url}: {error}");
            return fail(GuestError::HostFailed);
        }
    };
    let mut request = client.request(method, url.clone());
    for (name, value) in &settings.headers {
        request = request.header(name, value);
    }
    if let Some(body) = body {
        request = request.body(body);
    }
    let response = match request.send() {
        Ok(response) => response,
        Err(error) => {
            tracing::warn!("sceHttpSendRequest: {url}: {error}");
            return fail(send_failure(&error));
        }
    };
    let received = received(response);
    match table().get_mut(&id) {
        Some(object) => {
            object.response = Some(Arc::new(Mutex::new(received)));
            ok()
        }
        // Deleted while the transfer ran.
        None => fail(GuestError::InvalidHandle),
    }
}

/// What a failed send answers: libSceNet's own code where the host's failure is one hardware was
/// seen to answer, the host-failure placeholder otherwise.
fn send_failure(error: &reqwest::Error) -> GuestError {
    let mut source: Option<&(dyn std::error::Error + 'static)> = Some(error);
    while let Some(cause) = source {
        if let Some(io) = cause.downcast_ref::<std::io::Error>()
            && io.kind() == std::io::ErrorKind::ConnectionRefused
        {
            return GuestError::vendor_in(crate::NET_ERROR_BASE, ECONNREFUSED);
        }
        source = cause.source();
    }
    GuestError::HostFailed
}

/// The parts of a host response the guest can ask for.
fn received(response: reqwest::blocking::Response) -> Response {
    let status = response.status();
    let mut block = format!(
        "{:?} {} {}\r\n",
        response.version(),
        status.as_u16(),
        status.canonical_reason().unwrap_or("")
    );
    let mut chunked = false;
    let mut declared = None;
    for (name, value) in response.headers() {
        let value = String::from_utf8_lossy(value.as_bytes());
        if name == reqwest::header::TRANSFER_ENCODING
            && value.to_ascii_lowercase().contains("chunked")
        {
            chunked = true;
        }
        if name == reqwest::header::CONTENT_LENGTH {
            declared = value.trim().parse::<u64>().ok();
        }
        let _ = write!(block, "{}: {value}\r\n", name.as_str());
    }
    block.push_str("\r\n");
    let mut header_block = block.into_bytes();
    header_block.push(0);
    Response {
        status: status.as_u16(),
        header_block,
        content_length: declared,
        chunked,
        body: Box::new(response),
    }
}

/// The response `id`'s request holds, if it has been sent.
fn response_of(id: u32) -> Result<Arc<Mutex<Response>>, GuestError> {
    match table().get(&id) {
        Some(object) if object.kind == Kind::Request => {
            object.response.clone().ok_or(GuestError::InvalidArgument)
        }
        _ => Err(GuestError::InvalidHandle),
    }
}

fn locked(response: &Mutex<Response>) -> MutexGuard<'_, Response> {
    response.lock().unwrap_or_else(PoisonError::into_inner)
}

/// `sceHttpGetStatusCode(request, int *status)`.
fn get_status_code(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let response = match response_of(id_of(args[0])) {
        Ok(response) => response,
        Err(error) => return fail(error),
    };
    let status = locked(&response).status;
    // SAFETY: the guest's `int *`, valid by the call's contract.
    if unsafe { guest::write_u32(args[1], u32::from(status)) } {
        ok()
    } else {
        fail(GuestError::InvalidArgument)
    }
}

/// `sceHttpGetAllResponseHeaders(request, char **header, size_t *size)`: the header block stays the
/// library's, valid until the request is deleted.
fn get_all_response_headers(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let response = match response_of(id_of(args[0])) {
        Ok(response) => response,
        Err(error) => return fail(error),
    };
    let held = locked(&response);
    let at = held.header_block.as_ptr().expose_provenance() as u64;
    // The terminator is not counted.
    let size = held.header_block.len() as u64 - 1;
    // SAFETY: the guest's `char **`, valid by the call's contract.
    let pointer = unsafe { guest::write_u64(args[1], at) };
    // SAFETY: the guest's `size_t *`, likewise.
    let written = pointer && unsafe { guest::write_u64(args[2], size) };
    if written {
        ok()
    } else {
        fail(GuestError::InvalidArgument)
    }
}

/// `sceHttpGetResponseContentLength(request, int *result, uint64_t *length)`.
fn get_response_content_length(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let response = match response_of(id_of(args[0])) {
        Ok(response) => response,
        Err(error) => return fail(error),
    };
    let held = locked(&response);
    let (result, length) = match (held.content_length, held.chunked) {
        (_, true) => (CONTENT_LENGTH_CHUNKED, 0),
        (Some(length), false) => (CONTENT_LENGTH_EXISTS, length),
        (None, false) => (CONTENT_LENGTH_NOT_FOUND, 0),
    };
    // SAFETY: the guest's `int *`, valid by the call's contract.
    let result_written = unsafe { guest::write_u32(args[1], result) };
    // SAFETY: the guest's `uint64_t *`, likewise.
    let written = result_written && unsafe { guest::write_u64(args[2], length) };
    if written {
        ok()
    } else {
        fail(GuestError::InvalidArgument)
    }
}

/// `sceHttpReadData(request, buffer, size)`: bytes read, zero at the end of the body.
fn read_data(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let response = match response_of(id_of(args[0])) {
        Ok(response) => response,
        Err(error) => return fail(error),
    };
    let Ok(size) = usize::try_from(args[2] & 0xFFFF_FFFF) else {
        return fail(GuestError::InvalidArgument);
    };
    if args[1] == 0 {
        return fail(GuestError::InvalidArgument);
    }
    // The answer is an `int`.
    let size = size.min(i32::MAX as usize);
    let mut buffer = vec![0_u8; size];
    let read = locked(&response).body.read(&mut buffer);
    match read {
        Ok(n) => {
            // SAFETY: the guest's buffer of at least `size` bytes, by the call's contract.
            if unsafe { guest::write_bytes(args[1], &buffer[..n]) } {
                n as u64
            } else {
                fail(GuestError::InvalidArgument)
            }
        }
        Err(error) => {
            tracing::warn!("sceHttpReadData: {error}");
            fail(GuestError::HostFailed)
        }
    }
}
