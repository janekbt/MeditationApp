cargo_bin = find_program('cargo')

if get_option('buildtype') == 'release'
  rust_target = 'release'
  cargo_profile_args = ['--release']
else
  rust_target = 'debug'
  cargo_profile_args = []
endif

cargo_env = {
  'APP_ID': application_id,
  'PKGDATADIR': get_option('prefix') / get_option('datadir') / meson.project_name(),
  # CARGO_HOME is intentionally absent here. In Flatpak builds the
  # build-options.env in the manifest sets it to the vendored-crates
  # directory populated by cargo-sources.json. In dev builds,
  # cargo-build.py falls back to <build_root>/cargo-home.
}

cargo_build = custom_target(
  'cargo-build',
  build_by_default: true,
  build_always_stale: true,
  output: meson.project_name(),
  console: true,
  install: true,
  install_dir: get_option('bindir'),
  command: [
    find_program('python3'),
    meson.project_source_root() / 'build-aux' / 'cargo-build.py',
    cargo_bin.full_path(),
    meson.project_source_root() / 'Cargo.toml',
    meson.project_build_root() / 'target',
    rust_target,
    '@OUTPUT@',
  ],
  env: cargo_env,
)
