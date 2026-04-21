# TODO — small items for later

Polish and UX items to tackle when convenient. Graduate each one out of this file as it lands in a commit — remove the entry rather than letting the list grow stale.

## UI / HIG polish

- [ ] **Phosh startup splash (logo + spinner) doesn't appear on Librem 5**
  Other locally-installed apps show Phosh's launcher splash while loading; Meditate doesn't. First attempt was adding `StartupNotify=true` to `data/io.github.janekbt.Meditate.desktop.in` — on-device test showed no change, so reverted. Needs deeper investigation: (1) compare with a native app that DOES splash (gnome-calculator etc.) and diff their `.desktop` + Wayland surface `app_id`; (2) check whether `gio::Application` is consuming the `xdg_activation_v1` token before the window maps (the splash-to-window handoff would then land on nothing); (3) verify whether Phoc/Phosh on PureOS Crimson actually renders third-party-app splashes at all, or only built-in ones.
