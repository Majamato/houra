// Run with: gjs -m shell-extension/tests/activeTimer.test.js
// Needs a session bus: GApplication computes its object path on registration.
import Gio from 'gi://Gio';
import System from 'system';

import {APP_ID, BUS_NAME, OBJECT_PATH} from '../activeTimer.js';

// NON_UNIQUE registers without claiming the name from a running Houra.
const application = new Gio.Application({
    application_id: APP_ID,
    flags: Gio.ApplicationFlags.NON_UNIQUE,
});
application.register(null);
if (application.get_dbus_object_path() === null) {
    printerr('no session bus; run under dbus-run-session');
    System.exit(1);
}

const cases = [
    [BUS_NAME, APP_ID],
    [OBJECT_PATH, application.get_dbus_object_path()],
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
print(`${cases.length} active timer checks passed`);
