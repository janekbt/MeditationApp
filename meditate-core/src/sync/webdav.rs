//! WebDAV client for talking to a personal Nextcloud instance.
//!
//! The `WebDav` trait abstracts the six operations sync needs
//! (PROPFIND, GET, PUT, MKCOL, MOVE, DELETE) so Phase D's pull/push
//! orchestration can be tested against an in-memory fake. The
//! production impl `HttpWebDav` is ureq-backed and Basic-auth'd
//! with an app-password from the user's Nextcloud security panel.

use std::error::Error;
use std::fmt;
use std::time::Duration;

/// Connect cap, TCP and TLS handshake together. Network-unreachable /
/// DNS-failure should surface as a `Network(_)` error quickly so the
/// sync runner records the failure and the UI's spinner clears. ureq
/// has no default — without this, a stuck connect blocks the worker
/// thread indefinitely and the in-flight flag never clears. 20 s
/// leaves room for the handshake on a slow CPU (Librem 5).
const TIMEOUT_CONNECT: Duration = Duration::from_secs(20);

/// Per-read cap inside an established connection. Generous enough
/// to absorb a slow phone uplink mid-PUT but short enough that a
/// dead TCP session doesn't leave the worker hanging forever.
const TIMEOUT_READ: Duration = Duration::from_secs(60);

/// Per-write cap. Same logic as TIMEOUT_READ — bounded so a
/// half-open connection eventually fails out rather than hanging.
const TIMEOUT_WRITE: Duration = Duration::from_secs(60);

#[derive(Debug)]
pub enum WebDavError {
    /// Resource doesn't exist (404).
    NotFound,
    /// Authentication failure (401).
    Unauthorized,
    /// Conditional request failed — typically PUT with `If-None-Match: *`
    /// against a path that already exists (412 Precondition Failed).
    Conflict,
    /// Transport-level failure (DNS, connection refused, TLS handshake,
    /// etc). The string is the underlying error's message — opaque,
    /// callers should treat it as a retry signal not a programmable error.
    Network(String),
    /// 429 Too Many Requests. The server is asking us to slow down.
    /// `retry_after` carries the `Retry-After` header value in seconds
    /// when the server provided one, and `None` otherwise — callers
    /// should fall back to exponential backoff in that case. Distinct
    /// from `Server { status: 429, .. }` so the push retry loop can
    /// recognise it and back off rather than surface a hard failure.
    RateLimited { retry_after: Option<u64> },
    /// HTTP status code we didn't translate to a more specific variant.
    /// Body is included so logs can show what the server complained about.
    Server { status: u16, body: String },
    /// PROPFIND parser bailed — server responded but the XML wasn't a
    /// shape we recognise. `detail` is the parser's error string; the
    /// page itself (a proxy's login page can name the host) only goes
    /// to the diag log, as an `error_excerpt`.
    MalformedResponse { detail: String },
    /// GET body exceeded the per-call cap. A malicious or compromised
    /// server could otherwise serve a multi-GB body and OOM the
    /// client before any post-read size check fires. `limit` is the
    /// caller-supplied cap in bytes — surfacing it in logs makes
    /// "which cap did we hit" obvious.
    ResponseTooLarge { limit: u64 },
    /// Server returned a 3xx redirect. The client follows zero
    /// redirects (`max_redirects(0)` on the Agent) so the Authorization
    /// header can never leak to a destination different from the
    /// configured base URL. The target is not kept: it can carry a user
    /// name and host, and this error lands in the status and the log.
    Redirected,
}

/// Strip URL-shaped tokens from a transport-error message.
/// A ureq error's Display may embed the request URL; these
/// strings land in the user-shareable diagnostics log and the
/// last-sync-error tooltip, so the server hostname must not ride
/// along. Keeps the cause ("Connection refused", DNS failure...).
fn sanitize_transport_msg(raw: &str) -> String {
    raw.split_whitespace()
        .filter(|w| !w.contains("://"))
        .collect::<Vec<_>>()
        .join(" ")
}

/// A transport or body-read error as `Network`, without the server address.
fn network(e: impl fmt::Display) -> WebDavError {
    WebDavError::Network(sanitize_transport_msg(&e.to_string()))
}

impl fmt::Display for WebDavError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound => write!(f, "WebDAV: resource not found"),
            Self::Unauthorized => write!(f, "WebDAV: unauthorized (check app password)"),
            Self::Conflict => write!(f, "WebDAV: conflict (resource exists)"),
            Self::Network(s) => write!(f, "WebDAV: network error: {s}"),
            Self::RateLimited { retry_after } => match retry_after {
                Some(s) => write!(f, "WebDAV: rate limited (retry after {s}s)"),
                None => write!(f, "WebDAV: rate limited"),
            },
            // The page can name the host or the user; this text lands in
            // the status and the shared log.
            Self::Server { status, .. } => write!(f, "WebDAV: server returned {status}"),
            Self::MalformedResponse { detail } => write!(f, "WebDAV: malformed response: {detail}"),
            Self::ResponseTooLarge { limit } => {
                write!(f, "WebDAV: response exceeded {limit}-byte cap")
            }
            Self::Redirected => write!(f, "WebDAV: server redirected"),
        }
    }
}

impl Error for WebDavError {}

pub type WebDavResult<T> = Result<T, WebDavError>;

pub trait WebDav {
    /// List entries in a WebDAV collection (directory). Returns each
    /// entry's URL-decoded final path segment — for events stored
    /// under `/Meditate/events/`, this means filenames like
    /// `00000000000001__{batch_uuid}.json`. The collection itself is
    /// NOT included in the result.
    fn list_collection(&self, path: &str) -> WebDavResult<Vec<String>>;

    /// Download a file's full body, capped at `max_bytes` to bound
    /// the in-memory allocation. A server that serves a body
    /// exceeding the cap surfaces `ResponseTooLarge { limit }`
    /// without buffering the rest. `NotFound` for missing paths.
    /// Each caller picks its own cap from the kind of body it
    /// expects (event-bundle JSON vs custom-sound audio).
    fn get(&self, path: &str, max_bytes: u64) -> WebDavResult<Vec<u8>>;

    /// Upload `body` to `path`, creating or overwriting. WebDAV PUT
    /// semantics — no atomic put-if-absent unless the impl negotiates
    /// `If-None-Match: *` (we don't, in the v1 design).
    fn put(&self, path: &str, body: &[u8]) -> WebDavResult<()>;

    /// Create a new collection (directory). `Conflict` if the path
    /// already exists.
    fn mkcol(&self, path: &str) -> WebDavResult<()>;

    /// Delete a file or empty collection.
    fn delete(&self, path: &str) -> WebDavResult<()>;

    /// Atomically rename a remote resource via the WebDAV MOVE verb.
    /// `from` and `to` are server-relative paths (same shape as the
    /// other verbs). On success the resource at `to` reflects the
    /// bytes that were at `from` and `from` no longer exists.
    /// Used to commit a `.tmp` upload to its canonical name only
    /// after the body bytes are durable on the server, so a
    /// partial-PUT crash never leaves a half-written file at the
    /// canonical path.
    fn move_to(&self, from: &str, to: &str) -> WebDavResult<()>;
}

/// Production WebDAV client. Holds the base URL of the user's WebDAV
/// endpoint (e.g. `https://nc.example.com/remote.php/dav/files/janek/`)
/// and the credentials, applies Basic-auth on every request.
pub struct HttpWebDav {
    agent: ureq::Agent,
    base_url: String,
    auth_header: String,
}

type Response = ureq::http::Response<ureq::Body>;

impl HttpWebDav {
    pub fn new(base_url: &str, username: &str, password: &str) -> Self {
        Self::with_read_timeout(base_url, username, password, TIMEOUT_READ)
    }

    /// `new` with its own per-read limit, so a test can wait 1 s, not 60.
    fn with_read_timeout(base_url: &str, username: &str, password: &str, read: Duration) -> Self {
        // Pre-compute the Basic-auth header once. Credentials shouldn't
        // change for the lifetime of this client.
        use std::io::Write;
        let mut creds = Vec::new();
        write!(&mut creds, "{username}:{password}").unwrap();
        let encoded = base64_encode(&creds);
        let config = ureq::Agent::config_builder()
            .timeout_connect(Some(TIMEOUT_CONNECT))
            .timeout_per_read(Some(read))
            .timeout_per_write(Some(TIMEOUT_WRITE))
            // No automatic redirect following: ureq strips
            // Authorization on cross-host redirects, but mixed-case
            // host / IDN tricks have historical bypasses and we'd
            // rather not depend on the client library's defense. A
            // redirect surfaces as `Redirected` so the user knows their
            // stored URL needs updating rather than the client silently
            // chasing a new host with their app password.
            .max_redirects(0)
            // Every status comes back as a response, so `check` can read
            // a 429's Retry-After and an error page's body.
            .http_status_as_error(false)
            // PROPFIND, MKCOL and MOVE.
            .allow_non_standard_methods(true)
            // ureq 3 would take ALL_PROXY / HTTPS_PROXY from the
            // environment; the sync has always connected directly.
            .proxy(None)
            .build();
        Self {
            agent: config.into(),
            base_url: base_url.trim_end_matches('/').to_string(),
            auth_header: format!("Basic {encoded}"),
        }
    }

    /// Combine base URL + relative path. The relative path may or may
    /// not lead with a slash; either way produces a single slash join.
    fn url(&self, path: &str) -> String {
        let trimmed = path.trim_start_matches('/');
        percent_encode_url(&format!("{}/{}", self.base_url, trimmed))
    }

    /// Send one request with Basic auth and turn every answer that isn't
    /// a 2xx into its `WebDavError`.
    fn send(
        &self,
        method: &str,
        path: &str,
        headers: &[(&str, &str)],
        body: impl ureq::AsSendBody,
    ) -> WebDavResult<Response> {
        let mut request = ureq::http::Request::builder()
            .method(method)
            .uri(self.url(path))
            .header("Authorization", &self.auth_header);
        for (name, value) in headers {
            request = request.header(*name, *value);
        }
        let request = request.body(body).map_err(network)?;
        let resp = self.agent.run(request).map_err(network)?;
        Self::check(resp)
    }

    fn check(mut resp: Response) -> WebDavResult<Response> {
        let status = resp.status().as_u16();
        match status {
            // With `max_redirects(0)` a 3xx is an answer, not an error;
            // processing its empty body as data would claim a write
            // succeeded against a path that doesn't exist.
            300..=399 => Err(WebDavError::Redirected),
            // The header MAY be an integer (seconds), an HTTP-date (rare)
            // or missing: anything but seconds leaves exponential backoff
            // to the caller.
            429 => Err(WebDavError::RateLimited {
                retry_after: resp.headers().get("Retry-After")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|s| s.trim().parse().ok()),
            }),
            400.. => {
                let body = resp.body_mut().read_to_string().unwrap_or_default();
                Err(Self::map_status_error(status, body))
            }
            _ => Ok(resp),
        }
    }

    fn map_status_error(status: u16, body: String) -> WebDavError {
        match status {
            401 => WebDavError::Unauthorized,
            404 => WebDavError::NotFound,
            // 405 on MKCOL (Method Not Allowed) means the collection
            // already exists. 409 (Conflict) on PUT means a parent path
            // is missing. 412 (Precondition Failed) is our If-None-Match
            // race signal. Treat all three as Conflict so the caller
            // can act on "resource state collided with my expectation".
            405 | 409 | 412 => WebDavError::Conflict,
            _ => {
                let body = error_excerpt(&body);
                crate::diag::log("sync.http", &format!("status={status} body={body}"));
                WebDavError::Server { status, body }
            }
        }
    }
}

impl WebDav for HttpWebDav {
    fn get(&self, path: &str, max_bytes: u64) -> WebDavResult<Vec<u8>> {
        let resp = self.send("GET", path, &[], ())?;
        // `take(max_bytes + 1)` lets us detect overflow: a body
        // at-or-under the cap fits in `max_bytes` bytes; a body OVER the
        // cap reads exactly `max_bytes + 1`. Bailing here keeps the
        // in-memory allocation bounded even when a malicious server
        // claims a small Content-Length and then streams gigabytes.
        let cap = max_bytes.saturating_add(1);
        let mut body = Vec::new();
        resp.into_body().into_reader()
            .take(cap)
            .read_to_end(&mut body)
            .map_err(network)?;
        if body.len() as u64 > max_bytes {
            return Err(WebDavError::ResponseTooLarge { limit: max_bytes });
        }
        Ok(body)
    }

    fn put(&self, path: &str, body: &[u8]) -> WebDavResult<()> {
        self.send("PUT", path, &[], body).map(drain_response_body)
    }

    fn mkcol(&self, path: &str) -> WebDavResult<()> {
        self.send("MKCOL", path, &[], ()).map(drain_response_body)
    }

    fn move_to(&self, from: &str, to: &str) -> WebDavResult<()> {
        // Per RFC 4918, MOVE carries the target in the `Destination`
        // header as an absolute URL. `Overwrite: F` would make the
        // verb refuse to clobber an existing target — we omit it
        // because the caller (atomic-PUT helper) treats overwriting
        // as the success case. The header takes the percent-encoded
        // form: header bytes outside ASCII are refused ("Übungen").
        let destination = self.url(to);
        self.send("MOVE", from, &[("Destination", &destination)], ()).map(drain_response_body)
    }

    fn delete(&self, path: &str) -> WebDavResult<()> {
        self.send("DELETE", path, &[], ()).map(drain_response_body)
    }

    fn list_collection(&self, path: &str) -> WebDavResult<Vec<String>> {
        // Minimal PROPFIND body — we only need filenames, not props.
        // `<d:propname/>` asks for just the property names of each
        // resource, which is the cheapest variant.
        const BODY: &str =
            r#"<?xml version="1.0"?><d:propfind xmlns:d="DAV:"><d:propname/></d:propfind>"#;
        let mut resp = self.send(
            "PROPFIND",
            path,
            &[("Depth", "1"), ("Content-Type", "application/xml")],
            BODY,
        )?;
        // 10 MB, lossy UTF-8.
        let body = resp.body_mut().read_to_string().map_err(network)?;
        parse_multistatus_filenames(&body, path).map_err(|e| {
            crate::diag::log("sync.http", &format!("malformed listing body={}", error_excerpt(&body)));
            e
        })
    }
}

use std::io::Read;

/// Percent-encode what an HTTP path can't carry as typed: controls,
/// space, `"<>`{}` and every non-ASCII byte ("Übungen"). `%`, `?` and
/// `#` stay, so an address typed already encoded is not encoded twice.
fn percent_encode_url(s: &str) -> String {
    use std::fmt::Write;
    let mut out = String::with_capacity(s.len());
    for &b in s.as_bytes() {
        if b <= b' ' || b >= 0x7f || b"\"<>`{}".contains(&b) {
            let _ = write!(out, "%{b:02X}");
        } else {
            out.push(b as char);
        }
    }
    out
}

/// What of a server's error page may reach the diagnostics log.
fn error_excerpt(body: &str) -> String {
    body.chars().filter(|c| !c.is_control()).take(512).collect()
}

/// Read and discard a Response's body so ureq returns the underlying
/// connection to its idle-connection pool. ureq's pool-return
/// semantics hinge on the body Reader being driven to EOF — if a verb
/// just drops the Response (the obvious thing for a "no body needed"
/// verb like PUT/MKCOL/DELETE), the connection is closed instead of
/// pooled, and every subsequent request inside the same sync pays a
/// full TLS handshake. On a slow CPU (Librem 5) those handshakes add
/// up; calling this on success paths preserves keep-alive across the
/// PROPFIND → MKCOL → PUT sequence within one sync.
fn drain_response_body(resp: Response) {
    let _ = std::io::copy(&mut resp.into_body().into_reader(), &mut std::io::sink());
}

/// Parse a WebDAV multistatus response body into a list of child file
/// names, with the self-entry filtered out and URL-encoded characters
/// decoded back to their literal form. `requested_path` is the path
/// the caller passed to `list_collection` — used to identify which
/// `<d:href>` is the directory itself.
fn parse_multistatus_filenames(body: &str, requested_path: &str)
    -> WebDavResult<Vec<String>>
{
    let malformed = |detail: String| WebDavError::MalformedResponse { detail };
    let doc = roxmltree::Document::parse(body).map_err(|e| malformed(e.to_string()))?;
    // A login page in well-formed XHTML parses too; read as a listing it
    // would be an empty folder.
    if !doc.root_element().has_tag_name(("DAV:", "multistatus")) {
        return Err(malformed("not a WebDAV multistatus".into()));
    }

    // The "self" path: the directory we asked about. Strip leading /
    // and trailing / so the comparison is path-segment based.
    let self_normalised = requested_path.trim_matches('/');

    let mut names = Vec::new();
    for href in doc.descendants()
        .filter(|n| n.has_tag_name(("DAV:", "href")))
    {
        let raw = href.text().unwrap_or("").trim();
        if raw.is_empty() { continue; }

        // Strip any URL prefix the server includes (e.g. full path
        // `/remote.php/dav/files/janek/Meditate/events/foo.json`) —
        // we only care about the final path segment.
        // Also normalise away any trailing slash.
        let trimmed = raw.trim_end_matches('/');
        // Final path segment.
        let last_slash = trimmed.rfind('/').map_or(0, |i| i + 1);
        let raw_name = &trimmed[last_slash..];
        let name = url_decode(raw_name);

        // Skip the self entry. Comparing on the trailing path segment:
        // for a request to "/Meditate/events/", the self href ends in
        // "events/" and the segment is "events"; we don't want it.
        if name == self_segment(self_normalised) {
            continue;
        }
        // Defensive: skip empty (would be the case if href ended with
        // a slash and decoded to nothing).
        if !name.is_empty() {
            names.push(name);
        }
    }
    Ok(names)
}

/// The trailing path segment of a normalised path — used to recognise
/// the "self" entry of a PROPFIND response. For "Meditate/events"
/// returns "events"; for "" returns "".
fn self_segment(normalised: &str) -> String {
    let last_slash = normalised.rfind('/').map_or(0, |i| i + 1);
    normalised[last_slash..].to_string()
}

/// Decode a single URL-encoded path segment. Hand-rolled rather than
/// pulling in `urlencoding` for one call site — handles the only
/// escape forms WebDAV servers actually emit (percent-hex pairs).
fn url_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(h), Some(l)) = (hex_val(bytes[i + 1]), hex_val(bytes[i + 2])) {
                out.push((h << 4) | l);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8(out).unwrap_or_else(|_| s.to_string())
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// Minimal Base64 encoder for the Basic-auth header. Avoids pulling in
/// the `base64` crate for a single use site (the `b64encode` ABI is
/// stable since the 1990s — no point taking on a dep for it).
fn base64_encode(input: &[u8]) -> String {
    const ALPHABET: &[u8; 64] =
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let b0 = chunk[0];
        let b1 = chunk.get(1).copied().unwrap_or(0);
        let b2 = chunk.get(2).copied().unwrap_or(0);
        out.push(ALPHABET[(b0 >> 2) as usize] as char);
        out.push(ALPHABET[(((b0 & 0x03) << 4) | (b1 >> 4)) as usize] as char);
        if chunk.len() >= 2 {
            out.push(ALPHABET[(((b1 & 0x0f) << 2) | (b2 >> 6)) as usize] as char);
        } else {
            out.push('=');
        }
        if chunk.len() >= 3 {
            out.push(ALPHABET[(b2 & 0x3f) as usize] as char);
        } else {
            out.push('=');
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_macros::assert_matches;

    // ── Basic-auth header construction ───────────────────────────────────
    //
    // Locked down at the unit-test level so it doesn't need a server to
    // verify. Basic-auth = `Basic <base64(user:pass)>`.

    #[test]
    fn basic_auth_header_matches_rfc_7617() {
        // Worked example from RFC 7617: "Aladdin:open sesame" →
        // "Basic QWxhZGRpbjpvcGVuIHNlc2FtZQ==".
        let client = HttpWebDav::new("http://x", "Aladdin", "open sesame");
        assert_eq!(client.auth_header, "Basic QWxhZGRpbjpvcGVuIHNlc2FtZQ==");
    }

    #[test]
    fn basic_auth_handles_empty_password() {
        // Edge case: app-password might be empty during config; encoder
        // must still produce a syntactically valid header (single colon).
        let client = HttpWebDav::new("http://x", "user", "");
        assert_eq!(client.auth_header, "Basic dXNlcjo=");
    }

    #[test]
    fn url_construction_joins_base_and_path_with_one_slash() {
        // Catches the "base ends in /, path starts with /" double-slash
        // bug that breaks Nextcloud's path matching.
        let with_slash = HttpWebDav::new("http://nc/", "u", "p");
        let no_slash   = HttpWebDav::new("http://nc",  "u", "p");
        assert_eq!(with_slash.url("/Meditate/x"), "http://nc/Meditate/x");
        assert_eq!(no_slash.url("/Meditate/x"),  "http://nc/Meditate/x");
        assert_eq!(no_slash.url("Meditate/x"),   "http://nc/Meditate/x");
    }

    #[test]
    fn url_encodes_what_a_path_cannot_carry_and_keeps_existing_escapes() {
        let client = HttpWebDav::new("https://nc/files/me/Meine Übungen", "u", "p");
        assert_eq!(
            client.url("/a%20b \"<>`{}\u{7f}.json"),
            "https://nc/files/me/Meine%20%C3%9Cbungen/a%20b%20%22%3C%3E%60%7B%7D%7F.json",
        );
    }

    #[test]
    fn url_construction_handles_nested_paths() {
        let client = HttpWebDav::new("http://nc/remote.php/dav/files/janek", "u", "p");
        assert_eq!(
            client.url("/Meditate/events/00000000000001-aaa-bbb.json"),
            "http://nc/remote.php/dav/files/janek/Meditate/events/00000000000001-aaa-bbb.json",
        );
    }

    // ── HTTP-level tests against mockito ─────────────────────────────────
    //
    // mockito spins up an HTTP server on a random local port and lets
    // each test specify exactly which request shape it expects. The
    // production `HttpWebDav` doesn't know it's talking to a mock —
    // these tests verify the bytes-on-the-wire behavior, including
    // headers and status mapping.

    #[test]
    fn get_returns_response_body_on_200() {
        let mut server = mockito::Server::new();
        let mock = server.mock("GET", "/file.json")
            .with_status(200)
            .with_body("hello world")
            .create();

        let client = HttpWebDav::new(&server.url(), "u", "p");
        let body = client.get("/file.json", u64::MAX).unwrap();
        assert_eq!(body, b"hello world");
        mock.assert();
    }

    /// ureq refuses header bytes outside ASCII, so with "Übungen" in the
    /// folder every MOVE failed as a network error and nothing uploaded.
    #[test]
    fn move_sends_a_non_ascii_destination_encoded() {
        let mut server = mockito::Server::new();
        let mock = server.mock("MOVE", "/files/me/%C3%9Cbungen/a.tmp")
            .match_header("Destination", format!("{}/files/me/%C3%9Cbungen/a.json", server.url()).as_str())
            .with_status(201)
            .create();

        let client = HttpWebDav::new(&format!("{}/files/me/Übungen/", server.url()), "u", "p");
        client.move_to("a.tmp", "a.json").unwrap();
        mock.assert();
    }

    #[test]
    fn get_rejects_body_exceeding_cap() {
        // A malicious or compromised Nextcloud could serve a multi-GB
        // body and OOM the client if `get` read unbounded. The
        // `max_bytes` cap stops the read at `max_bytes + 1` and
        // surfaces `ResponseTooLarge` so the orchestrator drops the
        // payload instead of buffering it.
        let mut server = mockito::Server::new();
        let _mock = server.mock("GET", "/big.json")
            .with_status(200)
            .with_body(vec![b'a'; 200])
            .create();

        let client = HttpWebDav::new(&server.url(), "u", "p");
        let err = client.get("/big.json", 100).unwrap_err();
        assert!(
            matches!(err, WebDavError::ResponseTooLarge { limit: 100 }),
            "expected ResponseTooLarge {{ limit: 100 }}, got {err:?}",
        );
    }

    #[test]
    fn get_3xx_returns_redirected() {
        // Authorization on a 3xx is the credential-exfil shape we
        // want to refuse. `max_redirects(0)` keeps ureq from chasing
        // the new host; this verb-level check turns the 3xx into a
        // typed error so the shell can tell the user to update their
        // stored URL.
        let mut server = mockito::Server::new();
        let _mock = server.mock("GET", "/old.json")
            .with_status(301)
            .with_header("Location", "https://attacker.example/new.json")
            .with_body("")
            .create();

        let client = HttpWebDav::new(&server.url(), "u", "p");
        let err = client.get("/old.json", u64::MAX).unwrap_err();
        assert_matches!(err, WebDavError::Redirected);
    }

    #[test]
    fn put_3xx_returns_redirected() {
        // Writes are the dangerous direction — if the client followed
        // a 3xx, the app password would land on the redirect target.
        let mut server = mockito::Server::new();
        let mock = server.mock("PUT", "/x.json")
            .with_status(301)
            .with_header("Location", "https://attacker.example/x.json")
            .with_body("")
            .create();

        let client = HttpWebDav::new(&server.url(), "u", "p");
        let result = client.put("/x.json", b"payload");
        mock.assert();
        let err = result.unwrap_err();
        assert!(
            matches!(err, WebDavError::Redirected),
            "expected Redirected, got {err:?}",
        );
    }

    #[test]
    fn get_at_cap_succeeds() {
        // A body exactly at the cap must NOT trip the overflow check
        // — the `take(max_bytes + 1)` boundary fires only when more
        // than `max_bytes` bytes are read.
        let mut server = mockito::Server::new();
        let _mock = server.mock("GET", "/at-cap.json")
            .with_status(200)
            .with_body(vec![b'x'; 100])
            .create();

        let client = HttpWebDav::new(&server.url(), "u", "p");
        let body = client.get("/at-cap.json", 100).unwrap();
        assert_eq!(body.len(), 100);
    }

    #[test]
    fn get_sends_basic_auth_header() {
        // The handshake-by-headers contract: every request carries the
        // pre-computed Authorization header. mockito's match_header lets
        // us assert this declaratively.
        let mut server = mockito::Server::new();
        let mock = server.mock("GET", "/file.json")
            .match_header("authorization", "Basic QWxhZGRpbjpvcGVuIHNlc2FtZQ==")
            .with_status(200)
            .with_body("ok")
            .create();

        let client = HttpWebDav::new(&server.url(), "Aladdin", "open sesame");
        client.get("/file.json", u64::MAX).unwrap();
        mock.assert();
    }

    #[test]
    fn get_404_maps_to_not_found() {
        let mut server = mockito::Server::new();
        let _mock = server.mock("GET", "/missing.json")
            .with_status(404)
            .with_body("Not Found")
            .create();

        let client = HttpWebDav::new(&server.url(), "u", "p");
        let err = client.get("/missing.json", u64::MAX).unwrap_err();
        assert!(matches!(err, WebDavError::NotFound),
            "expected NotFound, got {err:?}");
    }

    #[test]
    fn get_401_maps_to_unauthorized() {
        let mut server = mockito::Server::new();
        let _mock = server.mock("GET", "/file.json")
            .with_status(401)
            .with_body("Unauthorized")
            .create();

        let client = HttpWebDav::new(&server.url(), "u", "wrong-pass");
        let err = client.get("/file.json", u64::MAX).unwrap_err();
        assert!(matches!(err, WebDavError::Unauthorized),
            "expected Unauthorized, got {err:?}");
    }

    #[test]
    fn get_500_includes_status_and_body_in_error() {
        let mut server = mockito::Server::new();
        let _mock = server.mock("GET", "/file.json")
            .with_status(500)
            .with_body("DB connection failed")
            .create();

        let client = HttpWebDav::new(&server.url(), "u", "p");
        let err = client.get("/file.json", u64::MAX).unwrap_err();
        assert_matches!(
            err,
            WebDavError::Server { status, body } => {
                assert_eq!(status, 500);
                assert!(body.contains("DB connection"),
                    "body should be preserved for diagnostics, got: {body}");
            }
        );
    }

    // ── A server that goes silent mid-answer (#192) ──────────────────────
    //
    // mockito always answers in full, so these use a hand-written server:
    // each accepted connection answers `full` requests completely, then
    // sends only the headers of the next one and holds the socket open.

    /// Start the server; returns its URL and a counter of accepted connections.
    fn silent_after_headers_server(full: usize) -> (String, std::sync::Arc<std::sync::atomic::AtomicUsize>) {
        use std::io::{BufRead, BufReader, Write};
        use std::sync::atomic::{AtomicUsize, Ordering};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let accepted = std::sync::Arc::new(AtomicUsize::new(0));
        let count = accepted.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { return };
                count.fetch_add(1, Ordering::SeqCst);
                std::thread::spawn(move || {
                    let mut reader = BufReader::new(stream.try_clone().unwrap());
                    for answered in 0.. {
                        // Read one request's head (our GETs carry no body).
                        let mut line = String::new();
                        loop {
                            line.clear();
                            if reader.read_line(&mut line).unwrap_or(0) == 0 { return; }
                            if line == "\r\n" { break; }
                        }
                        if answered < full {
                            let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok");
                        } else {
                            let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 10\r\n\r\n");
                            std::thread::sleep(Duration::from_secs(10));
                            return;
                        }
                    }
                });
            }
        });
        (url, accepted)
    }

    #[test]
    fn a_server_silent_after_the_headers_ends_the_request() {
        let (url, _) = silent_after_headers_server(0);
        let client = HttpWebDav::with_read_timeout(&url, "u", "p", Duration::from_secs(1));
        let started = std::time::Instant::now();
        let err = client.get("/a.json", u64::MAX).unwrap_err();
        assert_matches!(err, WebDavError::Network(_));
        assert!(started.elapsed() < Duration::from_secs(5), "waited {:?}", started.elapsed());
    }

    #[test]
    fn a_server_silent_on_a_reused_connection_ends_the_request() {
        let (url, accepted) = silent_after_headers_server(1);
        let client = HttpWebDav::with_read_timeout(&url, "u", "p", Duration::from_secs(1));
        assert_eq!(client.get("/a.json", u64::MAX).unwrap(), b"ok");
        let started = std::time::Instant::now();
        let err = client.get("/b.json", u64::MAX).unwrap_err();
        assert_matches!(err, WebDavError::Network(_));
        assert!(started.elapsed() < Duration::from_secs(5), "waited {:?}", started.elapsed());
        assert_eq!(accepted.load(std::sync::atomic::Ordering::SeqCst), 1, "the second request must reuse the connection");
    }

    /// ureq 3 reads at most 10 MB through `read_to_vec`; a guided file
    /// may be 100 MB, so GET must not use it.
    #[test]
    fn get_reads_a_body_over_ten_megabytes() {
        let size = 11 * 1024 * 1024;
        let mut server = mockito::Server::new();
        let mock = server.mock("GET", "/big.ogg")
            .with_status(200)
            .with_body(vec![7u8; size])
            .create();
        let client = HttpWebDav::new(&server.url(), "u", "p");
        assert_eq!(client.get("/big.ogg", u64::MAX).unwrap().len(), size);
        mock.assert();
    }

    // ── Server text stays out of the sync status (#173) ──────────────────

    #[test]
    fn a_server_error_shows_only_its_status() {
        let mut server = mockito::Server::new();
        let mock = server.mock("GET", "/a.json")
            .with_status(500)
            .with_body("<html>nc.example.com: DB connection lost</html>")
            .create();
        let client = HttpWebDav::new(&server.url(), "u", "p");
        let err = client.get("/a.json", u64::MAX).unwrap_err();
        assert_eq!(err.to_string(), "WebDAV: server returned 500");
        mock.assert();
    }

    #[test]
    fn a_redirect_error_hides_its_target() {
        let mut server = mockito::Server::new();
        let mock = server.mock("GET", "/a.json")
            .with_status(302)
            .with_header("Location", "https://janek@other.example/remote.php/dav/files/janek/a.json")
            .create();
        let client = HttpWebDav::new(&server.url(), "u", "p");
        let err = client.get("/a.json", u64::MAX).unwrap_err();
        assert_eq!(err.to_string(), "WebDAV: server redirected");
        mock.assert();
    }

    #[test]
    fn a_listing_that_is_a_web_page_shows_no_page_text() {
        let mut server = mockito::Server::new();
        let mock = server.mock("PROPFIND", "/x/")
            .with_status(200)
            .with_body("<html><body>Sign in to sso.nc.example.com<br></body></html>")
            .create();
        let client = HttpWebDav::new(&server.url(), "u", "p");
        let shown = client.list_collection("/x/").unwrap_err().to_string();
        assert!(shown.starts_with("WebDAV: malformed response: "), "{shown}");
        assert!(!shown.contains("nc.example.com") && !shown.contains("Sign in"), "{shown}");
        mock.assert();
    }

    /// A login page that is well-formed XHTML parses as XML; it must not
    /// pass as an empty events folder.
    #[test]
    fn a_listing_that_is_an_xhtml_page_is_malformed() {
        let mut server = mockito::Server::new();
        let mock = server.mock("PROPFIND", "/x/")
            .with_status(200)
            .with_body(r#"<html xmlns="http://www.w3.org/1999/xhtml"><body>Sign in to sso.nc.example.com</body></html>"#)
            .create();
        let client = HttpWebDav::new(&server.url(), "u", "p");
        let err = client.list_collection("/x/").unwrap_err();
        assert_matches!(err, WebDavError::MalformedResponse { .. });
        assert!(!err.to_string().contains("nc.example.com"), "{err}");
        mock.assert();
    }

    #[test]
    fn the_logged_error_excerpt_is_short_and_clean() {
        let body = format!("a\nb\u{1b}[31mc\r\t{}", "Ü".repeat(600));
        assert_eq!(error_excerpt(&body), format!("ab[31mc{}", "Ü".repeat(505)));
    }

    #[test]
    fn get_against_unreachable_server_maps_to_network_error() {
        // Bind to a port nobody's listening on — the OS will refuse the
        // connection synchronously. We use a fixed unlikely port; if
        // this test flakes it means the port was actually in use.
        let client = HttpWebDav::new("http://127.0.0.1:1", "u", "p");
        let err = client.get("/whatever", u64::MAX).unwrap_err();
        assert!(matches!(err, WebDavError::Network(_)),
            "expected Network, got {err:?}");
        assert!(!err.to_string().contains("127.0.0.1"), "the address must not reach the status: {err}");
    }

    // ── C1.B: PUT, DELETE, MKCOL ─────────────────────────────────────────
    //
    // Same shape as GET: HTTP verb + URL + auth header → response status
    // mapped to either Ok(()) or a typed WebDavError. PUT carries a body;
    // the others don't.

    #[test]
    fn put_uploads_body_and_returns_ok_on_201() {
        // 201 Created is the canonical "we made it" response for a PUT
        // that creates a new resource.
        let mut server = mockito::Server::new();
        let mock = server.mock("PUT", "/Meditate/events/foo.json")
            .match_header("authorization", mockito::Matcher::Any)
            .match_body("payload bytes")
            .with_status(201)
            .create();

        let client = HttpWebDav::new(&server.url(), "u", "p");
        client.put("/Meditate/events/foo.json", b"payload bytes").unwrap();
        mock.assert();
    }

    #[test]
    fn put_returns_ok_on_204_no_content() {
        // 204 No Content is what Nextcloud returns when PUT overwrites
        // an existing resource. Must also be treated as success.
        let mut server = mockito::Server::new();
        let _mock = server.mock("PUT", "/file.json")
            .with_status(204)
            .create();
        let client = HttpWebDav::new(&server.url(), "u", "p");
        client.put("/file.json", b"new body").unwrap();
    }

    #[test]
    fn put_404_on_missing_parent_collection_maps_to_not_found() {
        // PUT into a path whose parent collection doesn't exist returns
        // 404 (or 409 — varies by server). The 404 shape must surface
        // as NotFound so the sync layer can choose to MKCOL and retry.
        let mut server = mockito::Server::new();
        let _mock = server.mock("PUT", "/missing-dir/file.json")
            .with_status(404)
            .create();
        let client = HttpWebDav::new(&server.url(), "u", "p");
        let err = client.put("/missing-dir/file.json", b"x").unwrap_err();
        assert!(matches!(err, WebDavError::NotFound),
            "expected NotFound, got {err:?}");
    }

    #[test]
    fn put_409_conflict_maps_to_conflict() {
        // 409 means a parent path is missing or some other state
        // collision; surface as Conflict so the caller can decide
        // (typically: MKCOL the parent and retry).
        let mut server = mockito::Server::new();
        let _mock = server.mock("PUT", "/file.json")
            .with_status(409)
            .create();
        let client = HttpWebDav::new(&server.url(), "u", "p");
        let err = client.put("/file.json", b"x").unwrap_err();
        assert!(matches!(err, WebDavError::Conflict),
            "expected Conflict, got {err:?}");
    }

    // ── 429 rate-limit handling ──────────────────────────────────────────
    //
    // Nextcloud, like most servers, can return 429 Too Many Requests
    // when a client pushes too fast. The WebDavError::RateLimited
    // variant lets the push retry loop distinguish this from an
    // unrecoverable error and back off rather than fail the sync.

    #[test]
    fn put_429_with_no_retry_after_maps_to_rate_limited_with_none() {
        // Without a Retry-After header the caller falls back to
        // exponential backoff. RateLimited { retry_after: None } is the
        // signal for that path.
        let mut server = mockito::Server::new();
        let _mock = server.mock("PUT", "/file.json")
            .with_status(429)
            .create();
        let client = HttpWebDav::new(&server.url(), "u", "p");
        let err = client.put("/file.json", b"x").unwrap_err();
        assert_matches!(
            err,
            WebDavError::RateLimited { retry_after } => assert_eq!(
                retry_after, None,
                "no Retry-After header → retry_after must be None",
            ),
        );
    }

    #[test]
    fn put_429_with_retry_after_seconds_passes_value_through() {
        // Server says "wait 30 s" — the value must reach the caller
        // verbatim so the backoff state can honor it.
        let mut server = mockito::Server::new();
        let _mock = server.mock("PUT", "/file.json")
            .with_status(429)
            .with_header("retry-after", "30")
            .create();
        let client = HttpWebDav::new(&server.url(), "u", "p");
        let err = client.put("/file.json", b"x").unwrap_err();
        assert_matches!(
            err,
            WebDavError::RateLimited { retry_after } => assert_eq!(retry_after, Some(30)),
        );
    }

    #[test]
    fn put_429_with_garbled_retry_after_falls_back_to_none() {
        // Some servers send Retry-After as an HTTP-date instead of
        // seconds, or as a malformed value. Don't crash — gracefully
        // fall back to "none" so exponential backoff kicks in.
        let mut server = mockito::Server::new();
        let _mock = server.mock("PUT", "/file.json")
            .with_status(429)
            .with_header("retry-after", "not a number")
            .create();
        let client = HttpWebDav::new(&server.url(), "u", "p");
        let err = client.put("/file.json", b"x").unwrap_err();
        assert!(matches!(err, WebDavError::RateLimited { retry_after: None }),
            "unparseable Retry-After must degrade to None, got {err:?}");
    }

    #[test]
    fn rate_limited_display_text_is_informative() {
        // The error string flows into diag logs and (for non-429
        // failures) sometimes user-visible toasts. Verify the wording
        // includes the key signal so a sleepy debugger spots it fast.
        assert!(
            WebDavError::RateLimited { retry_after: Some(45) }.to_string()
                .contains("rate limited"),
            "Display must mention 'rate limited'",
        );
        assert!(
            WebDavError::RateLimited { retry_after: Some(45) }.to_string()
                .contains("45"),
            "Display must include the suggested retry-after value",
        );
    }

    #[test]
    fn delete_returns_ok_on_204() {
        // 204 No Content is the canonical success for DELETE.
        let mut server = mockito::Server::new();
        let mock = server.mock("DELETE", "/file.json")
            .match_header("authorization", mockito::Matcher::Any)
            .with_status(204)
            .create();
        let client = HttpWebDav::new(&server.url(), "u", "p");
        client.delete("/file.json").unwrap();
        mock.assert();
    }

    #[test]
    fn delete_404_maps_to_not_found() {
        // Idempotency at the protocol level: deleting a missing file
        // returns 404. Sync's compaction may want to ignore this; we
        // surface the typed error and let the caller decide.
        let mut server = mockito::Server::new();
        let _mock = server.mock("DELETE", "/missing.json")
            .with_status(404)
            .create();
        let client = HttpWebDav::new(&server.url(), "u", "p");
        let err = client.delete("/missing.json").unwrap_err();
        assert!(matches!(err, WebDavError::NotFound),
            "expected NotFound, got {err:?}");
    }

    #[test]
    fn mkcol_returns_ok_on_201_created() {
        let mut server = mockito::Server::new();
        let mock = server.mock("MKCOL", "/Meditate/")
            .match_header("authorization", mockito::Matcher::Any)
            .with_status(201)
            .create();
        let client = HttpWebDav::new(&server.url(), "u", "p");
        client.mkcol("/Meditate/").unwrap();
        mock.assert();
    }

    #[test]
    fn mkcol_405_on_existing_collection_maps_to_conflict() {
        // Per RFC 4918: MKCOL on an existing path returns 405 Method
        // Not Allowed. The right semantic is "already there" which
        // we model as Conflict so the caller can swallow it.
        let mut server = mockito::Server::new();
        let _mock = server.mock("MKCOL", "/Meditate/")
            .with_status(405)
            .create();
        let client = HttpWebDav::new(&server.url(), "u", "p");
        let err = client.mkcol("/Meditate/").unwrap_err();
        assert!(matches!(err, WebDavError::Conflict),
            "expected Conflict for 405-on-existing-collection, got {err:?}");
    }

    #[test]
    fn mkcol_404_when_grandparent_missing_maps_to_not_found() {
        // Creating /a/b/c/ when /a/b/ doesn't exist → 404 or 409. Per
        // map_status_error, 404 → NotFound, 409 → Conflict. Both shapes
        // need to surface intelligibly.
        let mut server = mockito::Server::new();
        let _mock = server.mock("MKCOL", "/missing-grandparent/Meditate/")
            .with_status(404)
            .create();
        let client = HttpWebDav::new(&server.url(), "u", "p");
        let err = client.mkcol("/missing-grandparent/Meditate/").unwrap_err();
        assert!(matches!(err, WebDavError::NotFound),
            "expected NotFound, got {err:?}");
    }

    #[test]
    fn put_carries_basic_auth_like_get_does() {
        // Defensive against forgetting auth on one verb but not another.
        let mut server = mockito::Server::new();
        let mock = server.mock("PUT", "/x")
            .match_header("authorization", "Basic QWxhZGRpbjpvcGVuIHNlc2FtZQ==")
            .with_status(201)
            .create();
        let client = HttpWebDav::new(&server.url(), "Aladdin", "open sesame");
        client.put("/x", b"y").unwrap();
        mock.assert();
    }

    // ── C1.C: PROPFIND with XML response parsing ─────────────────────────
    //
    // Lists a WebDAV collection. The request is a custom PROPFIND verb
    // with `Depth: 1` (= "this collection plus its direct children")
    // and a small XML body asking for a basic prop set. The response
    // is 207 Multi-Status with one `<d:response>` per resource.
    //
    // We extract just the child filenames (final path segments), URL-
    // decoded so callers see "foo bar.json" not "foo%20bar.json".

    /// Helper: a realistic Nextcloud-style multistatus body.
    fn multistatus_body(href_paths: &[&str]) -> String {
        let mut s = String::from(
            r#"<?xml version="1.0"?><d:multistatus xmlns:d="DAV:">"#);
        for path in href_paths {
            s.push_str(&format!(
                r#"<d:response><d:href>{path}</d:href><d:propstat><d:prop><d:resourcetype/></d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response>"#));
        }
        s.push_str("</d:multistatus>");
        s
    }

    #[test]
    fn list_collection_returns_empty_when_only_self_in_response() {
        // Empty directory: Nextcloud returns one <d:response> for the
        // directory itself, no children.
        let mut server = mockito::Server::new();
        let _mock = server.mock("PROPFIND", "/Meditate/events/")
            .with_status(207)
            .with_header("content-type", "application/xml; charset=utf-8")
            .with_body(multistatus_body(&["/Meditate/events/"]))
            .create();

        let client = HttpWebDav::new(&server.url(), "u", "p");
        let names = client.list_collection("/Meditate/events/").unwrap();
        assert!(names.is_empty(),
            "self-only response must yield empty list, got {names:?}");
    }

    #[test]
    fn list_collection_returns_filenames_for_each_child() {
        let mut server = mockito::Server::new();
        let _mock = server.mock("PROPFIND", "/Meditate/events/")
            .with_status(207)
            .with_body(multistatus_body(&[
                "/Meditate/events/",
                "/Meditate/events/00000000000001-aaa-bbb.json",
                "/Meditate/events/00000000000002-aaa-ccc.json",
            ]))
            .create();

        let client = HttpWebDav::new(&server.url(), "u", "p");
        let mut names = client.list_collection("/Meditate/events/").unwrap();
        names.sort();
        assert_eq!(names, vec![
            "00000000000001-aaa-bbb.json".to_string(),
            "00000000000002-aaa-ccc.json".to_string(),
        ]);
    }

    #[test]
    fn list_collection_url_decodes_filenames() {
        // PROPFIND href values are URL-encoded — a space becomes %20,
        // a colon %3A, etc. Callers expect the decoded name so they
        // can match it against the unencoded local DB representation.
        let mut server = mockito::Server::new();
        let _mock = server.mock("PROPFIND", "/x/")
            .with_status(207)
            .with_body(multistatus_body(&[
                "/x/",
                "/x/has%20space.json",
                "/x/colon%3Astuff.json",
            ]))
            .create();
        let client = HttpWebDav::new(&server.url(), "u", "p");
        let mut names = client.list_collection("/x/").unwrap();
        names.sort();
        assert_eq!(names, vec![
            "colon:stuff.json".to_string(),
            "has space.json".to_string(),
        ]);
    }

    #[test]
    fn list_collection_sends_depth_1_header() {
        // `Depth: infinity` would recursively list everything which
        // we don't want — only direct children. Nextcloud requires
        // explicit `Depth: 1` for non-recursive listings.
        let mut server = mockito::Server::new();
        let mock = server.mock("PROPFIND", "/x/")
            .match_header("depth", "1")
            .with_status(207)
            .with_body(multistatus_body(&["/x/"]))
            .create();
        let client = HttpWebDav::new(&server.url(), "u", "p");
        let _ = client.list_collection("/x/").unwrap();
        mock.assert();
    }

    #[test]
    fn list_collection_404_maps_to_not_found() {
        let mut server = mockito::Server::new();
        let _mock = server.mock("PROPFIND", "/missing/")
            .with_status(404)
            .create();
        let client = HttpWebDav::new(&server.url(), "u", "p");
        let err = client.list_collection("/missing/").unwrap_err();
        assert!(matches!(err, WebDavError::NotFound),
            "expected NotFound, got {err:?}");
    }

    #[test]
    fn list_collection_malformed_xml_maps_to_malformed_response() {
        // Server claims success but body isn't valid XML — likely a
        // misconfigured proxy. Surface as MalformedResponse so the
        // sync layer can retry or surface a config-error message.
        let mut server = mockito::Server::new();
        let _mock = server.mock("PROPFIND", "/x/")
            .with_status(207)
            .with_body("not actually XML at all")
            .create();
        let client = HttpWebDav::new(&server.url(), "u", "p");
        let err = client.list_collection("/x/").unwrap_err();
        assert!(matches!(err, WebDavError::MalformedResponse { .. }),
            "expected MalformedResponse, got {err:?}");
    }

    #[test]
    fn list_collection_handles_uppercase_dav_namespace_prefix() {
        // Some servers emit `<D:href>` with capital D. The parser is
        // namespace-aware, not prefix-aware — both must work.
        let mut server = mockito::Server::new();
        let _mock = server.mock("PROPFIND", "/x/")
            .with_status(207)
            .with_body(
                r#"<?xml version="1.0"?>
                <D:multistatus xmlns:D="DAV:">
                  <D:response><D:href>/x/</D:href></D:response>
                  <D:response><D:href>/x/file.json</D:href></D:response>
                </D:multistatus>"#)
            .create();
        let client = HttpWebDav::new(&server.url(), "u", "p");
        let names = client.list_collection("/x/").unwrap();
        assert_eq!(names, vec!["file.json".to_string()]);
    }


    #[test]
    fn list_collection_excludes_self_when_self_href_lacks_trailing_slash() {
        // Some servers omit the trailing slash on the self-href even
        // though the request had one. The "self" filter must compare
        // the path part, not the exact string match.
        let mut server = mockito::Server::new();
        let _mock = server.mock("PROPFIND", "/x/")
            .with_status(207)
            .with_body(multistatus_body(&[
                "/x",                 // self, no trailing slash
                "/x/child.json",
            ]))
            .create();
        let client = HttpWebDav::new(&server.url(), "u", "p");
        let names = client.list_collection("/x/").unwrap();
        assert_eq!(names, vec!["child.json".to_string()],
            "self-href without trailing slash must still be filtered out");
    }
}
