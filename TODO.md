# TODO — small items for later

Polish and UX items to tackle when convenient. Graduate each one out of this file as it lands in a commit — remove the entry rather than letting the list grow stale.

## UI / HIG polish

- [ ] **Discard-session button should be destructive (red)**
  After a completed session, the "Discard" button is a destructive action per GNOME HIG. Fix: set `adw::ResponseAppearance::Destructive` on the "discard" response of the AdwAlertDialog in `src/timer/imp.rs` around the on_discard handler, OR if the button is a plain gtk::Button, add the `destructive-action` CSS class.

- [ ] **Stop button during an active meditation should be destructive (red)**
  During a running session, the "Stop" button cancels progress — destructive. Add the `destructive-action` CSS class (or equivalent AdwAlertDialog response appearance if it's a dialog). Likely lives alongside the discard button in `src/timer/imp.rs` / `data/ui/timer_view.blp`.

- [ ] **Phosh startup splash (logo + spinner) doesn't appear on Librem 5**
  Other locally-installed apps show Phosh's launcher splash while loading; Meditate doesn't. Check: (1) does `data/io.github.janekbt.Meditate.desktop.in` have `StartupNotify=true`? (2) does `gtk::Application` inherit the `xdg_activation_v1` token via `gio::Application::activate`? (3) does the Wayland surface `app_id` match the `.desktop` basename?
