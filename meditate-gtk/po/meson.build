i18n.gettext(
  meson.project_name(),
  preset: 'glib',
  args: [
    '--from-code=UTF-8',
    # No --language override — xgettext needs to auto-detect so it parses
    # shortcuts.ui as GtkBuilder (not C) and picks up `translatable="yes"`
    # properties. Forcing --language=C here used to mute Rust-lifetime
    # "unterminated character constant" warnings, but it also dropped the
    # shortcuts dialog from translation. The warnings are cosmetic; POT
    # generation still completes.
    '--keyword=_',
    '--keyword=gettext',
    '--keyword=C_:1c,2',
    '--keyword=NC_:1c,2',
  ],
)
