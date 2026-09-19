use crate::desktop::log_background_error;
use chrono::{Local, NaiveDateTime, TimeZone};
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use houra_core::{TrackerCommand, TrackerState};
use libadwaita as adw;
use libadwaita::prelude::*;

use crate::desktop::window::MainWindow;

impl MainWindow {
    pub(in crate::desktop) fn show_recovery_dialog(&self) {
        let Some(handle) = self.handle() else { return };
        let Ok(snapshot) = handle.snapshot() else {
            return;
        };
        let TrackerState::RecoveryPending(pending) = snapshot.state else {
            return;
        };
        let dialog = adw::AlertDialog::builder()
            .heading("Recover interrupted timer?")
            .body(if pending.unresolved_idle_start_ms.is_some() {
                "The app stopped during idle reconciliation. Edit the proposed end, then keep or discard it."
            } else {
                "Only time up to the last saved heartbeat is proposed. You may edit that end time."
            })
            .build();
        let proposed_end = Local
            .timestamp_millis_opt(pending.proposed_end_ms)
            .single()
            .map_or_else(String::new, |value| {
                value.format("%Y-%m-%d %H:%M:%S").to_string()
            });
        let end_entry = gtk::Entry::builder()
            .text(proposed_end)
            .placeholder_text("YYYY-MM-DD HH:MM:SS")
            .build();
        dialog.set_extra_child(Some(&end_entry));
        dialog.add_responses(&[
            ("resume", "Keep and Resume"),
            ("stop", "Keep and Stop"),
            ("discard", "Discard"),
        ]);
        dialog.set_default_response(Some("stop"));
        let weak = self.downgrade();
        dialog.connect_response(None, move |_, response| {
            let result = if response == "discard" {
                handle.apply(TrackerCommand::DiscardRecovery)
            } else {
                let end_ms = NaiveDateTime::parse_from_str(&end_entry.text(), "%Y-%m-%d %H:%M:%S")
                    .ok()
                    .and_then(|value| Local.from_local_datetime(&value).single())
                    .map_or(pending.proposed_end_ms, |value| value.timestamp_millis());
                handle.apply(TrackerCommand::ResolveRecovery {
                    end_ms,
                    resume: response == "resume",
                })
            };
            let succeeded = result.is_ok();
            if let Err(error) = result {
                log_background_error("recovering timer", error);
            }
            if let Some(window) = weak.upgrade() {
                if succeeded && response == "stop" {
                    window.imp().note_entry.set_text("");
                }
                window.refresh();
            }
        });
        dialog.present(Some(self));
    }
}
