use crate::locale::tr;
use gio::prelude::*;
use glib::variant::ToVariant;
use houra_core::TrackerCommand;
use std::os::fd::OwnedFd;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

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
    let notification = gio::Notification::new(tr("Idle time needs review"));
    notification.set_body(Some(tr("Open Houra to keep, discard, reassign, or stop.")));
    notification.set_default_action("app.toggle-timer");
    application.send_notification(Some("idle-resolution"), &notification);
}

/// Asks logind for a delay inhibitor; dropping the fd releases it.
fn take_sleep_inhibitor(proxy: &gio::DBusProxy) -> Option<OwnedFd> {
    let parameters = (
        "sleep",
        "Houra",
        tr("Save the active timer before suspend"),
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
