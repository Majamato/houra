// Clock formatting for the top-bar element. Pure functions, no GNOME imports.

const MINUTE_MS = 60_000;

/** English templates; the indicator passes the translated ones, which are
 *  shared with the app's own durations. */
export const ENGLISH_TEMPLATES = {
    minutes: '{minutes}m',
    hoursMinutes: '{hours}h {minutes}m',
};

/**
 * Splits elapsed milliseconds into the parts of a duration like 45m or
 * 1h 05m. Only minutes show under an hour; seconds never show, so the top
 * bar only changes once a minute.
 *
 * Literal template text becomes `unit` parts, except space that leads into
 * the next number, which becomes a `gap` so groups read apart ("1h 30m")
 * while a space before a unit ("1 h") stays with the unit.
 *
 * @param {number} ms elapsed milliseconds
 * @param {{minutes: string, hoursMinutes: string}} templates
 * @returns {{kind: 'number'|'unit'|'gap', text: string}[]}
 */
export function elapsedParts(ms, templates = ENGLISH_TEMPLATES) {
    const totalMinutes = Math.max(0, Math.floor(ms / MINUTE_MS));
    const hours = Math.floor(totalMinutes / 60);
    const minutes = totalMinutes % 60;
    const [template, values] = hours === 0
        ? [templates.minutes, {minutes: String(minutes)}]
        : [templates.hoursMinutes,
            {hours: String(hours), minutes: String(minutes).padStart(2, '0')}];

    const parts = [];
    const push = (kind, text) => {
        if (text)
            parts.push({kind, text});
    };
    const placeholder = /\{([a-z_]+)\}/g;
    let start = 0;
    for (const match of template.matchAll(placeholder)) {
        if (!(match[1] in values))
            continue;
        const literal = template.slice(start, match.index);
        const unit = literal.trimEnd();
        push(unit.trim() ? 'unit' : 'gap', unit);
        push('gap', literal.slice(unit.length));
        push('number', values[match[1]]);
        start = match.index + match[0].length;
    }
    const tail = template.slice(start);
    push(tail.trim() ? 'unit' : 'gap', tail);
    return parts;
}

/**
 * Formats elapsed milliseconds as plain text, e.g. 0m, 45m or 12h 00m.
 *
 * @param {number} ms elapsed milliseconds
 * @param {{minutes: string, hoursMinutes: string}} templates
 * @returns {string}
 */
export function formatElapsed(ms, templates = ENGLISH_TEMPLATES) {
    return elapsedParts(ms, templates).map(part => part.text).join('');
}

/**
 * Delay until the displayed minute changes, so the clock flips on time.
 *
 * @param {number} ms elapsed milliseconds
 * @returns {number} milliseconds in 1..60000
 */
export function millisecondsUntilNextMinute(ms) {
    return MINUTE_MS - (Math.max(0, Math.floor(ms)) % MINUTE_MS);
}
