// Run with: gjs -m shell-extension/tests/format.test.js
import System from 'system';

import {elapsedParts, formatElapsed, millisecondsUntilNextMinute} from '../format.js';

const spanish = {minutes: '{minutes} min', hoursMinutes: '{hours} h {minutes} min'};
const japanese = {minutes: '{minutes}分', hoursMinutes: '{hours}時間{minutes}分'};
const shape = parts => parts.map(({kind, text}) => `${kind}:${text}`).join('|');

const cases = [
    [formatElapsed(0), '0m'],
    [formatElapsed(-5), '0m'],
    [formatElapsed(59_999), '0m'],
    [formatElapsed(60_000), '1m'],
    [formatElapsed(2_700_000), '45m'],
    [formatElapsed(3_599_999), '59m'],
    [formatElapsed(3_600_000), '1h 00m'],
    [formatElapsed(5_025_000), '1h 23m'],
    [formatElapsed(360_000_000), '100h 00m'],
    [formatElapsed(5_025_000, spanish), '1 h 23 min'],
    [formatElapsed(300_000, japanese), '5分'],
    [shape(elapsedParts(2_700_000)), 'number:45|unit:m'],
    [shape(elapsedParts(5_025_000)), 'number:1|unit:h|gap: |number:23|unit:m'],
    [shape(elapsedParts(5_025_000, spanish)),
        'number:1|unit: h|gap: |number:23|unit: min'],
    [shape(elapsedParts(3_900_000, japanese)), 'number:1|unit:時間|number:05|unit:分'],
    [millisecondsUntilNextMinute(0), 60_000],
    [millisecondsUntilNextMinute(1), 59_999],
    [millisecondsUntilNextMinute(59_999), 1],
    [millisecondsUntilNextMinute(90_500), 29_500],
    [millisecondsUntilNextMinute(-5), 60_000],
];
let failures = 0;
for (const [index, [actual, expected]] of cases.entries()) {
    if (actual !== expected) {
        printerr(`case ${index}: got ${actual}, expected ${expected}`);
        failures++;
    }
}
if (failures > 0)
    System.exit(1);
print(`${cases.length} format checks passed`);
