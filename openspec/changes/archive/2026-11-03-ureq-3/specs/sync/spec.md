## ADDED Requirements

### Requirement: A server that goes silent mid-request cannot hold a sync
When the server stops sending in the middle of an answer, the request SHALL end with a network error once one read has waited the read limit, on a fresh connection and on one reused from an earlier request of the same sync.

#### Scenario: The server sends the headers and then nothing
- **WHEN** a request's answer headers arrive and the body never does
- **THEN** the request ends with a network error after the read limit
- **TEST** `meditate-core/src/sync/webdav.rs::a_server_silent_after_the_headers_ends_the_request`

#### Scenario: The server goes silent on a reused connection
- **WHEN** a first request is answered in full and the second one on the same connection gets only its headers
- **THEN** the second request ends with a network error after the read limit
- **TEST** `meditate-core/src/sync/webdav.rs::a_server_silent_on_a_reused_connection_ends_the_request`

### Requirement: Server text stays out of the sync status
When the server answers with an error status, a redirect or a listing that is not WebDAV XML, the sync status SHALL name only the status, that it redirected or the parser's complaint, with at most a short cleaned excerpt of the page in the log.

#### Scenario: The server answers 500 with an error page
- **WHEN** a request gets a 500 whose page names the server's host
- **THEN** the sync error reads "server returned 500" without the page or the host
- **TEST** `meditate-core/src/sync/webdav.rs::a_server_error_shows_only_its_status`

#### Scenario: The server redirects elsewhere
- **WHEN** a request gets a redirect to an address with a user name and host
- **THEN** the sync error says the server redirected, without the address
- **TEST** `meditate-core/src/sync/webdav.rs::a_redirect_error_hides_its_target`

#### Scenario: The folder listing comes back as a web page
- **WHEN** the server answers the folder listing with success but a page instead of WebDAV XML (a proxy's login page naming the host)
- **THEN** the sync error names the parser's complaint only, without the page or the host
- **TEST** `meditate-core/src/sync/webdav.rs::a_listing_that_is_a_web_page_shows_no_page_text`

#### Scenario: The folder listing is a well-formed XHTML page
- **WHEN** the listing comes back as a page that is valid XML but not a WebDAV multistatus
- **THEN** the sync fails with a malformed response instead of reading an empty folder
- **TEST** `meditate-core/src/sync/webdav.rs::a_listing_that_is_an_xhtml_page_is_malformed`

#### Scenario: The error page is long or holds control characters
- **WHEN** the error page is longer than 512 characters or holds line breaks and escape characters
- **THEN** the logged excerpt is at most 512 characters with no control characters
- **TEST** `meditate-core/src/sync/webdav.rs::the_logged_error_excerpt_is_short_and_clean`

### Requirement: A leftover upload is not reported as unreadable
When the events folder on the server holds a `.tmp` file left by an interrupted upload, a pull SHALL skip it without reporting it, and still report any other file it cannot read.

#### Scenario: A leftover upload next to a stray file
- **WHEN** the events folder lists a batch, a stray file and a leftover `.json.tmp`
- **THEN** only the stray file is reported as unreadable
- **TEST** `meditate-core/src/sync/orchestrator.rs::a_leftover_upload_is_not_reported_as_unreadable`
