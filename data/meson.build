# Desktop entry
desktop_file = i18n.merge_file(
  input: 'io.github.janekbt.Meditate.desktop.in',
  output: 'io.github.janekbt.Meditate.desktop',
  type: 'desktop',
  po_dir: '../po',
  install: true,
  install_dir: get_option('datadir') / 'applications',
)

desktop_utils = find_program('desktop-file-validate', required: false)
if desktop_utils.found()
  test('Validate desktop file', desktop_utils, args: [desktop_file])
endif

# D-Bus service file — matches DBusActivatable=true in the .desktop.
# Without this, flatpak-builder refuses to export the app.
service_conf = configuration_data()
service_conf.set('APP_ID', 'io.github.janekbt.Meditate')
service_conf.set('APP_NAME', 'meditate')
service_conf.set('BINDIR', get_option('prefix') / get_option('bindir'))

configure_file(
  input: 'io.github.janekbt.Meditate.service.in',
  output: 'io.github.janekbt.Meditate.service',
  configuration: service_conf,
  install: true,
  install_dir: get_option('datadir') / 'dbus-1' / 'services',
)

# AppStream metainfo
appstream_file = i18n.merge_file(
  input: 'io.github.janekbt.Meditate.metainfo.xml.in',
  output: 'io.github.janekbt.Meditate.metainfo.xml',
  po_dir: '../po',
  install: true,
  install_dir: get_option('datadir') / 'metainfo',
)

appstream_util = find_program('appstream-util', required: false)
if appstream_util.found()
  test('Validate metainfo', appstream_util, args: ['validate', appstream_file])
endif

# GSettings schema — drives window-size persistence via Gio.Settings.
# `compile_schemas` makes `gschemas.compiled` available in build/data so
# dev runs can `GSETTINGS_SCHEMA_DIR=build/data ./build/src/meditate`.
install_data(
  'io.github.janekbt.Meditate.gschema.xml',
  install_dir: get_option('datadir') / 'glib-2.0' / 'schemas',
)

gnome.compile_schemas(
  build_by_default: true,
  depend_files: 'io.github.janekbt.Meditate.gschema.xml',
)

glib_compile_schemas = find_program('glib-compile-schemas', required: false)
if glib_compile_schemas.found()
  test('Validate GSchema',
    glib_compile_schemas,
    args: ['--strict', '--dry-run', meson.current_source_dir()])
endif

# Icons
install_data(
  'icons/hicolor/scalable/apps/io.github.janekbt.Meditate.svg',
  install_dir: get_option('datadir') / 'icons' / 'hicolor' / 'scalable' / 'apps',
)
install_data(
  'icons/hicolor/symbolic/apps/io.github.janekbt.Meditate-symbolic.svg',
  install_dir: get_option('datadir') / 'icons' / 'hicolor' / 'symbolic' / 'apps',
)
