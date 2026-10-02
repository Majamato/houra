//! Publishes the active timer on D-Bus for Houra's GNOME Shell extension,
//! which draws Houra's element in the top bar.

mod extension;
mod status;

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use gio::prelude::*;
use glib::variant::ToVariant;
use houra_core::TrackerSnapshot;
use libadwaita as adw;
use tracing::warn;

use crate::locale::tr;
use crate::{AppError, TrackerHandle};
pub(in crate::desktop) use extension::enable_shell_extension;
use status::{TopBarState, TopBarStatus};

/// D-Bus contract; `shell-extension/activeTimer.js` embeds an identical copy.
const INTERFACE_XML: &str =
    include_str!("../../../../../data/dbus/io.github.majamato.Houra.ActiveTimer.xml");
const INTERFACE_NAME: &str = concat!(crate::app_id!(), ".ActiveTimer");
const PROPERTY_NAMES: [&str; 3] = ["State", "ElapsedMs", "Summary"];

/// Publishes the active timer; clones share one D-Bus registration.
#[derive(Clone)]
pub(in crate::desktop) struct TopBar {
    inner: Rc<Inner>,
}

struct Inner {
    application: glib::WeakRef<adw::Application>,
    handle: TrackerHandle,
    connection: gio::DBusConnection,
    object_path: String,
    /// Tracker revision and state last announced; `None` before the first one.
    published: RefCell<Option<(u64, TopBarState)>>,
}

impl TopBar {
    /// Exports `ActiveTimer` on the application's own connection and object
    /// path, then announces the current timer.
    pub(in crate::desktop) fn register(
        application: &adw::Application,
        handle: TrackerHandle,
    ) -> Result<Self, AppError> {
        let connection = application
            .dbus_connection()
            .ok_or_else(|| AppError::TopBar("Houra has no session bus connection".into()))?;
        let object_path = application
            .dbus_object_path()
            .ok_or_else(|| AppError::TopBar("Houra has no D-Bus object path".into()))?
            .to_string();
        let node = gio::DBusNodeInfo::for_xml(INTERFACE_XML)
            .map_err(|error| AppError::TopBar(error.to_string()))?;
        let interface = node
            .lookup_interface(INTERFACE_NAME)
            .ok_or_else(|| AppError::TopBar(format!("{INTERFACE_NAME} is missing")))?;
        let top_bar = Self {
            inner: Rc::new(Inner {
                application: application.downgrade(),
                handle,
                connection: connection.clone(),
                object_path: object_path.clone(),
                published: RefCell::new(None),
            }),
        };
        let for_methods = Rc::downgrade(&top_bar.inner);
        let for_properties = Rc::downgrade(&top_bar.inner);
        connection
            .register_object(&object_path, &interface)
            .method_call(
                move |_, _, _, _, method, _, invocation| match for_methods.upgrade() {
                    Some(inner) => Self { inner }.call(method, invocation),
                    None => invocation.return_dbus_error(
                        "org.freedesktop.DBus.Error.Failed",
                        "Houra is shutting down",
                    ),
                },
            )
            .property(move |_, _, _, _, property| {
                let status = for_properties
                    .upgrade()
                    .map(|inner| Self { inner }.current_status())
                    .unwrap_or_default();
                property_value(&status, property).unwrap_or_else(|| "".to_variant())
            })
            .build()
            .map_err(|error| AppError::TopBar(error.to_string()))?;
        // Announce at once: the Shell may have read properties before the
        // object existed.
        top_bar.sync();
        Ok(top_bar)
    }

    /// Announces the active timer when the tracker changed since the last
    /// announcement. Costs one snapshot request when nothing changed.
    pub(in crate::desktop) fn sync(&self) {
        let Ok(snapshot) = self.inner.handle.snapshot() else {
            return;
        };
        let key = (snapshot.revision, TopBarState::of(&snapshot.state));
        if *self.inner.published.borrow() == Some(key) {
            return;
        }
        let status = self.status_for(&snapshot);
        match self.inner.connection.emit_signal(
            None,
            &self.inner.object_path,
            "org.freedesktop.DBus.Properties",
            "PropertiesChanged",
            Some(&properties_changed(&status)),
        ) {
            Ok(()) => {
                self.inner.published.replace(Some(key));
            }
            Err(error) => warn!(%error, "could not announce the active timer"),
        }
    }

    /// Runs the matching application action; the extension hides controls
    /// that don't apply, and the window routes pending reviews itself.
    fn call(&self, method: &str, invocation: gio::DBusMethodInvocation) {
        let action = match method {
            "TogglePause" => "toggle-pause",
            "OpenReview" => "review-pending",
            _ => {
                invocation.return_dbus_error(
                    "org.freedesktop.DBus.Error.UnknownMethod",
                    &format!("Unknown method {method}"),
                );
                return;
            }
        };
        if let Some(application) = self.inner.application.upgrade() {
            application.activate_action(action, None);
        }
        self.sync();
        invocation.return_value(None);
    }

    fn current_status(&self) -> TopBarStatus {
        self.inner
            .handle
            .snapshot()
            .map(|snapshot| self.status_for(&snapshot))
            .unwrap_or_default()
    }

    fn status_for(&self, snapshot: &TrackerSnapshot) -> TopBarStatus {
        let handle = &self.inner.handle;
        let saved_ms = snapshot
            .state
            .active()
            .and_then(|active| active.entry_id)
            .and_then(|entry_id| handle.entry(entry_id).ok())
            .map_or(0, |entry| entry.duration_ms());
        TopBarStatus::new(
            &snapshot.state,
            saved_ms,
            handle.live_elapsed().unwrap_or_default(),
            &handle.projects(true).unwrap_or_default(),
            &handle.activities(true).unwrap_or_default(),
            tr("Tracked work"),
        )
    }
}

fn property_value(status: &TopBarStatus, name: &str) -> Option<glib::Variant> {
    match name {
        "State" => Some(status.state.wire_name().to_variant()),
        "ElapsedMs" => Some(status.elapsed_ms.to_variant()),
        "Summary" => Some(status.summary.to_variant()),
        _ => None,
    }
}

/// Body of `org.freedesktop.DBus.Properties.PropertiesChanged`: `(sa{sv}as)`.
fn properties_changed(status: &TopBarStatus) -> glib::Variant {
    let changed: HashMap<&str, glib::Variant> = PROPERTY_NAMES
        .into_iter()
        .filter_map(|name| property_value(status, name).map(|value| (name, value)))
        .collect();
    (INTERFACE_NAME, changed, Vec::<String>::new()).to_variant()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interface_xml_declares_the_extension_contract() {
        let node = gio::DBusNodeInfo::for_xml(INTERFACE_XML)
            .unwrap_or_else(|error| panic!("invalid interface XML: {error}"));
        let Some(interface) = node.lookup_interface(INTERFACE_NAME) else {
            panic!("{INTERFACE_NAME} is missing from the XML");
        };
        for method in ["TogglePause", "OpenReview"] {
            assert!(interface.lookup_method(method).is_some(), "{method}");
        }
        for property in PROPERTY_NAMES {
            assert!(interface.lookup_property(property).is_some(), "{property}");
        }
    }

    #[test]
    fn properties_use_the_declared_types() {
        let status = TopBarStatus::default();
        for (name, signature) in [("State", "s"), ("ElapsedMs", "t"), ("Summary", "s")] {
            let Some(value) = property_value(&status, name) else {
                panic!("{name} has no value");
            };
            assert_eq!(value.type_().as_str(), signature, "{name}");
        }
        assert!(property_value(&status, "Nope").is_none());
    }

    #[test]
    fn properties_changed_carries_every_property() {
        let value = properties_changed(&TopBarStatus::default());
        assert_eq!(value.type_().as_str(), "(sa{sv}as)");
        assert_eq!(value.child_value(0).str(), Some(INTERFACE_NAME));
        let dict = glib::VariantDict::new(Some(&value.child_value(1)));
        assert!(matches!(
            dict.lookup::<String>("State"),
            Ok(Some(state)) if state == "stopped"
        ));
        assert!(matches!(dict.lookup::<u64>("ElapsedMs"), Ok(Some(0))));
        assert!(matches!(
            dict.lookup::<String>("Summary"),
            Ok(Some(summary)) if summary.is_empty()
        ));
        assert_eq!(value.child_value(2).n_children(), 0);
    }
}
