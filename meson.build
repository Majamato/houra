project(
  'houra',
  'rust',
  version: '0.1.0',
  license: 'GPL-3.0-or-later',
  meson_version: '>= 1.3.0',
)

gnome = import('gnome')
i18n = import('i18n')

app_id = 'io.github.majamato.Houra'

cargo = find_program('cargo')
glib_compile_schemas = find_program('glib-compile-schemas')

gnome.compile_resources(
  'houra-resources',
  'data/' + app_id + '.gresource.xml',
  source_dir: 'data',
  gresource_bundle: true,
  build_by_default: true,
)

cargo_args = ['--locked', '--features', 'native-ui']
if get_option('offline')
  cargo_args += '--offline'
endif

rust_binary = custom_target(
  'houra-rust',
  input: ['Cargo.toml', 'Cargo.lock'],
  output: 'houra',
  command: [
    find_program('bash'),
    files('build-aux/cargo-build.sh'),
    meson.project_source_root(),
    meson.project_build_root(),
    '@OUTPUT@',
  ] + cargo_args,
  build_by_default: true,
  build_always_stale: true,
  install: true,
  install_dir: get_option('bindir'),
)

desktop = i18n.merge_file(
  input: 'data/' + app_id + '.desktop.in',
  output: app_id + '.desktop',
  po_dir: 'po',
  type: 'desktop',
  install: true,
  install_dir: get_option('datadir') / 'applications',
)

metainfo = i18n.merge_file(
  input: 'data/' + app_id + '.metainfo.xml.in',
  output: app_id + '.metainfo.xml',
  po_dir: 'po',
  type: 'xml',
  install: true,
  install_dir: get_option('datadir') / 'metainfo',
)

install_data(
  'data/' + app_id + '.gschema.xml',
  install_dir: get_option('datadir') / 'glib-2.0/schemas',
)

install_data(
  'data/icons/hicolor/scalable/apps/' + app_id + '.svg',
  install_dir: get_option('datadir') / 'icons/hicolor/scalable/apps',
)
install_data(
  'data/icons/hicolor/symbolic/apps/' + app_id + '-symbolic.svg',
  install_dir: get_option('datadir') / 'icons/hicolor/symbolic/apps',
)

install_data('LICENSE', install_dir: get_option('datadir') / 'licenses/houra')

subdir('po')

meson.add_install_script('build-aux/post-install.py')
