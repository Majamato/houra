import Atk from 'gi://Atk';
import Clutter from 'gi://Clutter';
import Gio from 'gi://Gio';
import GLib from 'gi://GLib';
import GObject from 'gi://GObject';
import Shell from 'gi://Shell';
import St from 'gi://St';

import {gettext as _} from 'resource:///org/gnome/shell/extensions/extension.js';
import * as Main from 'resource:///org/gnome/shell/ui/main.js';
import * as PanelMenu from 'resource:///org/gnome/shell/ui/panelMenu.js';

import {APP_ID, BUS_NAME, OBJECT_PATH} from './activeTimer.js';
import {elapsedParts, millisecondsUntilNextMinute} from './format.js';
import {APP_NAME, INDICATOR_GTYPE_NAME, styleClass} from './identity.js';

const STATES = new Set(['stopped', 'running', 'paused', 'idle', 'recovery']);
const TICKING = new Set(['running', 'idle']);
const SHOWS_TIME = new Set(['running', 'paused', 'idle']);
const SHOWS_TOGGLE = new Set(['running', 'paused']);
const NEEDS_REVIEW = new Set(['idle', 'recovery']);
const PAUSED_OPACITY = 140; // ≈55%, like the window's .timer-paused
const FADE_MS = 180;
// Units sit a step smaller and softer than the numerals, but keep the panel's
// bold weight: at top-bar size a thin, faded unit all but disappears.
const UNIT_OPEN = '<span size="85%" alpha="85%">';

function isHouraWindow(window) {
    return window.get_gtk_application_id() === APP_ID ||
        window.get_wm_class() === APP_ID;
}

/** Brings Houra's window to the front; does nothing when it already is. */
function openHoura() {
    const focused = global.display.focus_window;
    if (!Main.overview.visible && focused && isHouraWindow(focused))
        return;
    Main.overview.hide();

    // Shell-side activation focuses the window without an activation token,
    // switching workspace and unminimizing as needed.
    const app = Shell.AppSystem.get_default().lookup_app(`${APP_ID}.desktop`);
    if (app) {
        app.activate();
        return;
    }
    // Development builds run without an installed launcher.
    const window = global.get_window_actors()
        .map(actor => actor.meta_window)
        .find(isHouraWindow);
    if (window) {
        Main.activateWindow(window);
        return;
    }
    Gio.DBus.session.call(BUS_NAME, OBJECT_PATH, 'org.freedesktop.Application',
        'Activate', new GLib.Variant('(a{sv})', [{}]), null,
        Gio.DBusCallFlags.NO_AUTO_START, -1, null, null);
}

export const HouraIndicator = GObject.registerClass({
    GTypeName: INDICATOR_GTYPE_NAME,
}, class HouraIndicator extends PanelMenu.Button {
    _init(extension, proxy) {
        super._init(0.5, APP_NAME, true); // true: no menu; we handle clicks
        this.accessible_role = Atk.Role.PUSH_BUTTON;
        this.add_style_class_name(styleClass('indicator'));

        this._proxy = proxy;
        this._state = 'stopped';
        this._elapsedMs = 0;
        this._anchorUs = 0;
        this._tickId = 0;

        const box = new St.BoxLayout({style_class: styleClass('box')});
        this._glyph = new St.Icon({
            gicon: Gio.icon_new_for_string(`${extension.path}/icons/houra-symbolic.svg`),
            fallback_icon_name: 'alarm-symbolic',
            style_class: `system-status-icon ${styleClass('glyph')}`,
            y_align: Clutter.ActorAlign.CENTER,
        });
        this._time = new St.Label({
            style_class: styleClass('time'),
            y_align: Clutter.ActorAlign.CENTER,
            visible: false,
        });
        this._toggleIcon = new St.Icon({
            icon_name: 'media-playback-pause-symbolic',
            style_class: styleClass('toggle-icon'),
        });
        this._toggle = new St.Button({
            style_class: styleClass('toggle'),
            can_focus: true,
            y_align: Clutter.ActorAlign.CENTER,
            visible: false,
            child: this._toggleIcon,
        });
        this._toggle.connectObject('clicked', () => this._togglePause(), this);
        box.add_child(this._glyph);
        box.add_child(this._time);
        box.add_child(this._toggle);
        this.add_child(box);

        // Same pattern as the Shell's ActivitiesButton and PopupBaseMenuItem:
        // recognize on release, so the inner St.Button claims its own clicks.
        this._clickGesture = new Clutter.ClickGesture({
            required_button: Clutter.BUTTON_PRIMARY,
        });
        this._clickGesture.connectObject(
            // The toggle owns presses on it. Checking the press position (not
            // hover) also works for touch, which has no hover.
            'may-recognize', () => !this._gestureOnToggle(),
            'recognize', () => this._onClicked(),
            'notify::pressed', () => this._syncPressed(),
            this);
        this.add_action(this._clickGesture);

        proxy.connectObject('g-properties-changed', () => this._sync(), this);
        this.connect('destroy', () => this._stopTicking());
        this._sync();
    }

    vfunc_key_release_event(event) {
        const symbol = event.get_key_symbol();
        const activates = symbol === Clutter.KEY_Return ||
            symbol === Clutter.KEY_KP_Enter || symbol === Clutter.KEY_space;
        if (activates && !this._toggle.has_key_focus()) {
            this._onClicked();
            return Clutter.EVENT_STOP;
        }
        return Clutter.EVENT_PROPAGATE;
    }

    /** Whether the current press of the pill's gesture lands on the toggle. */
    _gestureOnToggle() {
        if (!this._toggle.visible)
            return false;
        const {x, y} = this._clickGesture.get_coords_abs();
        const target = global.stage.get_actor_at_pos(Clutter.PickMode.REACTIVE, x, y);
        return this._toggle.contains(target);
    }

    _onClicked() {
        openHoura();
        if (NEEDS_REVIEW.has(this._state))
            this._proxy.OpenReviewAsync().catch(logDBusError);
    }

    _togglePause() {
        this._proxy.TogglePauseAsync().catch(logDBusError);
    }

    _syncPressed() {
        if (this._clickGesture.pressed && !this._gestureOnToggle())
            this.add_style_pseudo_class('active');
        else
            this.remove_style_pseudo_class('active');
    }

    _sync() {
        const state = this._proxy.State;
        this._state = STATES.has(state) ? state : 'stopped';
        this._elapsedMs = Number(this._proxy.ElapsedMs ?? 0);
        this._anchorUs = GLib.get_monotonic_time();
        const work = this._proxy.Summary || APP_NAME;

        this._setClass(styleClass('active'), this._state !== 'stopped');
        this._setClass(styleClass('running'), this._state === 'running');
        this._setClass(styleClass('paused'), this._state === 'paused');
        this._setClass(styleClass('attention'), NEEDS_REVIEW.has(this._state));

        const paused = this._state === 'paused';
        this._toggleIcon.icon_name = paused
            ? 'media-playback-start-symbolic' : 'media-playback-pause-symbolic';
        this._toggle.accessible_name = paused ? _('Resume timer') : _('Pause timer');
        this.accessible_name = accessibleName(this._state, work);

        this._reveal(this._time, SHOWS_TIME.has(this._state));
        this._reveal(this._toggle, SHOWS_TOGGLE.has(this._state));
        for (const actor of [this._glyph, this._time]) {
            actor.ease({
                opacity: paused ? PAUSED_OPACITY : 255,
                duration: FADE_MS,
                mode: Clutter.AnimationMode.EASE_OUT_QUAD,
            });
        }
        this._updateTime();
        this._scheduleTick();
    }

    _setClass(name, enabled) {
        if (enabled)
            this.add_style_class_name(name);
        else
            this.remove_style_class_name(name);
    }

    /** Fades an actor in or out; hidden actors take no space. */
    _reveal(actor, shown) {
        if (shown === actor.visible)
            return;
        actor.remove_all_transitions();
        if (shown) {
            actor.opacity = 0;
            actor.show();
            actor.ease({
                opacity: 255,
                duration: FADE_MS,
                mode: Clutter.AnimationMode.EASE_OUT_QUAD,
            });
        } else {
            actor.hide();
        }
    }

    _currentElapsedMs() {
        if (!TICKING.has(this._state))
            return this._elapsedMs;
        const sinceAnchorMs = Math.floor((GLib.get_monotonic_time() - this._anchorUs) / 1000);
        return this._elapsedMs + Math.max(0, sinceAnchorMs);
    }

    _updateTime() {
        // Same msgids as the app's durations, so both read alike in any language.
        const templates = {
            minutes: _('{minutes}m'),
            hoursMinutes: _('{hours}h {minutes}m'),
        };
        const markup = elapsedParts(this._currentElapsedMs(), templates)
            .map(({kind, text}) => {
                const escaped = GLib.markup_escape_text(text, -1);
                return kind === 'unit' ? `${UNIT_OPEN}${escaped}</span>` : escaped;
            })
            .join('');
        this._time.clutter_text.set_markup(markup);
    }

    _scheduleTick() {
        this._stopTicking();
        if (!TICKING.has(this._state))
            return;
        const delay = millisecondsUntilNextMinute(this._currentElapsedMs());
        this._tickId = GLib.timeout_add(GLib.PRIORITY_DEFAULT, delay, () => {
            this._tickId = 0;
            this._updateTime();
            this._scheduleTick();
            return GLib.SOURCE_REMOVE;
        });
        GLib.Source.set_name_by_id(this._tickId, '[houra] clock tick');
    }

    _stopTicking() {
        if (this._tickId) {
            GLib.source_remove(this._tickId);
            this._tickId = 0;
        }
    }
});

function accessibleName(state, work) {
    // A function replacement keeps `$` sequences in the user's note literal.
    switch (state) {
    case 'running':
        return _('Tracking: {work}').replace('{work}', () => work);
    case 'paused':
        return _('Paused: {work}').replace('{work}', () => work);
    case 'idle':
        return _('Idle time needs review');
    case 'recovery':
        return _('Interrupted timer needs review');
    default:
        // The stable label keeps its translation; development swaps in its
        // own brand. A function replacement keeps the brand literal.
        return _('Open Houra').replace('Houra', () => APP_NAME);
    }
}

function logDBusError(error) {
    console.error(`${APP_NAME}: ${error.message}`);
}
