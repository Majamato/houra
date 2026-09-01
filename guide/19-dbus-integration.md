# Chapter 19 — D-Bus

**Goal.** GNOME tells the app when the user goes idle and comes back, when
the screen locks and unlocks, and when the machine is about to sleep. Each
event becomes a `TrackerCommand`. A banner warns when the integrations are
unavailable. After this chapter every Rust file matches the original.
Files: `crates/app/src/native/platform.rs`, `crates/app/src/native/mod.rs`,
`crates/app/src/native/window.rs`, `data/ui/window.ui`.

**You will learn**

- D-Bus vocabulary: bus, name, object path, interface, method, signal,
  and how to explore them with `busctl`.
- `gio::DBusProxy`, GVariant ↔ Rust tuples, and keeping proxies alive.
- `AtomicU32` with `Ordering::Relaxed`; `OwnedFd` and `Drop`.
- Desktop notifications through `gio::Notification`.
- Optional adapters that fail quietly by design.

**Prerequisite.** Chapter 18 checkpoint passed; a GNOME session (Mutter
and the screensaver service on the session bus, logind on the system
bus).

---

## 19.0 Look at the services first

```sh
busctl --user introspect org.gnome.Mutter.IdleMonitor /org/gnome/Mutter/IdleMonitor/Core
busctl --user introspect org.gnome.ScreenSaver /org/gnome/ScreenSaver
busctl introspect org.freedesktop.login1 /org/freedesktop/login1 org.freedesktop.login1.Manager
```

The lines the code uses:

```
.AddIdleWatch                       method    t         u            -
.AddUserActiveWatch                 method    -         u            -
.GetIdletime                        method    -         t            -
.RemoveWatch                        method    u         -            -
.WatchFired                         signal    u         -            -

.GetActive                          method    -         b            -
.ActiveChanged                      signal    b         -            -

.Inhibit                          method   ssss                h
.PrepareForSleep                  signal   b                   -
```

**Linux — D-Bus.** The desktop's message bus. A *service* owns a *name*
(`org.gnome.Mutter.IdleMonitor`), exposes *objects* at *paths*
(`/org/gnome/Mutter/IdleMonitor/Core`), each implementing *interfaces*
with *methods* (call, get a reply) and *signals* (broadcast, subscribe).
Types are single letters: `t` = u64, `u` = u32, `b` = bool, `s` = string,
`h` = a file-descriptor index. The *session* bus (`--user`) belongs to your
login; the *system* bus carries logind. Try
`busctl --user call org.gnome.Mutter.IdleMonitor /org/gnome/Mutter/IdleMonitor/Core org.gnome.Mutter.IdleMonitor GetIdletime`
— it answers `t <milliseconds>` since your last input.

## 19.1 The three adapters

```rust
// crates/app/src/native/platform.rs
//! GNOME D-Bus adapters.
//!
//! Watches are owned for the lifetime of their proxies. GDBus automatically
//! follows name-owner changes, so a late GNOME Shell startup or shell restart
//! does not disable manual tracking; the next reconnect reinstalls watches.

use gio::prelude::*;
use glib::variant::ToVariant;
use std::os::fd::OwnedFd;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use work_time_core::TrackerCommand;

use crate::{AppError, TrackerHandle};

/// Connects the three GNOME/systemd event sources. Each is independent: a
/// missing service disables only itself.
pub fn start_integrations(
    handle: TrackerHandle,
    idle_threshold_minutes: u32,
    notifications: bool,
) -> Result<(), AppError> {
    start_screensaver(handle.clone(), notifications);
    start_logind(handle.clone(), notifications);
    start_mutter(handle, idle_threshold_minutes, notifications)
}
```

**What.** The entry point called from `install_actions`. Screensaver and
logind adapters return nothing — if the service is missing they silently
do nothing. Mutter's idle monitor returns a `Result`, because without it
automatic idle detection is off and the user should see a banner.

**Why events, not owners.** These services *produce events*; the engine
remains the only owner of the timer. A late Shell start, a missing
service, or a reconnect can never corrupt manual tracking — at worst, a
signal is missed.

## 19.2 Mutter's idle monitor

```rust
// crates/app/src/native/platform.rs
// ...

fn start_mutter(
    handle: TrackerHandle,
    idle_threshold_minutes: u32,
    notifications: bool,
) -> Result<(), AppError> {
    let proxy = gio::DBusProxy::for_bus_sync(
        gio::BusType::Session,
        gio::DBusProxyFlags::DO_NOT_AUTO_START,
        None::<&gio::DBusInterfaceInfo>,
        "org.gnome.Mutter.IdleMonitor",
        "/org/gnome/Mutter/IdleMonitor/Core",
        "org.gnome.Mutter.IdleMonitor",
        None::<&gio::Cancellable>,
    )
    .map_err(|error| AppError::InvalidBackup(format!("Mutter IdleMonitor: {error}")))?;

    let threshold_ms = u64::from(idle_threshold_minutes.clamp(1, 120)) * 60 * 1_000;
    let idle_watch = Arc::new(AtomicU32::new(add_idle_watch(&proxy, threshold_ms)?));
    let active_watch = Arc::new(AtomicU32::new(0));

    proxy.connect_g_signal({
        let proxy = proxy.clone();
        let idle_watch = Arc::clone(&idle_watch);
        let active_watch = Arc::clone(&active_watch);
        move |_, _, signal, parameters| {
            if signal != "WatchFired" {
                return;
            }
            let Some((watch_id,)) = parameters.get::<(u32,)>() else {
                return;
            };
            let now = chrono::Utc::now().timestamp_millis();
            if watch_id == idle_watch.load(Ordering::Relaxed) {
                idle_watch.store(0, Ordering::Relaxed);
                let idle_start_ms =
                    now.saturating_sub(i64::try_from(threshold_ms).unwrap_or(i64::MAX));
                let _ignored = handle.apply(TrackerCommand::IdleDetected { idle_start_ms });
                if let Ok(id) = add_active_watch(&proxy) {
                    active_watch.store(id, Ordering::Relaxed);
                }
            } else if watch_id == active_watch.load(Ordering::Relaxed) {
                active_watch.store(0, Ordering::Relaxed);
                apply_return_and_notify(&handle, now, notifications);
                if let Ok(id) = add_idle_watch(&proxy, threshold_ms) {
                    idle_watch.store(id, Ordering::Relaxed);
                }
            }
        }
    });

    proxy.connect_g_name_owner_notify({
        let idle_watch = Arc::clone(&idle_watch);
        let active_watch = Arc::clone(&active_watch);
        move |proxy| {
            active_watch.store(0, Ordering::Relaxed);
            if proxy.g_name_owner().is_some() {
                if let Ok(id) = add_idle_watch(proxy, threshold_ms) {
                    idle_watch.store(id, Ordering::Relaxed);
                }
            } else {
                idle_watch.store(0, Ordering::Relaxed);
            }
        }
    });

    glib::timeout_add_seconds_local(30, move || {
        let _keep_alive = &proxy;
        glib::ControlFlow::Continue
    });
    Ok(())
}

fn add_idle_watch(proxy: &gio::DBusProxy, threshold_ms: u64) -> Result<u32, AppError> {
    let reply = proxy
        .call_sync(
            "AddIdleWatch",
            Some(&(threshold_ms,).to_variant()),
            gio::DBusCallFlags::NONE,
            -1,
            None::<&gio::Cancellable>,
        )
        .map_err(|error| AppError::InvalidBackup(format!("adding idle watch: {error}")))?;
    reply
        .get::<(u32,)>()
        .map(|value| value.0)
        .ok_or_else(|| AppError::InvalidBackup("Mutter returned an invalid idle watch".into()))
}

fn add_active_watch(proxy: &gio::DBusProxy) -> Result<u32, AppError> {
    let reply = proxy
        .call_sync(
            "AddUserActiveWatch",
            None,
            gio::DBusCallFlags::NONE,
            -1,
            None::<&gio::Cancellable>,
        )
        .map_err(|error| AppError::InvalidBackup(format!("adding active watch: {error}")))?;
    reply
        .get::<(u32,)>()
        .map(|value| value.0)
        .ok_or_else(|| AppError::InvalidBackup("Mutter returned an invalid active watch".into()))
}
```

**What.** Create a proxy to Mutter's idle monitor, ask it to fire once
after *threshold* milliseconds of inactivity (`AddIdleWatch`), and listen
for `WatchFired`. When the idle watch fires: the user has been away since
`now − threshold`, so send `IdleDetected` and install a *user-active* watch.
When that one fires: the user is back — send `UserReturned` (and a
notification), then re-arm the idle watch. If GNOME Shell restarts, the
watch ids die with it; the name-owner handler installs new ones.

**Why `now − threshold`.** Mutter fires *when* the threshold is reached,
so the idle period started one threshold ago. That is the `idle_start_ms`
the engine validates (Chapter 6).

**GTK — `DBusProxy::for_bus_sync`.** Bus type, flags (`DO_NOT_AUTO_START`:
do not launch the service if absent), optional interface info, name, path,
interface, optional cancellable. Synchronous is acceptable at start-up, once.
`call_sync(method, parameters, flags, timeout, cancellable)` sends a
method call; `connect_g_signal` subscribes to *all* signals from the
proxied object, so the closure filters on the name.

**GTK — GVariant.** D-Bus values are dynamically typed. Rust converts with
`(threshold_ms,).to_variant()` (a one-element tuple → `(t)`) and back with
`parameters.get::<(u32,)>()`, which returns `None` if the shape does not
match. The trailing comma makes `(x,)` a tuple rather than a parenthesised
expression.

**Rust — `AtomicU32` and `Ordering::Relaxed`.** Two closures and the
name-owner handler share the current watch ids. An atomic integer can be
read and written from anywhere without a lock; `Relaxed` is enough because
nothing else depends on the order of these stores — they carry no
"happens-before" meaning, just a value. `Arc` shares the atomics between the
closures.

**GTK — keeping the proxy alive.** Signal subscriptions live as long as
the proxy object. The 30-second timeout that only touches `&proxy` exists to
own it: without something holding a reference, the proxy would be dropped
at the end of `start_mutter` and the signals would stop.

## 19.3 Screensaver and logind

```rust
// crates/app/src/native/platform.rs
// ...

fn start_screensaver(handle: TrackerHandle, notifications: bool) {
    let Ok(proxy) = gio::DBusProxy::for_bus_sync(
        gio::BusType::Session,
        gio::DBusProxyFlags::DO_NOT_AUTO_START,
        None::<&gio::DBusInterfaceInfo>,
        "org.gnome.ScreenSaver",
        "/org/gnome/ScreenSaver",
        "org.gnome.ScreenSaver",
        None::<&gio::Cancellable>,
    ) else {
        return;
    };
    proxy.connect_g_signal(move |_, _, signal, parameters| {
        if signal != "ActiveChanged" {
            return;
        }
        let Some((locked,)) = parameters.get::<(bool,)>() else {
            return;
        };
        let now = chrono::Utc::now().timestamp_millis();
        if locked {
            let _ignored = handle.apply(TrackerCommand::IdleDetected { idle_start_ms: now });
        } else {
            apply_return_and_notify(&handle, now, notifications);
        }
    });
    glib::timeout_add_seconds_local(30, move || {
        let _keep_alive = &proxy;
        glib::ControlFlow::Continue
    });
}

fn start_logind(handle: TrackerHandle, notifications: bool) {
    let Ok(proxy) = gio::DBusProxy::for_bus_sync(
        gio::BusType::System,
        gio::DBusProxyFlags::DO_NOT_AUTO_START,
        None::<&gio::DBusInterfaceInfo>,
        "org.freedesktop.login1",
        "/org/freedesktop/login1",
        "org.freedesktop.login1.Manager",
        None::<&gio::Cancellable>,
    ) else {
        return;
    };
    let inhibitor = Arc::new(Mutex::new(take_sleep_inhibitor(&proxy)));
    proxy.connect_g_signal({
        let proxy = proxy.clone();
        let inhibitor = Arc::clone(&inhibitor);
        move |_, _, signal, parameters| {
            if signal != "PrepareForSleep" {
                return;
            }
            let Some((sleeping,)) = parameters.get::<(bool,)>() else {
                return;
            };
            if sleeping {
                let now = chrono::Utc::now().timestamp_millis();
                let _ignored = handle.apply(TrackerCommand::IdleDetected { idle_start_ms: now });
                let _ignored = handle.apply(TrackerCommand::Heartbeat);
                if let Ok(mut guard) = inhibitor.lock() {
                    guard.take();
                }
            } else {
                let now = chrono::Utc::now().timestamp_millis();
                apply_return_and_notify(&handle, now, notifications);
                if let Ok(mut guard) = inhibitor.lock() {
                    *guard = take_sleep_inhibitor(&proxy);
                }
            }
        }
    });
    glib::timeout_add_seconds_local(30, move || {
        let _keep_alive = (&proxy, &inhibitor);
        glib::ControlFlow::Continue
    });
}
```

**What.** Locking the screen is an exact idle boundary: `IdleDetected` at
*now*; unlocking is a return. Suspend is subtler: logind announces
`PrepareForSleep(true)`, the app records idle *and* a heartbeat, then
releases its *delay inhibitor* so the machine may sleep; on wake,
`PrepareForSleep(false)` is a return and a new inhibitor is taken.

**Linux — delay inhibitors.** `Inhibit("sleep", who, why, "delay")` asks
logind to wait (briefly) before sleeping until the caller closes the file
descriptor it hands back. Holding one from start-up means the app always
gets its `PrepareForSleep(true)` *before* the machine sleeps, with time to
persist the heartbeat.

**Rust — `OwnedFd` and `Drop`.** The inhibitor is a file descriptor. Rust
wraps it in `OwnedFd`, which *closes the descriptor when dropped* — so
"release the inhibitor" is `guard.take()`: move the `Option<OwnedFd>` out
and let it drop (Exercise 1 shows the close happening). No `close(2)` call
to forget. `Arc<Mutex<Option<OwnedFd>>>` shares that slot between the
handler and the keep-alive.

**Rust — `let Ok(proxy) = ... else { return }`.** Optional adapters: no
service, no error, no adapter.

## 19.4 Notifying, and the inhibitor helper

```rust
// crates/app/src/native/platform.rs
// ...

fn apply_return_and_notify(handle: &TrackerHandle, return_ms: i64, notifications: bool) {
    if handle
        .apply(TrackerCommand::UserReturned { return_ms })
        .is_err()
    {
        return;
    }
    if !notifications {
        return;
    }
    let Some(application) = gio::Application::default() else {
        return;
    };
    let notification = gio::Notification::new("Idle time needs review");
    notification.set_body(Some(
        "Open Work Time Tracker to keep, discard, reassign, or stop.",
    ));
    notification.set_default_action("app.toggle-timer");
    application.send_notification(Some("idle-resolution"), &notification);
}

/// Asks logind for a delay inhibitor; dropping the fd releases it.
fn take_sleep_inhibitor(proxy: &gio::DBusProxy) -> Option<OwnedFd> {
    let parameters = (
        "sleep",
        "Work Time Tracker",
        "Save the active timer before suspend",
        "delay",
    )
        .to_variant();
    let (reply, fd_list) = proxy
        .call_with_unix_fd_list_sync(
            "Inhibit",
            Some(&parameters),
            gio::DBusCallFlags::NONE,
            -1,
            None::<&gio::UnixFDList>,
            None::<&gio::Cancellable>,
        )
        .ok()?;
    let (index,) = reply.get::<(i32,)>()?;
    fd_list?.get(index).ok()
}
```

**What.** A return is only notified if the engine *accepted*
`UserReturned` — an unlock while the timer is stopped is an
`InvalidState` error and stays silent. The notification's default action
is `app.toggle-timer`, so clicking it presents the window and, since the
state is `IdlePending`, opens the idle dialog (Chapter 17). The inhibitor
helper shows how file descriptors travel over D-Bus.

**Linux — fds over D-Bus.** A `h` value is an *index* into a separate fd
list that rides along with the message. `call_with_unix_fd_list_sync`
returns both; `fd_list?.get(index)` extracts the `OwnedFd`. Every step
returns `Option`, chained with `?` in a function returning `Option`: any
failure means "no inhibitor", which is safe — sleep just proceeds without
the extra heartbeat.

**GTK — notifications.** `gio::Notification` goes through the desktop's
notification service under the app's id (which is why the desktop file in
Chapter 20 must match `APP_ID`). The `Some("idle-resolution")` id replaces
an earlier notification with the same id instead of stacking.

## 19.5 Wire it in

```rust
// crates/app/src/native/mod.rs
mod platform;
mod window;
// ...
```

And restore the block at the end of `install_actions` that Chapter 18
told you to leave out:

```rust
// crates/app/src/native/mod.rs (end of install_actions)
    let idle_threshold = settings
        .as_ref()
        .map_or(5, |settings| settings.uint("idle-threshold-minutes"))
        .clamp(1, 120);
    let notifications = settings
        .as_ref()
        .is_none_or(|settings| settings.boolean("notifications"));
    if let Err(error) = platform::start_integrations(handle, idle_threshold, notifications) {
        warn!(%error, "GNOME idle/session integration is unavailable; manual tracking remains active");
        let message = error.to_string();
        let window = Rc::clone(window);
        glib::idle_add_local_once(move || {
            if let Some(window) = window.borrow().as_ref() {
                window.show_integration_warning(&message);
            }
        });
    }
}
```

The banner in `window.ui`, as the first child of the content box (before
the `AdwViewStack`):

```xml
<!-- data/ui/window.ui (inside the content GtkBox, before view_stack) -->
            <child>
              <object class="AdwBanner" id="integration_banner">
                <property name="revealed">false</property>
                <property name="title" translatable="yes">Automatic idle detection is unavailable</property>
              </object>
            </child>
```

And in `window.rs`, the template child (first in the struct, matching the
original) and the method (after `refresh_entries`):

```rust
// crates/app/src/native/window.rs
        #[template_child]
        pub integration_banner: gtk::TemplateChild<adw::Banner>,
// ...
    pub fn show_integration_warning(&self, message: &str) {
        self.imp().integration_banner.set_title(message);
        self.imp().integration_banner.set_revealed(true);
    }
```

**What.** The idle threshold and notification preference come from
GSettings; a failed Mutter connection becomes a banner instead of a crash,
scheduled for after the window exists.

## 19.6 Checkpoint

```sh
cargo build --features native-ui
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo fmt --all -- --check
for f in mod.rs platform.rs window.rs; do
  diff <(grep -v '^\s*//' ../work_time_tracker/crates/app/src/native/$f) \
       <(grep -v '^\s*//' crates/app/src/native/$f)
done
diff <(grep -v '^\s*<!--' ../work_time_tracker/data/ui/window.ui | sed '/^\s*$/d') \
     <(sed '/^\s*$/d' data/ui/window.ui)
```

`platform.rs` and `window.rs` identical; `mod.rs` differs only by
`load_css`; `window.ui` only by comment lines. **Every Rust file in the
project now matches the original.**

Run it with the settings environment from Chapter 18 and `RUST_LOG=warn`:
no warning means all three adapters connected. Start a timer, lock the
screen (Super+L), unlock: the idle dialog appears and a notification was
sent. Set the idle threshold to 1 minute in Preferences, restart the app,
start a timer, do not touch the machine for a minute, then move the mouse:
same dialog.

```sh
git add -A && git commit -m "Chapter 19: D-Bus integration (all Rust complete)"
```

## 19.7 Exercises

1. **`OwnedFd` closes on drop (temporary test).** Append to `platform.rs`:

   ```rust
   #[cfg(test)]
   mod fds {
       use std::os::fd::{AsRawFd, OwnedFd};

       #[test]
       fn owned_fd_closes_on_drop() {
           let file = std::fs::File::open("/dev/null").unwrap_or_else(|error| panic!("{error}"));
           let fd: OwnedFd = file.into();
           let raw = fd.as_raw_fd();
           let mut holder = Some(fd);
           assert!(std::fs::File::open(format!("/proc/self/fd/{raw}")).is_ok());
           holder.take();
           assert!(std::fs::File::open(format!("/proc/self/fd/{raw}")).is_err());
       }
   }
   ```

   Run `cargo test --features native-ui -p work-time-tracker --lib owned_fd`.

   <details><summary>Answer</summary>

   Passes. `/proc/self/fd/N` exists while descriptor N is open; after
   `holder.take()` drops the `OwnedFd`, it is gone. That `take()` is
   exactly how the logind adapter releases its inhibitor. Revert.
   </details>

2. **Watch the bus.** In one terminal:
   `busctl --user monitor org.gnome.ScreenSaver`. Lock and unlock the
   screen.

   <details><summary>Answer</summary>

   Two `ActiveChanged` signals, `b true` then `b false` — the exact
   messages `start_screensaver` reacts to. `G_DBUS_DEBUG=message` on the
   app itself prints every message it sends and receives, including the
   `AddIdleWatch` call and its `u` reply.
   </details>

3. **Ask for the idle time.** Run the `GetIdletime` call from 19.0 twice,
   with a pause in between, and compare the numbers. Then read
   `add_idle_watch` again.

   <details><summary>Answer</summary>

   The second number is larger by roughly the pause (milliseconds since
   your last input). Mutter's *watch* is the push version of this pull: it
   calls you when the counter crosses the threshold, once, and you re-arm
   it — which is why `WatchFired` handling ends by adding the next watch.
   </details>

## Recap

- D-Bus services are explored with `busctl` and driven with
  `gio::DBusProxy`; GVariant tuples map to Rust tuples.
- Idle detection is a one-shot watch re-armed after each return; lock and
  suspend are exact boundaries.
- Proxies must be owned by someone to keep delivering signals; atomics
  share watch ids between handlers.
- `OwnedFd` turns "release the inhibitor" into a drop.
- Every event becomes a command; the engine decides what it means.

Next: **Chapter 20 — Packaging**, the desktop file, AppStream metadata,
icons, Meson, translations, and a staged install.
