//! The running session as an MPRIS media player, so the shell's media
//! controls (GNOME's shade, the Phosh lock screen) show its time and
//! the play/pause keys reach it. Core decides the line and the bar;
//! this module words it and speaks D-Bus through gio.

use gettextrs::gettext;
use gtk::{gio, glib, prelude::*};
use meditate_core::session::{MediaBar, MediaLine, MediaStatus, Session, ToggleAction};
use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::rc::Rc;
use std::time::Duration;

const BUS_NAME: &str = "org.mpris.MediaPlayer2.io.github.janekbt.Meditate";
const PATH: &str = "/org/mpris/MediaPlayer2";
const ROOT: &str = "org.mpris.MediaPlayer2";
const PLAYER: &str = "org.mpris.MediaPlayer2.Player";
const TRACK: &str = "/io/github/janekbt/Meditate/session";
/// The app icon as the player's cover; the shell reads it from a file.
const COVER: &[u8] = include_bytes!("../data/icons/hicolor/scalable/apps/io.github.janekbt.Meditate.svg");

/// The cover's URI, written once per run to the app's cache, which the
/// shell can read also from outside a Flatpak; `None` when that failed,
/// and the shell shows its own note symbol.
fn cover_uri() -> Option<&'static str> {
    static URI: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();
    URI.get_or_init(|| {
        let dir = glib::user_cache_dir().join("meditate");
        let path = dir.join("mpris-cover.svg");
        let written = std::fs::create_dir_all(&dir).and_then(|()| std::fs::write(&path, COVER));
        if let Err(e) = &written {
            meditate_core::log("mpris", &format!("cover: {e}"));
        }
        written.ok().and_then(|()| glib::filename_to_uri(&path, None).ok()).map(String::from)
    })
    .as_deref()
}

const XML: &str = r#"<node>
  <interface name="org.mpris.MediaPlayer2">
    <method name="Raise"/><method name="Quit"/>
    <property name="CanQuit" type="b" access="read"/>
    <property name="CanRaise" type="b" access="read"/>
    <property name="HasTrackList" type="b" access="read"/>
    <property name="Identity" type="s" access="read"/>
    <property name="DesktopEntry" type="s" access="read"/>
    <property name="SupportedUriSchemes" type="as" access="read"/>
    <property name="SupportedMimeTypes" type="as" access="read"/>
  </interface>
  <interface name="org.mpris.MediaPlayer2.Player">
    <method name="Next"/><method name="Previous"/><method name="Pause"/>
    <method name="PlayPause"/><method name="Stop"/><method name="Play"/>
    <method name="Seek"><arg name="Offset" type="x" direction="in"/></method>
    <method name="SetPosition"><arg name="TrackId" type="o" direction="in"/><arg name="Position" type="x" direction="in"/></method>
    <method name="OpenUri"><arg name="Uri" type="s" direction="in"/></method>
    <signal name="Seeked"><arg name="Position" type="x"/></signal>
    <property name="PlaybackStatus" type="s" access="read"/>
    <property name="Rate" type="d" access="read"/>
    <property name="Metadata" type="a{sv}" access="read"/>
    <property name="Volume" type="d" access="read"/>
    <property name="Position" type="x" access="read"/>
    <property name="MinimumRate" type="d" access="read"/>
    <property name="MaximumRate" type="d" access="read"/>
    <property name="CanGoNext" type="b" access="read"/>
    <property name="CanGoPrevious" type="b" access="read"/>
    <property name="CanPlay" type="b" access="read"/>
    <property name="CanPause" type="b" access="read"/>
    <property name="CanSeek" type="b" access="read"/>
    <property name="CanControl" type="b" access="read"/>
  </interface>
</node>"#;

/// What the player shows: title, line, bar, whether the time stands,
/// and what play/pause does (nothing in overtime).
#[derive(Debug, Clone, PartialEq)]
pub struct Shown {
    title: String,
    line: String,
    bar: Option<MediaBar>,
    standing: bool,
    /// Where the bar stands, or the moving bar's start: a change is a
    /// jump the rate doesn't predict (pause, resume, the preparation's
    /// end, overtime).
    anchor: Option<Duration>,
    toggle: ToggleAction,
}

/// The player for `session` at `now`; `None` once it stopped.
pub fn shown(session: &Session, now: Duration, guided_name: Option<&str>) -> Option<Shown> {
    let status = session.media_status(now)?;
    Some(Shown {
        title: meditate_core::session::media_title(guided_name).to_string(),
        line: line_text(status),
        bar: status.bar,
        standing: status.standing,
        anchor: status.bar.map(|b| if status.standing { b.elapsed } else { now.saturating_sub(b.elapsed) }),
        toggle: session.toggle_action(),
    })
}

fn line_text(status: MediaStatus) -> String {
    let t = |secs: u64| meditate_core::format::format_time(Duration::from_secs(secs));
    match status.line {
        MediaLine::Left(s) if status.standing => gettext("Paused, {time} left").replace("{time}", &t(s)),
        MediaLine::Left(s) => gettext("{time} left").replace("{time}", &t(s)),
        MediaLine::SoFar(s) if status.standing => gettext("Paused, {time}").replace("{time}", &t(s)),
        MediaLine::SoFar(s) => t(s),
        MediaLine::Preparing(s) => gettext("Preparation, {time} left").replace("{time}", &t(s)),
        MediaLine::Overtime(s) => gettext("Overtime +{time}").replace("{time}", &t(s)),
    }
}

/// The position to send as `Seeked` when the bar jumps (see `anchor`).
fn seeked(old: &Shown, new: &Shown) -> Option<i64> {
    ((old.standing, old.anchor) != (new.standing, new.anchor)).then(|| position(new))
}

fn position(shown: &Shown) -> i64 {
    micros(shown.bar.map_or(Duration::ZERO, |b| b.elapsed))
}

fn micros(d: Duration) -> i64 {
    i64::try_from(d.as_micros()).unwrap_or(i64::MAX)
}

fn offered(toggle: ToggleAction) -> bool {
    matches!(toggle, ToggleAction::Pause | ToggleAction::Resume)
}

/// One property's value; `None` for a name the interfaces don't have.
fn property(shown: &Shown, name: &str) -> Option<glib::Variant> {
    Some(match name {
        "CanQuit" | "HasTrackList" | "CanGoNext" | "CanGoPrevious" | "CanSeek" => false.to_variant(),
        // GNOME drops a player that can't play, so it always can; in
        // overtime play/pause does nothing (the session's NoOp).
        "CanRaise" | "CanControl" | "CanPlay" => true.to_variant(),
        "Identity" => "Meditate".to_variant(),
        "DesktopEntry" => crate::config::APP_ID.to_variant(),
        "SupportedUriSchemes" | "SupportedMimeTypes" => Vec::<String>::new().to_variant(),
        "PlaybackStatus" => if shown.standing { "Paused" } else { "Playing" }.to_variant(),
        "Rate" | "MinimumRate" | "MaximumRate" | "Volume" => 1.0f64.to_variant(),
        "Position" => position(shown).to_variant(),
        "CanPause" => offered(shown.toggle).to_variant(),
        "Metadata" => {
            // Sorted, so the same contents give the same variant.
            let mut m: BTreeMap<&str, glib::Variant> = BTreeMap::new();
            m.insert("mpris:trackid", glib::variant::ObjectPath::try_from(TRACK).expect("valid path").to_variant());
            m.insert("xesam:title", shown.title.to_variant());
            if let Some(uri) = cover_uri() {
                m.insert("mpris:artUrl", uri.to_variant());
            }
            m.insert("xesam:artist", vec![shown.line.clone()].to_variant());
            if let Some(bar) = shown.bar {
                m.insert("mpris:length", micros(Duration::from_secs(bar.total_secs)).to_variant());
            }
            m.to_variant()
        }
        _ => return None,
    })
}

// Position never signals (MPRIS): clients read it and follow Rate.
const CHANGING: [&str; 3] = ["PlaybackStatus", "Metadata", "CanPause"];

/// The registered player. Dropping it takes it off the bus.
#[derive(Debug)]
pub struct Player {
    conn: gio::DBusConnection,
    owner: Option<gio::OwnerId>,
    ids: Vec<gio::RegistrationId>,
    shown: Rc<RefCell<Shown>>,
}

impl Player {
    /// Put the player on the app's session bus. `toggle` runs the
    /// session's play/pause, `raise` brings the window up. A failure only reaches the log:
    /// the session runs the same without a media control.
    pub fn start(
        conn: &gio::DBusConnection,
        shown: Shown,
        toggle: impl Fn() + 'static,
        raise: impl Fn() + 'static,
    ) -> Option<Self> {
        let node = gio::DBusNodeInfo::for_xml(XML).map_err(|e| meditate_core::log("mpris", &e.to_string())).ok()?;
        let shown = Rc::new(RefCell::new(shown));
        let toggle = Rc::new(toggle);
        let raise = Rc::new(raise);
        let infos = [node.lookup_interface(ROOT)?, node.lookup_interface(PLAYER)?];
        let mut ids = Vec::new();
        for info in infos {
            let (s, t, r) = (shown.clone(), toggle.clone(), raise.clone());
            let id = conn
                .register_object(PATH, &info)
                .method_call(move |_, _, _, _, method, _, call| {
                    match method {
                        "PlayPause" => t(),
                        "Play" if s.borrow().toggle == ToggleAction::Resume => t(),
                        "Pause" if s.borrow().toggle == ToggleAction::Pause => t(),
                        "Raise" => r(),
                        _ => {}
                    }
                    call.return_value(None);
                })
                .property({
                    let s = shown.clone();
                    move |_, _, _, _, name| property(&s.borrow(), name).unwrap_or_else(|| false.to_variant())
                })
                .build();
            match id {
                Ok(id) => ids.push(id),
                Err(e) => {
                    meditate_core::log("mpris", &format!("register failed: {e}"));
                    for id in ids {
                        let _ = conn.unregister_object(id);
                    }
                    return None;
                }
            }
        }
        let owner = gio::bus_own_name_on_connection(
            conn,
            BUS_NAME,
            gio::BusNameOwnerFlags::NONE,
            |_, _| {},
            |_, name| meditate_core::log("mpris", &format!("could not own {name}")),
        );
        Some(Self { conn: conn.clone(), owner: Some(owner), ids, shown })
    }

    /// Show `next`, telling the shell what changed.
    pub fn update(&self, next: Shown) {
        let (changed, seek): (HashMap<&str, glib::Variant>, _) = {
            let mut shown = self.shown.borrow_mut();
            if *shown == next {
                return;
            }
            let seek = seeked(&shown, &next);
            let changed = CHANGING
                .into_iter()
                .filter_map(|name| {
                    let new = property(&next, name)?;
                    (property(&shown, name).as_ref() != Some(&new)).then_some((name, new))
                })
                .collect();
            *shown = next;
            (changed, seek)
        };
        if !changed.is_empty() {
            let body = (PLAYER, changed, Vec::<String>::new()).to_variant();
            self.emit("org.freedesktop.DBus.Properties", "PropertiesChanged", &body);
        }
        if let Some(position) = seek {
            self.emit(PLAYER, "Seeked", &(position,).to_variant());
        }
    }

    fn emit(&self, iface: &str, signal: &str, body: &glib::Variant) {
        if let Err(e) = self.conn.emit_signal(None, PATH, iface, signal, Some(body)) {
            meditate_core::log("mpris", &format!("{signal} failed: {e}"));
        }
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        if let Some(owner) = self.owner.take() {
            gio::bus_unown_name(owner);
        }
        for id in self.ids.drain(..) {
            let _ = self.conn.unregister_object(id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use meditate_core::session::{SessionSettings, SessionShape};

    fn start(shape: SessionShape, prep_secs: Option<u32>) -> Session {
        let settings = SessionSettings { shape, prep_secs, ..SessionSettings::default() };
        Session::start(settings, Duration::from_secs(100)).0
    }

    #[test]
    fn play_pause_follows_the_session_buttons() {
        let toggle = |s: &Session, now| shown(s, Duration::from_secs(now), None).expect("live").toggle;
        let mut s = start(SessionShape::TimerCountdown { target_secs: 60 }, None);
        assert_eq!(toggle(&s, 105), ToggleAction::Pause, "running");
        s.pause(Duration::from_secs(110));
        assert_eq!(toggle(&s, 115), ToggleAction::Resume, "paused");
        let prep = start(SessionShape::TimerCountdown { target_secs: 60 }, Some(10));
        assert_eq!(toggle(&prep, 105), ToggleAction::Pause, "preparing");
        s.resume(Duration::from_secs(120));
        let _ = s.tick(Duration::from_secs(200));
        assert_eq!(toggle(&s, 201), ToggleAction::NoOp, "overtime");
    }

    #[test]
    fn the_player_reports_the_session() {
        let get = |shown: &Shown, name: &str| property(shown, name).expect(name);
        let meta = |shown: &Shown| glib::VariantDict::new(Some(&get(shown, "Metadata")));
        let mut s = start(SessionShape::TimerCountdown { target_secs: 1200 }, None);
        let running = shown(&s, Duration::from_secs(412), None).expect("running");
        assert_eq!(get(&running, "PlaybackStatus").str(), Some("Playing"));
        let m = meta(&running);
        assert_eq!(m.lookup::<String>("xesam:title").unwrap().as_deref(), Some("Meditate"));
        assert_eq!(m.lookup::<Vec<String>>("xesam:artist").unwrap(), Some(vec!["14:48 left".to_string()]));
        assert_eq!(m.lookup::<i64>("mpris:length").unwrap(), Some(1_200_000_000));
        assert!(cover_uri().is_some(), "the cache takes the cover");
        let m = meta(&running);
        let cover = m.lookup::<String>("mpris:artUrl").unwrap().expect("a cover");
        assert!(cover.starts_with("file://") && cover.ends_with("/meditate/mpris-cover.svg"), "{cover}");
        assert_eq!(get(&running, "Position").get::<i64>(), Some(312_000_000));
        for off in ["CanGoNext", "CanGoPrevious", "CanSeek"] {
            assert_eq!(get(&running, off).get::<bool>(), Some(false), "{off}");
        }
        assert_eq!(get(&running, "CanPause").get::<bool>(), Some(true));

        s.pause(Duration::from_secs(412));
        let paused = shown(&s, Duration::from_secs(900), None).expect("paused");
        assert_eq!(get(&paused, "PlaybackStatus").str(), Some("Paused"));
        assert_eq!(meta(&paused).lookup::<Vec<String>>("xesam:artist").unwrap(), Some(vec!["Paused, 14:48 left".to_string()]));
        assert_eq!(get(&paused, "CanPlay").get::<bool>(), Some(true));

        s.resume(Duration::from_secs(900));
        let _ = s.tick(Duration::from_secs(1910));
        let over = shown(&s, Duration::from_secs(1921), Some("Morning breath")).expect("overtime");
        assert_eq!(get(&over, "PlaybackStatus").str(), Some("Paused"));
        assert_eq!(meta(&over).lookup::<Vec<String>>("xesam:artist").unwrap(), Some(vec!["Overtime +02:13".to_string()]));
        assert_eq!(meta(&over).lookup::<String>("xesam:title").unwrap().as_deref(), Some("Morning breath"));
        // GNOME drops a player that can't play: overtime keeps it, and
        // play/pause there does nothing (the toggle is NoOp).
        assert_eq!(get(&over, "CanPlay").get::<bool>(), Some(true), "stays in the shade");
        assert_eq!(over.toggle, ToggleAction::NoOp);
        assert_eq!(get(&over, "CanPause").get::<bool>(), Some(false), "nothing to pause in overtime");

        let _ = s.finish_overtime();
        assert_eq!(shown(&s, Duration::from_secs(1930), None), None, "gone once stopped");

        let mut prep = start(SessionShape::TimerCountdown { target_secs: 60 }, Some(10));
        prep.pause(Duration::from_secs(105));
        let prep = shown(&prep, Duration::from_secs(110), None).expect("paused in preparation");
        assert_eq!(get(&prep, "PlaybackStatus").str(), Some("Paused"), "paused while preparing");
    }
    #[test]
    fn only_a_real_change_reaches_the_shell() {
        let mut s = start(SessionShape::TimerCountdown { target_secs: 1200 }, Some(10));
        let preparing = shown(&s, Duration::from_secs(105), None).unwrap();
        let _ = s.tick(Duration::from_secs(111));
        let a = shown(&s, Duration::from_millis(412_000), None).unwrap();
        let b = shown(&s, Duration::from_millis(412_400), None).unwrap();
        assert_eq!(property(&a, "Metadata"), property(&b, "Metadata"), "same line, same metadata");
        assert_eq!(seeked(&a, &b), None, "the position follows the rate");
        assert_eq!(seeked(&preparing, &a), Some(302_000_000), "the bar starts after preparation");
        s.pause(Duration::from_secs(500));
        let paused = shown(&s, Duration::from_secs(600), None).unwrap();
        assert_eq!(seeked(&b, &paused), Some(390_000_000), "a pause stops it");

        let mut same = start(SessionShape::TimerCountdown { target_secs: 60 }, Some(60));
        let late_prep = shown(&same, Duration::from_secs(159), None).unwrap();
        let _ = same.tick(Duration::from_secs(160));
        let session = shown(&same, Duration::from_secs(161), None).unwrap();
        assert_eq!(seeked(&late_prep, &session), Some(1_000_000), "a preparation as long as the session");
    }
}
