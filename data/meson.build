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

# Bundled sounds
install_data(
  'sounds/bowl.wav',
  'sounds/bell.wav',
  'sounds/gong.wav',
  install_dir: get_option('prefix') / get_option('datadir') / meson.project_name() / 'sounds',
)
