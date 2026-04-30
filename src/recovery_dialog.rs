//! Recovery dialog for the "remote data lost" detection.
//!
//! When `Sync::pull` notices that every batch this device previously
//! pushed is gone from the Nextcloud folder, it surfaces
//! `SyncError::RemoteDataLost` instead of silently re-uploading.
//! The status indicator's click handler then opens this dialog so the
//! user explicitly chooses how to recover.
//!
//! The dialog body is the substantive UX surface — the actions
//! themselves delegate to `sync_settings::prepare_push_local_recovery`
//! plus `app.trigger_sync()`. Those primitives are unit-tested
//! separately; this module is a thin GTK shell on top.

use adw::prelude::*;
use gtk::glib;

use crate::application::MeditateApplication;
use crate::i18n::gettext;

/// Open the recovery dialog. The user picks one of:
/// - "Push my data" — wipes the dedup tracker, flags every event
///   un-synced, clears the error display, kicks a fresh sync. The
///   re-sync uploads every event as a single bulk batch.
/// - "Cancel" — closes the dialog without changing anything; the
///   status indicator remains in warning state until the user picks
///   again or sync recovers some other way.
///
/// "Wipe local to match remote" intentionally isn't shipped here —
/// it's a destructive irreversible action, deserves its own
/// confirmation flow, and isn't needed for the immediate
/// "I accidentally deleted the Meditate folder" use case.
pub fn show(app: &MeditateApplication) {
    let dialog = adw::AlertDialog::builder()
        .heading(gettext("Remote data appears wiped"))
        .body(gettext(
            "The Nextcloud folder no longer contains any of the batches \
             this device previously synced. Has the Meditate folder been \
             deleted on the server?\n\n\
             Choose how to recover:"))
        .default_response("cancel")
        .close_response("cancel")
        .build();

    dialog.add_response("cancel", &gettext("Cancel"));
    dialog.add_response("push", &gettext("Push My Data"));
    // The push action is suggested-action because it's the typical
    // recovery — the user kept their local data and wants the remote
    // to match. Cancel is the safe default in case they're not sure.
    dialog.set_response_appearance("push", adw::ResponseAppearance::Suggested);

    dialog.connect_response(Some("push"), glib::clone!(
        #[weak] app,
        move |_, _| {
            run_push_local_recovery(&app);
        }
    ));

    let parent = app.active_window();
    dialog.present(parent.as_ref());
}

/// Execute the "push my data" action. Pure data manipulation +
/// sync trigger; no UI here so it can be exercised from tests via
/// the same primitives the dialog uses.
fn run_push_local_recovery(app: &MeditateApplication) {
    let prep = app.with_db(|db|
        crate::sync_settings::prepare_push_local_recovery(db));
    match prep {
        Some(Ok(())) => {
            crate::diag::log("recovery: push local — prepared, triggering sync");
            app.trigger_sync();
        }
        Some(Err(e)) => {
            crate::diag::log(&format!(
                "recovery: push local — prepare failed: {e:?}"));
        }
        None => {
            crate::diag::log("recovery: push local — DB unavailable");
        }
    }
}
