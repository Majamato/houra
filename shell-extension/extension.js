import Gio from 'gi://Gio';

import {Extension} from 'resource:///org/gnome/shell/extensions/extension.js';
import * as Main from 'resource:///org/gnome/shell/ui/main.js';

import {ACTIVE_TIMER_XML, BUS_NAME, OBJECT_PATH} from './activeTimer.js';
import {HouraIndicator} from './indicator.js';

export default class HouraExtension extends Extension {
    enable() {
        this._indicator = null;
        this._proxy = null;
        const cancellable = new Gio.Cancellable();
        this._cancellable = cancellable;

        const ActiveTimerProxy = Gio.DBusProxy.makeProxyWrapper(ACTIVE_TIMER_XML);
        // DO_NOT_AUTO_START: never launch Houra; only mirror a running Houra.
        ActiveTimerProxy.newAsync(Gio.DBus.session, BUS_NAME, OBJECT_PATH,
            cancellable, Gio.DBusProxyFlags.DO_NOT_AUTO_START)
            .then(proxy => {
                if (this._cancellable !== cancellable)
                    return; // disabled while connecting
                this._proxy = proxy;
                proxy.connectObject('notify::g-name-owner',
                    () => this._syncIndicator(), this);
                this._syncIndicator();
            })
            .catch(error => {
                if (!error.matches?.(Gio.IOErrorEnum, Gio.IOErrorEnum.CANCELLED))
                    console.error(`Houra: cannot watch the active timer: ${error.message}`);
            });
    }

    disable() {
        this._cancellable?.cancel();
        this._cancellable = null;
        this._proxy?.disconnectObject(this);
        this._proxy = null;
        this._indicator?.destroy();
        this._indicator = null;
    }

    /** Shows the element exactly while Houra owns its bus name. */
    _syncIndicator() {
        const running = Boolean(this._proxy?.g_name_owner);
        if (running && !this._indicator) {
            this._indicator = new HouraIndicator(this, this._proxy);
            // Right box, first position: Dash to Panel shows only the right box.
            Main.panel.addToStatusArea(this.uuid, this._indicator, 0, 'right');
        } else if (!running && this._indicator) {
            this._indicator.destroy();
            this._indicator = null;
        }
    }
}
