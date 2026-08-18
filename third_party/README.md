# third_party

## i-slint-compiler

An unmodified copy of the crates.io release `i-slint-compiler 1.16.1`, with a
single change in `translations.rs`: the `lang/` directory entries are sorted
before use.

Upstream reads that directory unsorted, so the bundled language order — and
with it the whole generated string table — follows whatever order the
filesystem returns. Two machines then produce different binaries from
identical sources, which is why F-Droid's rebuild could not be verified
against our published APK.

The same fix is proposed upstream in slint-ui/slint#12932. Delete this
directory and the `[patch.crates-io]` entry in the workspace `Cargo.toml`
once a Slint release carries it.

Only the build-time code generator is patched. It runs on the build host and
its output is generated source, so nothing from here is linked into the
shipped library.

### Refreshing after a Slint upgrade

    cp -r ~/.cargo/registry/src/*/i-slint-compiler-<version> third_party/i-slint-compiler
    rm -f third_party/i-slint-compiler/.cargo-checksum.json \
          third_party/i-slint-compiler/.cargo-ok \
          third_party/i-slint-compiler/Cargo.lock

then reapply the sort in `translations.rs` and regenerate
`build-aux/cargo-sources.json`.
