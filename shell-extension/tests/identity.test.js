// Run with: gjs -m shell-extension/tests/identity.test.js <dev-extension-dir>
// Compares the stable extension identity with the generated dev identity:
// bus names, object paths, GObject type names, CSS classes, and metadata.
// The dev directory comes from scripts/prepare-dev.py. Needs no session bus.
import Gio from 'gi://Gio';
import GObject from 'gi://GObject';
import System from 'system';

if (!ARGV[0]) {
    printerr('usage: gjs -m shell-extension/tests/identity.test.js <dev-extension-dir>');
    System.exit(2);
}

function readFile(path) {
    const file = Gio.File.new_for_path(path);
    const [ok, contents] = file.load_contents(null);
    if (!ok)
        throw new Error(`cannot read ${path}`);
    return new TextDecoder().decode(contents);
}

const stableFile = Gio.File.new_for_uri(import.meta.url).get_parent().get_parent();
const stableUrl = stableFile.get_uri();
const stableDir = stableFile.get_path();
const devFile = Gio.File.new_for_commandline_arg(ARGV[0]);
const devUrl = devFile.get_uri();
const devDir = devFile.get_path();

const stable = await import(`${stableUrl}/identity.js`);
const dev = await import(`${devUrl}/identity.js`);
const stableTimer = await import(`${stableUrl}/activeTimer.js`);
const devTimer = await import(`${devUrl}/activeTimer.js`);

const manifest = JSON.parse(readFile(`${stableDir}/../data/app-variants.json`));
const publishedXml = readFile(`${stableDir}/../data/dbus/io.github.majamato.Houra.ActiveTimer.xml`).trim();
const stableMetadata = JSON.parse(readFile(`${stableDir}/metadata.json`));
const devMetadata = JSON.parse(readFile(`${devDir}/metadata.json`));

const failures = [];
function check(actual, expected, label) {
    if (actual !== expected)
        failures.push(`${label}: got ${actual}, expected ${expected}`);
}

// Both identities match the variant manifest.
for (const [identity, variant] of [[stable, 'stable'], [dev, 'devel']]) {
    const values = manifest[variant];
    check(identity.APP_ID, values.app_id, `${variant} APP_ID`);
    check(identity.APP_NAME, values.app_name, `${variant} APP_NAME`);
    check(identity.INDICATOR_GTYPE_NAME, values.extension_gtype_name,
        `${variant} INDICATOR_GTYPE_NAME`);
    check(identity.STYLE_PREFIX, values.extension_style_prefix,
        `${variant} STYLE_PREFIX`);
    check(identity.styleClass('indicator'), `${values.extension_style_prefix}-indicator`,
        `${variant} styleClass`);
}

// Both variants derive their bus names and object paths from their app IDs.
for (const [timer, variant] of [[stableTimer, 'stable'], [devTimer, 'devel']]) {
    const appId = manifest[variant].app_id;
    check(timer.APP_ID, appId, `${variant} timer APP_ID`);
    check(timer.BUS_NAME, appId, `${variant} BUS_NAME`);
    check(timer.OBJECT_PATH, `/${appId.replaceAll('.', '/')}`, `${variant} OBJECT_PATH`);
    check(timer.ACTIVE_TIMER_XML.trim(), publishedXml, `${variant} ACTIVE_TIMER_XML`);
}

// The GObject type names differ, so both extensions share one Shell process.
if (stable.INDICATOR_GTYPE_NAME === dev.INDICATOR_GTYPE_NAME)
    failures.push('both variants share one GObject type name');
const StableIndicator = GObject.registerClass({
    GTypeName: stable.INDICATOR_GTYPE_NAME,
}, class StableIndicator extends GObject.Object {});
const DevIndicator = GObject.registerClass({
    GTypeName: dev.INDICATOR_GTYPE_NAME,
}, class DevIndicator extends GObject.Object {});
check(StableIndicator.$gtype.name, stable.INDICATOR_GTYPE_NAME, 'stable GType');
check(DevIndicator.$gtype.name, dev.INDICATOR_GTYPE_NAME, 'dev GType');

// The stylesheets target their own variant's classes.
for (const name of ['stylesheet-dark.css', 'stylesheet-light.css']) {
    const stableCss = readFile(`${stableDir}/${name}`);
    const devCss = readFile(`${devDir}/${name}`);
    if (!stableCss.includes('.houra-indicator'))
        failures.push(`stable ${name} lost its Houra selectors`);
    if (!devCss.includes('.houra-dev-indicator'))
        failures.push(`dev ${name} misses its Houra Dev selectors`);
    if (/\.houra-(?!dev-)/.test(devCss))
        failures.push(`dev ${name} still targets Houra classes`);
}

// The dev metadata identifies the development timer and keeps the
// supported Shell versions and repository URL.
check(devMetadata.uuid, manifest.devel.extension_uuid, 'dev uuid');
check(devMetadata.name, manifest.devel.app_name, 'dev name');
check(devMetadata['gettext-domain'], manifest.devel.extension_gettext_domain,
    'dev gettext-domain');
check(JSON.stringify(devMetadata['shell-version']),
    JSON.stringify(stableMetadata['shell-version']), 'dev shell-version');
check(devMetadata.url, stableMetadata.url, 'dev url');
if (!devMetadata.description.includes('Houra Dev'))
    failures.push('dev description does not identify the development timer');

if (failures.length > 0) {
    for (const failure of failures)
        printerr(failure);
    System.exit(1);
}
print(`identity checks passed for ${stable.APP_ID} and ${dev.APP_ID}`);
