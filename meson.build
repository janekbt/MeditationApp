project('meditate',
  version: '26.4.1',
  meson_version: '>= 0.62.0',
  default_options: ['warning_level=2', 'werror=false'],
)

i18n = import('i18n')
gnome = import('gnome')

application_id = 'io.github.janekbt.Meditate'

subdir('data')
subdir('src')

gnome.post_install(
  gtk_update_icon_cache: true,
  update_desktop_database: true,
)
