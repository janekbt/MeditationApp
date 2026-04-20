i18n.gettext(
  meson.project_name(),
  preset: 'glib',
  args: [
    '--from-code=UTF-8',
    # xgettext doesn't know .rs / .blp natively; force a C-like lexer on
    # both so string literals parse cleanly (without this, single-quote
    # Rust lifetimes and Blueprint braces trigger "unterminated" noise).
    '--language=C',
    '--keyword=_',
    '--keyword=gettext',
    '--keyword=C_:1c,2',
    '--keyword=NC_:1c,2',
  ],
)
