use crate::date_format::LocalTimestampFormat;
use crate::desktop::log_background_error;
use crate::locale::{tr, trf};
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
            .heading(tr("Recover interrupted timer?"))
            .body(if pending.unresolved_idle_start_ms.is_some() {
                tr("The app stopped during idle reconciliation. Edit the proposed end, then keep or discard it.")
            } else {
                tr("Only time up to the last saved heartbeat is proposed. You may edit that end time.")
            })
            .build();
        let timestamps = LocalTimestampFormat::new(self.date_format());
        let initial = timestamps.format(pending.proposed_end_ms);
        let end_entry = gtk::Entry::builder()
            .text(&initial)
            .placeholder_text(timestamps.example())
            .hexpand(true)
            .build();
        let validation = gtk::Label::builder()
            .halign(gtk::Align::Start)
            .css_classes(["dim-label"])
            .build();
        let extras = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(6)
            .build();
        extras.append(&end_entry);
        extras.append(&validation);
        dialog.set_extra_child(Some(&extras));
        dialog.add_responses(&[
            ("resume", tr("Keep and Resume")),
            ("stop", tr("Keep and Stop")),
            ("discard", tr("Discard")),
        ]);
        dialog.set_default_response(Some("stop"));
        // Untouched text keeps the original instant, including milliseconds
        // the whole-second display cannot show.
        let proposed_end_ms = pending.proposed_end_ms;
        let resolve = {
            let timestamps = timestamps.clone();
            let initial = initial.clone();
            move |text: &str| {
                if text == initial {
                    Some(proposed_end_ms)
                } else {
                    timestamps.parse(text)
                }
            }
        };
        let refresh = {
            let resolve = resolve.clone();
            let validation = validation.clone();
            let dialog = dialog.clone();
            let guidance = trf(
                "End time must look like {example} and identify one local time.",
                &[("example", &timestamps.example())],
            );
            move |entry: &gtk::Entry| {
                let valid = resolve(&entry.text()).is_some();
                dialog.set_response_enabled("resume", valid);
                dialog.set_response_enabled("stop", valid);
                validation.set_label(if valid { "" } else { &guidance });
            }
        };
        refresh(&end_entry);
        end_entry.connect_changed(move |entry| refresh(entry));
        let weak = self.downgrade();
        dialog.connect_response(None, move |_, response| {
            let result = if response == "discard" {
                handle.apply(TrackerCommand::DiscardRecovery)
            } else {
                let Some(end_ms) = resolve(&end_entry.text()) else {
                    // The keep actions stay disabled while the text is
                    // invalid, so this only guards a stale response.
                    return;
                };
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
