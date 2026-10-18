# Proposal: Linux app on FlatPark

## Why
Flathub bans AI-assisted apps, so Linux users only get the app as
`.flatpak` bundles on GitHub releases: no updates, no store. FlatPark
(flatpark.org) is a signed Flatpak remote that welcomes AI-assisted
apps. It does not build apps: its package downloads the vendor's own
release file at install time (`extra-data`), unpacks it and runs it on
a Flatpak runtime. It rejects `.flatpak` bundles as payload; it takes a
tarball of the installed app tree, built in the runtime's SDK, and it
requires the current runtime major (GNOME 51; we are on 50).

## What changes
1. **GNOME 51.** Manifest `runtime-version` 50 -> 51, both CI jobs on
   the `gnome-51` builder image. Our own bundles move to 51 too.
2. **A tarball per architecture.** Each Flatpak CI job also packs the
   finished `/app` tree (`flatpak_app/files`) under one top directory
   `meditate/`, the shape FlatPark's tar template expects, as
   `meditate-<arch>.tar.gz` and uploads it as an artifact. The app
   embeds its UI and sounds (gresource), needs no paths beyond
   `LOCALEDIR` (already overridable by env) and the GSettings schema
   (`GSETTINGS_SCHEMA_DIR`), so the same tree runs from `/app/extra`
   with no app code change.
3. **Releases carry them.** `attach-flatpaks.sh` also uploads
   `meditate-<version>-linux-<arch>.tar.gz`. Our own
   `resolve-update.sh` in the package matches it anchored
   (`linux-<arch>\.tar\.gz$`) and fails unless both are attached, so a
   check between publishing and attaching never pins only one.
4. **The FlatPark package**, drafted in this change folder
   (`flatpark-package/`): `flatpark.yml`, manifest, `apply_extra.sh`,
   wrapper (sets `LOCALEDIR` and `GSETTINGS_SCHEMA_DIR`, execs the
   unmodified binary), desktop file, metainfo, icon, D-Bus service,
   `resolve-update.sh`. Used for the local install test now; the pull
   request to github.com/flatpark/flatpark, with the upstream-approval
   row for the developer-approved shield, follows after the next
   release, with its own go.
5. **Docs.** RELEASE.md 5b (tarballs are attached too), BUILDING.md and
   README "GNOME 50" -> 51. README gets the FlatPark install lines only
   when the listing is live (the follow-up), not now.

## Decisions
- Both architectures (PC x86_64, Librem 5 aarch64) under the one app id
  `io.github.janekbt.Meditate`. No beta channel. (Janek)
- Listed with the next regular release, no extra release. (Janek)
- No `publish-action`: releases are created by hand and the tarballs
  are attached after publishing, so the action would fire before the
  file exists; FlatPark's twice-daily check picks it up. (my call)
- Sandbox permissions in the FlatPark manifest: ours minus the two
  notification bus names. GLib sends notifications through the portal
  inside a sandbox, so they were never needed (there since the first
  scaffold); FlatPark asks for the smallest set. Our bundle keeps them,
  out of scope. (my call)
- Desktop file and metainfo in the package are the translated ones
  Meson builds, the metainfo with one `<release>` entry as FlatPark
  wants; their updater adds the later ones. Description and
  screenshots there go stale until a PR refreshes them. (my call)
- Same app id as the bundle, so settings and data in
  `~/.var/app/io.github.janekbt.Meditate` carry over when a user
  switches. (my call)

## Dropped
publish-action; a beta channel; trimming blueprint-compiler out of the
tree (it is in today's bundles too, about 1 MB); README install lines
before the listing exists.

## Reaches
Manifest, `.github/workflows/flatpak.yml`, `build-aux/attach-flatpaks.sh`,
RELEASE.md, BUILDING.md, README. No core, GTK, Android, translation,
database or Android build environment change. Laptop: installs the
GNOME 51 runtime and SDK (user install) for the local test.
