# Tasks: ureq-3

Base: 5aa91a9add7330c7a29165ebe138bb3f0acfe2c8; suites on it: green (meditate-core 1462 incl. integration, meditate-android 287, meditate 168; run on this exact tree before the #202 commit)

## Loop
- [x] 0 Skills loaded, again after every /compact: `/ponytail ultra`; `slint:slint` (loaded, no Android UI touched)
- [x] 1 Understand
- [x] 2 Propose (approved: 2026-11-03)
- [x] 3 Build
- [x] 4 Review, then `/simplify` after the last review (two reviewers, two fix reviews, /simplify)
- [x] 5 Specs, docs, list (check-specs ok; OPEN-ISSUES #203 out, #173 and #192 shortened, #183 updated, AA row; ARCHITECTURE.md still right)
- [x] 6 Verify and hand over
- [x] 7 Ship

## Review Focus
- A 3xx with `max_redirects(0)`: returned as a response, must become `Redirected` -> existing `get_3xx_returns_redirected` (renamed: the target is no longer kept), `put_3xx_returns_redirected`
- 4xx/5xx: ureq 3 errors by default and drops the body and headers -> `http_status_as_error(false)`; existing status-mapping and 429 `Retry-After` tests
- ureq 3's own body limit (10 MB on `read_to_string`/`read_to_vec`, none on `into_reader`): GET reads through `into_reader().take(max_bytes + 1)` as today, so a 100 MB guided file passes -> 1.3 test with a GET body over 10 MB; PROPFIND and error bodies keep `read_to_string` (10 MB, lossy UTF-8: the same cap and decoding as ureq 2's `into_string`)
- Non-ASCII and space in the base URL or the path (folder "Übungen", "Meine Übungen") -> existing `move_sends_a_non_ascii_destination_encoded`; 1.2 unit test of the encoder (space, Ü, an existing `%20` left alone)
- The server address in a network error message -> existing `get_against_unreachable_server_maps_to_network_error` plus 1.3 checks the new error Display
- Connections returned to the pool only after the body is read -> drain kept; the reused-connection scenario proves reuse happens
- Android graph: new crates with a build script compiling C or looking up a system library -> 1.1 tree diff; stop and ask if one appears
- The read limit on a reused connection (#192) -> scenario 2, run first on ureq 2 to see whether it is the cause
- An excerpt cut in the middle of a multi-byte character -> excerpt counts characters, not bytes (as `MalformedResponse` does); the clean-excerpt test uses umlauts
- A file named only `.tmp`, or `x.tmp` that is not `.json.tmp` -> skipped too (any `.tmp` is an upload leftover by our own naming), test covers `.json.tmp`
- TLS roots on the phone (webpki-roots 1.x instead of 0.26) -> 3.1 sync against the real Nextcloud from the phone

## 1. Build
- [x] 1.1 Silent-server tests first, on ureq 2: the read limit becomes a constructor argument (`HttpWebDav::new` keeps 60 s); both scenario tests with a hand-written `TcpListener` server and a 1 s limit; run, note in Notes whether each fails on ureq 2
- [x] 1.2 Encoder test, then a hand-written path encoder used by `url()` (see proposal); the MOVE `Destination` header uses it too
- [x] 1.3 `ureq = "3.5"`, lock update of ureq alone, held at 3.5.0 with `--precise`; check cc 1.2.60 and kstring 2.0.2; Android graph before/after (aarch64, normal + build). Port the six verbs (Agent config: connect 20 s, per read/write limits, `max_redirects(0)`, `http_status_as_error(false)`, `allow_non_standard_methods(true)`, `proxy(None)`). Comment in `credentials.rs:339`. A GET over 10 MB test. Proof: all webdav tests and both scenario tests green
- [x] 1.3b #173: the three status-text tests first (fail on today's Display), then Display without body and Location; the body is cut to the clean excerpt where the `Server` error is made (so the existing `sync.push.refused` log line at `orchestrator.rs:811` carries only the excerpt) and logged there once as `sync.http`; the Test-connection detail (`credentials.rs:80`, `Other(e.to_string())`) follows the Display
- [x] 1.3c `.tmp` test first (a pure helper that returns the names to report from a listing), then the skip in the pull loop
- [x] 1.3d Review fix (maintainer's go, 2026-11-03): a listing that is a web page: test first, then `MalformedResponse` keeps only the parser's complaint, the page is logged as the clean excerpt
- [x] 1.4 Proof: tests of meditate-core (all targets), meditate-android `--lib`, meditate; both clippy commands, zero warnings
- [x] 1.5 Regenerate `build-aux/cargo-sources.json`. Proof: 2.1

## 2. Builds
- [x] 2.1 `flatpak.yml` run by hand on `deps-test` (commit object): all jobs green, bundles built (run 41671296346, all five jobs green)
- [x] 2.2 Android debug build; `REF=<commit object> build-aux/repro-probe.sh`: payloads identical

## 3. Hand-over
- [x] 3.1 Debug APK on the FP5 (`adb install -r`, keeps its data) and the CI Flatpak bundle installed over 26.11.0 on the laptop (real data, real keyring): a sync on each without errors; a short session on the phone shows on the laptop after a sync, and one from the laptop on the phone. Proof: maintainer says it works (2026-11-03: phone test passed, all three laptop tests passed)

## Notes
- Pre-build agent, checked against the ureq 3.5.0 source and decided as my call:
  1. ureq 3 reads `ALL_PROXY`/`HTTPS_PROXY`/`HTTP_PROXY` by default (`Config::default`, `proxy.rs:232`); ureq 2 here did not: `proxy(None)` keeps today's direct connection.
  2. ureq 2 retried a GET/DELETE once on a stale pooled connection; ureq 3 has no retry but probes a pooled connection before reuse (`pool.rs:192`) and drops idle ones after 15 s. No retry of our own; a rare failed sync is retried by the next one. The device test watches for new errors.
  3. Encoder scope: the listed bytes, on the whole URL (base and path), so "Übungen" in the base works; a non-ASCII host (IDN) is not supported (it would be encoded and fail). Host case and `:443` stay as typed; Nextcloud matches MOVE `Destination` by path.
  4. PROPFIND and error bodies keep a 10 MB lossy read, as today (no raise to 64 MB).
  5. ureq 3's connect limit includes the TLS handshake (ureq 2: TCP only): 20 s instead of 10 s, so a slow CPU on a slow link still connects.
  6. Network error text becomes ureq 3's wording (e.g. "io: Connection refused (os error 111)"), URLs still stripped; English, as today.
  Also: `allow_non_standard_methods(true)` for PROPFIND, MKCOL and MOVE; `new` keeps its signature (`runner.rs:56` passes it as a function pointer), the read limit goes into a second constructor; User-Agent becomes `ureq/3.5.0`.
- 1.1: both silent-server tests pass at once on ureq 2 (1 s limit, plain HTTP, also on the reused connection: one accepted connection). Not empty: with the read limit set to 30 s both fail after 10 s. So the behaviour existed, and a plain-HTTP read on a reused connection is not #192's cause; the TLS path is unchecked. Maintainer (2026-11-03): keep the tests as protection for the port, #192 stays open with this finding (proposal Decisions changed accordingly).
- 1.2: encoder test failed on the stub (unencoded), green after; all 40 webdav tests green on ureq 2 with it.
- 1.3b: the three #173 tests failed on the old Display / stub excerpt, green after.
- 1.3c: the `.tmp` test failed with the `.tmp` reported, green after; the pull loop now takes the names to report from the helper.
- 1.3: lock: ureq 2.12.1 -> 3.5.0 (held with `--precise`; a plain update took 3.5.1), ureq-proto 0.6.5, http 1.5.0, httparse 1.10.1, utf8-zero 0.8.1, base64 0.23.1 added; base64 0.22.1 and webpki-roots 0.26.11 (a shim over 1.x) gone. cc 1.2.60, kstring 2.0.2 unchanged. Android graph diff: those crates only; httparse's build script only reads `rustc --version` for SIMD cfgs; nothing compiles C or looks up a system library. Port: one `send` + `check` for all six verbs instead of six match blocks (3xx -> Redirected, 429 -> RateLimited, 4xx/5xx -> body read 10 MB lossy -> mapped). All 43 webdav tests green at once, incl. both silent-server tests on ureq 3. New `get_reads_a_body_over_ten_megabytes`: fails when GET reads through `read_to_vec` (10 MB), green with `into_reader().take`.
- 1.4: meditate-core 1452 + 4 + 11 + 3 (base 1444: 8 new tests), meditate-android 287, meditate 168; both clippy commands zero warnings.
- 1.5: cargo-sources.json regenerated (ureq-3.5.0, ureq-proto-0.6.5, http-1.5.0, httparse-1.10.1, utf8-zero-0.8.1, base64-0.23.1 in it).
- CI commit object 82cf4d4 pushed to `deps-test`; flatpak.yml run 41666977925. Android debug APK 16:45.
- 2.2: repro probe on 82cf4d4 (ureq 3.5.0 compiled in both clones): 28/28 identical. The review fixes after it are plain Rust in core and GTK (no crate, no build file), so the probe still stands for the crate change. Probe clones deleted.
- Review (two fresh agents, data/sync and completeness), checked against the code:
  - Fixed: `unreadable_names_to_report`'s doc comment had split `parse_batch_uuid_from_filename`'s (both reviewers).
  - Fixed: `Redirected.location` was kept but never shown or logged: field removed (the type now enforces "the target not at all"), test renamed `get_3xx_returns_redirected`, clones in credentials.rs and gtk sync_runner.rs follow.
  - Fixed: `get_against_unreachable_server_maps_to_network_error` now asserts the address is not in the message (Review Focus promised it).
  - Fixed with the maintainer's go: a listing that is a web page (proxy login page) showed 512 characters of it in the status. New scenario and test `a_listing_that_is_a_web_page_shows_no_page_text` (failed first); `MalformedResponse` keeps only the parser's complaint, the page goes to the log as `error_excerpt`.
  - Fixed: OPEN-ISSUES #183 pointed at ureq 2 lines; now names ureq 3's `TlsConfig` roots.
  - Rejected: the logged excerpt can still name the host (`sync.http`, `sync.push.refused`). The approved design logs a cleaned excerpt; stripping URL tokens would not catch a bare host name, and the log is the user's own to share.
  - After the fixes: meditate-core 1453 + 4 + 11 + 3, meditate 168, meditate-android 287; both clippy commands zero warnings.
- Fix review (fresh agent): 1 a well-formed XHTML login page parsed and read as an empty events folder (older than this change, but the approved requirement's case): new scenario and test `a_listing_that_is_an_xhtml_page_is_malformed` (failed first); the root must be `DAV:multistatus`, else `MalformedResponse` with the page logged. 2 stale `.redirects(0)` in a test comment: fixed. meditate-core 1454 + 4 + 11 + 3 green.
- Second fix review (fresh agent): PASS. Real servers' roots (sabre/dav, mod_dav, nginx dav-ext, x/net/webdav) are DAV:multistatus under any prefix; declaration, BOM, whitespace pass. Skipped its optional finding: Test connection on such a page says "Server error" rather than "Not a WebDAV folder"; that is a different toast text, not in the proposal, and the failure is reported either way.
- /simplify (4 agents), applied: one `network()` for every transport or body-read error (was three copies); the four no-body verbs as `send(..).map(drain_response_body)`; the malformed-listing log moved from the parser to `list_collection` (the parser is pure again); a refused upload logs its page once (`sync.http`), `sync.push.refused` keeps uuid and status; `UPLOAD_SUFFIX` shared by the upload and the `.tmp` filter; the encoder writes into its string without a temporary per byte; stale comments (module doc "six operations", the Display comment). Skipped: `#[derive(Clone)]` on `WebDavError` to replace the two test `clone_err`s (outside this change's shape); folding `map_status_error` into `check` (optional); dropping `Server.body` (12 sites, tests read it); the second parse of the listing names (the price of a testable helper); reading only 4 KiB of an error page (the connection would not be reused; same full read as ureq 2). After: meditate-core 1454 + 4 + 11 + 3, meditate 168, meditate-android 287; both clippy commands zero warnings.
- First CI run 41666977925 (82cf4d4, before the review fixes): metadata, tests + clippy, Android APK, Flatpak x86_64 green; aarch64 cancelled by me once the fixes made it stale. Final commit object b07f179 force-pushed to `deps-test`; flatpak.yml run 41671296346. Android debug APK with the final code 17:10.

## Hand-over
Artifacts: `meditate-android/android/app/build/outputs/apk/debug/app-debug.apk` (2026-11-03 17:10), the x86_64 bundle `meditate-x86_64.flatpak` from run 41671296346 (download to `~/.cache/meditate-ci/`).
- Phone: `adb install -r` the APK (closes Meditate if open; data stays). A sync from the sync status: no error. Make a short session; sync.
- Laptop: install the bundle with `--user` over 26.11.0 (real data, real keyring; it stays the beta build until the next release). Start it, sync: no error, and the phone's session from above appears in the Log. Make a short session; sync.
- Phone again: sync; the laptop's session appears in the Log.
- Both: no sync error over the test (watch for "io: ..." or "connection" errors from a reused connection).

Proposed commit:
```
sync: WebDAV on ureq 3; server text out of the status

ureq 2 gets no more work. The six verbs now share one send/check path
with the same limits as before (per read and write 60 s, connect 20 s
now that it includes TLS, no redirects, no proxy from the environment)
and encode non-ASCII paths themselves, which ureq 3 no longer does.

A server's error page, a redirect target or a login page in place of a
folder listing no longer reaches the sync status; the log gets a short
cleaned excerpt. A well-formed XHTML page no longer reads as an empty
events folder, and a leftover .tmp upload is no longer reported on
every pull. Two tests with a server that goes silent after the headers
keep the read limit honest; they pass on ureq 2 too, so the old
four-minute hang stays open in OPEN-ISSUES.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_012WGZUPYHL7TurMz6YSFVJs
```
