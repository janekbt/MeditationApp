//! WebDAV client for talking to a personal Nextcloud instance.
//!
//! The `WebDav` trait abstracts the five operations sync needs
//! (PROPFIND, GET, PUT, MKCOL, DELETE) so Phase D's pull/push
//! orchestration can be tested against an in-memory fake. The
//! production impl `HttpWebDav` is ureq-backed and Basic-auth'd
//! with an app-password from the user's Nextcloud security panel.

use std::error::Error;
use std::fmt;

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
    /// HTTP status code we didn't translate to a more specific variant.
    /// Body is included so logs can show what the server complained about.
    Server { status: u16, body: String },
    /// PROPFIND parser bailed — server responded but the XML wasn't a
    /// shape we recognise.
    MalformedResponse(String),
}

impl fmt::Display for WebDavError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound => write!(f, "WebDAV: resource not found"),
            Self::Unauthorized => write!(f, "WebDAV: unauthorized (check app password)"),
            Self::Conflict => write!(f, "WebDAV: conflict (resource exists)"),
            Self::Network(s) => write!(f, "WebDAV: network error: {s}"),
            Self::Server { status, body } => {
                write!(f, "WebDAV: server returned {status}: {body}")
            }
            Self::MalformedResponse(s) => write!(f, "WebDAV: malformed response: {s}"),
        }
    }
}

impl Error for WebDavError {}

pub type WebDavResult<T> = Result<T, WebDavError>;

pub trait WebDav {
    /// List entries in a WebDAV collection (directory). Returns each
    /// entry's URL-decoded final path segment — for events stored under
    /// `/Meditate/events/`, this means filenames like
    /// `00000000000001-{device_uuid}-{event_uuid}.json`. The collection
    /// itself is NOT included in the result.
    fn list_collection(&self, path: &str) -> WebDavResult<Vec<String>>;

    /// Download a file's full body. `NotFound` for missing paths.
    fn get(&self, path: &str) -> WebDavResult<Vec<u8>>;

    /// Upload `body` to `path`, creating or overwriting. WebDAV PUT
    /// semantics — no atomic put-if-absent unless the impl negotiates
    /// `If-None-Match: *` (we don't, in the v1 design).
    fn put(&self, path: &str, body: &[u8]) -> WebDavResult<()>;

    /// Create a new collection (directory). `Conflict` if the path
    /// already exists.
    fn mkcol(&self, path: &str) -> WebDavResult<()>;

    /// Delete a file or empty collection.
    fn delete(&self, path: &str) -> WebDavResult<()>;
}

/// Production WebDAV client. Holds the base URL of the user's WebDAV
/// endpoint (e.g. `https://nc.example.com/remote.php/dav/files/janek/`)
/// and the credentials, applies Basic-auth on every request.
pub struct HttpWebDav {
    agent: ureq::Agent,
    base_url: String,
    auth_header: String,
}

impl HttpWebDav {
    pub fn new(base_url: &str, username: &str, password: &str) -> Self {
        // Pre-compute the Basic-auth header once. Credentials shouldn't
        // change for the lifetime of this client.
        use std::io::Write;
        let mut creds = Vec::new();
        write!(&mut creds, "{username}:{password}").unwrap();
        let encoded = base64_encode(&creds);
        Self {
            agent: ureq::AgentBuilder::new().build(),
            base_url: base_url.trim_end_matches('/').to_string(),
            auth_header: format!("Basic {encoded}"),
        }
    }

    /// Combine base URL + relative path. The relative path may or may
    /// not lead with a slash; either way produces a single slash join.
    fn url(&self, path: &str) -> String {
        let trimmed = path.trim_start_matches('/');
        format!("{}/{}", self.base_url, trimmed)
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
            _ => WebDavError::Server { status, body },
        }
    }
}

impl WebDav for HttpWebDav {
    fn get(&self, path: &str) -> WebDavResult<Vec<u8>> {
        match self.agent
            .get(&self.url(path))
            .set("Authorization", &self.auth_header)
            .call()
        {
            Ok(resp) => {
                let mut body = Vec::new();
                resp.into_reader().read_to_end(&mut body)
                    .map_err(|e| WebDavError::Network(e.to_string()))?;
                Ok(body)
            }
            Err(ureq::Error::Status(status, resp)) => {
                let body = resp.into_string().unwrap_or_default();
                Err(Self::map_status_error(status, body))
            }
            Err(ureq::Error::Transport(t)) => {
                Err(WebDavError::Network(t.to_string()))
            }
        }
    }

    fn put(&self, path: &str, body: &[u8]) -> WebDavResult<()> {
        match self.agent
            .put(&self.url(path))
            .set("Authorization", &self.auth_header)
            .send_bytes(body)
        {
            Ok(_resp) => Ok(()),
            Err(ureq::Error::Status(status, resp)) => {
                let body = resp.into_string().unwrap_or_default();
                Err(Self::map_status_error(status, body))
            }
            Err(ureq::Error::Transport(t)) => Err(WebDavError::Network(t.to_string())),
        }
    }

    fn mkcol(&self, path: &str) -> WebDavResult<()> {
        // MKCOL is a custom WebDAV verb; ureq's `request(method, url)`
        // handles arbitrary methods.
        match self.agent
            .request("MKCOL", &self.url(path))
            .set("Authorization", &self.auth_header)
            .call()
        {
            Ok(_resp) => Ok(()),
            Err(ureq::Error::Status(status, resp)) => {
                let body = resp.into_string().unwrap_or_default();
                Err(Self::map_status_error(status, body))
            }
            Err(ureq::Error::Transport(t)) => Err(WebDavError::Network(t.to_string())),
        }
    }

    fn delete(&self, path: &str) -> WebDavResult<()> {
        match self.agent
            .delete(&self.url(path))
            .set("Authorization", &self.auth_header)
            .call()
        {
            Ok(_resp) => Ok(()),
            Err(ureq::Error::Status(status, resp)) => {
                let body = resp.into_string().unwrap_or_default();
                Err(Self::map_status_error(status, body))
            }
            Err(ureq::Error::Transport(t)) => Err(WebDavError::Network(t.to_string())),
        }
    }

    fn list_collection(&self, path: &str) -> WebDavResult<Vec<String>> {
        // Minimal PROPFIND body — we only need filenames, not props.
        // `<d:propname/>` asks for just the property names of each
        // resource, which is the cheapest variant.
        const BODY: &str =
            r#"<?xml version="1.0"?><d:propfind xmlns:d="DAV:"><d:propname/></d:propfind>"#;
        let response = match self.agent
            .request("PROPFIND", &self.url(path))
            .set("Authorization", &self.auth_header)
            .set("Depth", "1")
            .set("Content-Type", "application/xml")
            .send_string(BODY)
        {
            Ok(resp) => resp,
            Err(ureq::Error::Status(status, resp)) => {
                let body = resp.into_string().unwrap_or_default();
                return Err(Self::map_status_error(status, body));
            }
            Err(ureq::Error::Transport(t)) => {
                return Err(WebDavError::Network(t.to_string()));
            }
        };
        let body = response.into_string()
            .map_err(|e| WebDavError::Network(e.to_string()))?;
        parse_multistatus_filenames(&body, path)
    }
}

use std::io::Read;

/// Parse a WebDAV multistatus response body into a list of child file
/// names, with the self-entry filtered out and URL-encoded characters
/// decoded back to their literal form. `requested_path` is the path
/// the caller passed to `list_collection` — used to identify which
/// `<d:href>` is the directory itself.
fn parse_multistatus_filenames(body: &str, requested_path: &str)
    -> WebDavResult<Vec<String>>
{
    let doc = roxmltree::Document::parse(body)
        .map_err(|e| WebDavError::MalformedResponse(e.to_string()))?;

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
        let last_slash = trimmed.rfind('/').map(|i| i + 1).unwrap_or(0);
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
    let last_slash = normalised.rfind('/').map(|i| i + 1).unwrap_or(0);
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
    let mut out = String::with_capacity((input.len() + 2) / 3 * 4);
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
        let body = client.get("/file.json").unwrap();
        assert_eq!(body, b"hello world");
        mock.assert();
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
        client.get("/file.json").unwrap();
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
        let err = client.get("/missing.json").unwrap_err();
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
        let err = client.get("/file.json").unwrap_err();
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
        let err = client.get("/file.json").unwrap_err();
        match err {
            WebDavError::Server { status, body } => {
                assert_eq!(status, 500);
                assert!(body.contains("DB connection"),
                    "body should be preserved for diagnostics, got: {body}");
            }
            other => panic!("expected Server{{500}}, got {other:?}"),
        }
    }

    #[test]
    fn get_against_unreachable_server_maps_to_network_error() {
        // Bind to a port nobody's listening on — the OS will refuse the
        // connection synchronously. We use a fixed unlikely port; if
        // this test flakes it means the port was actually in use.
        let client = HttpWebDav::new("http://127.0.0.1:1", "u", "p");
        let err = client.get("/whatever").unwrap_err();
        assert!(matches!(err, WebDavError::Network(_)),
            "expected Network, got {err:?}");
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
        assert!(matches!(err, WebDavError::MalformedResponse(_)),
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
