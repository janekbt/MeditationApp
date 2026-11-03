# ureq 3

## Why
The WebDAV sync of both apps runs on ureq 2; ureq 3 is a rewrite and
2.x gets no more work (OPEN-ISSUES #203). On the laptop a sync once
hung for four minutes after its push although every read has a 60 s
limit (#192); two tests now hold the port to ending a request whose
server goes silent, on fresh and reused connections (they pass on
ureq 2 too, so #192's cause lies elsewhere). Two smaller sync annoyances sit in the same
lines and go with it: a server's error page or redirect target lands in
the sync status and the shareable log (#173), and a leftover `.tmp`
upload on the server is reported as unreadable on every pull (#192).

## What Changes
- `HttpWebDav` (meditate-core/src/sync/webdav.rs) on ureq 3.5.0: the six
  verbs, same behaviour as today: connect 20 s (now includes TLS), per read and per write
  60 s, no redirects (a 3xx becomes `Redirected`), 429 with
  `Retry-After`, 4xx/5xx bodies kept for the message, the GET size
  cap, the server address stripped from network errors, a non-ASCII
  folder name sent percent-encoded (ureq 3 no longer encodes it).
- New: a request whose server sends the headers and then goes silent,
  also on a reused connection, ends with a network error after the
  read limit.
- New: the sync status says only "server returned 500" or "server
  redirected"; the error page is logged as a short excerpt (512
  characters, control characters removed), the redirect target not at
  all (#173).
- New: a leftover `<name>.json.tmp` in the events folder is skipped
  silently; only other unknown files are logged (#192).
- `Cargo.lock`, `build-aux/cargo-sources.json`.
- Left out: a cap on a whole request (a 100 MB guided file on a slow
  link may take longer than any cap that is still useful; maintainer);
  deleting leftover `.tmp` files (another device may be uploading it;
  maintainer); the release-notes parser half of #173 (its own text
  defers it to the next metainfo change); batches Q, V and W otherwise
  (own changes after this one, in that order: W, V, Q; maintainer).

## Decisions
- Cargo.toml and Cargo.lock may change (maintainer, 2026-11-03).
- #192 in this change as per-read/per-write limits plus the silent-server
  test, no total cap (maintainer).
- ureq 3.5.0, not 3.5.1 (released yesterday): `ureq = "3.5"` in
  Cargo.toml, the lock held at 3.5.0 with `--precise`; the update list
  shows 3.5.1 later as a normal lock update (my call).
- Default features kept (rustls with ring and webpki-roots, gzip), as
  with ureq 2 today (my call).
- The percent-encoding is written by hand next to `url_decode` and
  `base64_encode`, not a new direct dependency (my call). It encodes
  what ureq 2's url crate encoded in a path: controls, space, `"`, `<`,
  `>`, `` ` ``, `{`, `}` and every non-ASCII byte; `%`, `?` and `#` stay
  as they are (my call).
- No proxy from the environment, no own retry on a stale pooled connection, a non-ASCII host unsupported (details in tasks.md Notes) (my call).
- The read limit becomes a constructor argument so the silent-server
  test can use 1 s; production keeps 60 s (my call).
- #173 and the `.tmp` skip are in this change; Q, V, W follow as own
  changes (maintainer, 2026-11-03).
- #173 keeps only its release-notes parser part (my call).
- #192 stays open: the silent-server tests already pass on ureq 2 (plain
  HTTP, also on a reused connection), so they protect the port but don't
  explain the hang; the entry records that and that the TLS path is
  unchecked; its `.tmp` note goes (maintainer, 2026-11-03).

## Capabilities
### Modified Capabilities
- `sync`: a server that goes silent mid-request no longer holds the sync;
  server text stays out of the status; a leftover upload is not reported.

## Impact
- Core: `meditate-core/src/sync/webdav.rs` (port, encoder, status text,
  tests), `sync/orchestrator.rs` (the `.tmp` skip),
  `meditate-core/Cargo.toml`; the comment in `sync/credentials.rs:339`.
- GTK: none (uses core). Android: none in code; the APK's native library
  changes with the new crates.
- Translations: none. Data: none (same requests on the wire).
- Android build environment: `meditate-core/Cargo.toml`, `Cargo.lock`
  (asked); new crates checked for C or system libraries; repro probe.
- Docs: OPEN-ISSUES.md (#203 out, #173 and #192 shortened, AA row).
