// Build-selected extension identity. The stable values below match the stable
// object in the variant manifest; scripts/prepare-dev.py generates the
// equivalent module with the development values. Imports stay empty so
// headless tests can load this module outside GNOME Shell.

export const APP_ID = 'io.github.majamato.Houra';
export const APP_NAME = 'Houra';
export const INDICATOR_GTYPE_NAME = 'Gjs_HouraIndicator';
export const STYLE_PREFIX = 'houra';

/** The Shell CSS class for an element part, e.g. `houra-indicator`. */
export function styleClass(suffix) {
    return `${STYLE_PREFIX}-${suffix}`;
}
