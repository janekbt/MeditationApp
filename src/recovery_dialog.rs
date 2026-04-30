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
/// - "Push My Data" — re-uploads local state. Wipes the dedup
///   tracker, flags every event un-synced, clears the error display,
///   kicks a fresh sync.
/// - "Wipe Local" — discards local state to match the (empty)
///   remote. Goes through a second confirmation dialog because the
///   action is irreversible.
/// - "Cancel" — closes without changing anything; the indicator
///   remains in warning state until the user picks again.
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
    dialog.add_response("wipe", &gettext("Wipe Local"));
    dialog.add_response("push", &gettext("Push My Data"));
    // Push is suggested — typical recovery, the user kept their local
    // data and wants the remote to match. Wipe is destructive — its
    // own confirmation step (see `confirm_wipe_local`) catches stray
    // taps. Cancel is the safe default if the user is unsure.
    dialog.set_response_appearance("push", adw::ResponseAppearance::Suggested);
    dialog.set_response_appearance("wipe", adw::ResponseAppearance::Destructive);

    dialog.connect_response(Some("push"), glib::clone!(
        #[weak] app,
        move |_, _| run_push_local_recovery(&app),
    ));
    dialog.connect_response(Some("wipe"), glib::clone!(
        #[weak] app,
        move |_, _| confirm_wipe_local(&app),
    ));

    let parent = app.active_window();
    dialog.present(parent.as_ref());
}

/// Execute the "Push My Data" action. Pure data manipulation +
/// sync trigger; the underlying `prepare_push_local_recovery`
/// primitive is unit-tested in `sync_settings`.
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

/// Second confirmation step for "Wipe Local". The first dialog was
/// "remote data lost — what now?"; users who picked "Wipe Local"
/// there might still misclick. This step is explicit about
/// irreversibility before any destructive write happens.
fn confirm_wipe_local(app: &MeditateApplication) {
    let dialog = adw::AlertDialog::builder()
        .heading(gettext("Wipe local data?"))
        .body(gettext(
            "This permanently deletes every session, label, and event \
             stored on this device. Your Nextcloud account, sync settings, \
             and app preferences are kept.\n\n\
             This cannot be undone."))
        .default_response("cancel")
        .close_response("cancel")
        .build();

    dialog.add_response("cancel", &gettext("Cancel"));
    dialog.add_response("wipe", &gettext("Wipe Local Data"));
    dialog.set_response_appearance("wipe", adw::ResponseAppearance::Destructive);

    dialog.connect_response(Some("wipe"), glib::clone!(
        #[weak] app,
        move |_, _| run_wipe_local_recovery(&app),
    ));

    let parent = app.active_window();
    dialog.present(parent.as_ref());
}

/// Execute the "Wipe Local" action: erase every authored row,
/// invalidate cached UI state across views, kick a fresh sync.
/// `prepare_wipe_local_recovery` is unit-tested separately.
fn run_wipe_local_recovery(app: &MeditateApplication) {
    let prep = app.with_db_mut(|db|
        crate::sync_settings::prepare_wipe_local_recovery(db));
    match prep {
        Some(Ok(())) => {
            crate::diag::log("recovery: wipe local — purged, triggering sync");
            // Force every cached view to re-read from the (empty) DB.
            // with_db_mut already bumps the invalidate-all flag, but
            // a manual ALL invalidate ensures even non-mutating views
            // notice the change.
            app.invalidate(crate::application::InvalidateScope::ALL);
            app.trigger_sync();
        }
        Some(Err(e)) => {
            crate::diag::log(&format!(
                "recovery: wipe local — prepare failed: {e:?}"));
        }
        None => {
            crate::diag::log("recovery: wipe local — DB unavailable");
        }
    }
}
