//! Typed UUID wrappers for the workspace's seven entity types.
//!
//! Every cross-device row identity used to travel as a bare
//! `String`, which made it possible — and historically did happen
//! — to pass a `BellSound` UUID where an `IntervalBell` UUID was
//! expected, or vice versa. The Tier-0 path-traversal bug
//! (`bell_sound_insert` with `target_id = "../../../etc/passwd"`)
//! traces back to UUIDs being stringly-typed at the API surface.
//!
//! Each newtype below is:
//!
//! - `#[serde(transparent)]` so the wire format is unchanged:
//!   sync event payloads, preset_config JSON, on-disk SQL columns
//!   all continue to see a bare quoted string.
//! - `ToSql + FromSql` so rusqlite `params!` / `row.get` work as
//!   if the field were still `String`.
//! - `From<String>` / `From<&str>` for ergonomic construction
//!   at boundaries that legitimately carry untyped strings
//!   (DB reads, JSON payload extraction, settings table values).
//! - `AsRef<str> + Display + as_str()` so call sites that need
//!   the raw value (filesystem paths, settings keys, etc.) can
//!   get it without a `.0` dance.
//!
//! Pure tagging — no validation. A `LabelUuid` doesn't check that
//! the string is a well-formed UUID, only that it isn't structurally
//! confused with a `BellSoundUuid`. UUID well-formedness on
//! untrusted input lives in `target_id_is_well_formed_for`.

macro_rules! entity_uuid {
    ($name:ident, $doc:literal) => {
        #[doc = $doc]
        #[derive(Debug, Clone, Hash, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
        #[serde(transparent)]
        pub struct $name(pub String);

        impl $name {
            /// Construct from anything `Into<String>`. Accepts both
            /// `&str` and `String`; cheaper than `From::<&str>` when
            /// the caller already owns a `String` (no extra copy).
            pub fn new(s: impl Into<String>) -> Self {
                Self(s.into())
            }

            /// Borrow the underlying string. Equivalent to `AsRef<str>`,
            /// kept as an inherent method so call sites don't need
            /// to import the trait.
            pub fn as_str(&self) -> &str {
                &self.0
            }

            /// `true` iff the wrapped string is empty. Used as the
            /// "uuid is unset" sentinel in a few wire-format paths
            /// (preset_config, settings) — keeps the convention
            /// explicit at the call site.
            pub fn is_empty(&self) -> bool {
                self.0.is_empty()
            }
        }

        impl From<String> for $name {
            fn from(s: String) -> Self { Self(s) }
        }

        impl From<&str> for $name {
            fn from(s: &str) -> Self { Self(s.to_string()) }
        }

        impl AsRef<str> for $name {
            fn as_ref(&self) -> &str { &self.0 }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl rusqlite::ToSql for $name {
            fn to_sql(&self) -> rusqlite::Result<rusqlite::types::ToSqlOutput<'_>> {
                self.0.to_sql()
            }
        }

        impl rusqlite::types::FromSql for $name {
            fn column_result(
                value: rusqlite::types::ValueRef<'_>,
            ) -> rusqlite::types::FromSqlResult<Self> {
                String::column_result(value).map($name)
            }
        }
    };
}

entity_uuid!(
    LabelUuid,
    "Cross-device identity for a `Label` row. Stored in `labels.uuid`, referenced by `presets.config_json.label.uuid`."
);
entity_uuid!(
    BellSoundUuid,
    "Cross-device identity for a `BellSound` row. Stored in `bell_sounds.uuid`, referenced by `interval_bells.sound_uuid`, `box_breath_phases.sound_uuid`, and per-mode settings (`starting_bell_sound`, `end_bell_sound`)."
);
entity_uuid!(
    VibrationPatternUuid,
    "Cross-device identity for a `VibrationPattern` row. Stored in `vibration_patterns.uuid`, referenced by `interval_bells.vibration_pattern_uuid` and `box_breath_phases.pattern_uuid`."
);
entity_uuid!(
    PresetUuid,
    "Cross-device identity for a `Preset` row. Stored in `presets.uuid`; events use it as `target_id`."
);
entity_uuid!(
    GuidedFileUuid,
    "Cross-device identity for a `GuidedFile` row. Stored in `guided_files.uuid`, referenced by `sessions.guided_file_uuid` and `session_in_progress.guided_file_uuid`."
);
entity_uuid!(
    IntervalBellUuid,
    "Cross-device identity for an `IntervalBell` row. Stored in `interval_bells.uuid`; events use it as `target_id`."
);
entity_uuid!(
    SessionUuid,
    "Cross-device identity for a `Session` row. Stored in `sessions.uuid`; events use it as `target_id`. UUIDs are minted on insert, never user-supplied."
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distinct_types_dont_unify_via_assignment() {
        // Compile-time check: this file compiles iff the types are
        // separate. The test body itself is trivial — the win is the
        // negative test that should NOT compile (commented out below).
        let a: LabelUuid = "label-1".into();
        let b: BellSoundUuid = "bell-1".into();
        assert_eq!(a.as_str(), "label-1");
        assert_eq!(b.as_str(), "bell-1");
        // The following would not compile (correct):
        // let _: LabelUuid = b;
    }

    #[test]
    fn serde_is_transparent_to_plain_string() {
        let u = LabelUuid::new("550e8400-e29b-41d4-a716-446655440000");
        let json = serde_json::to_string(&u).unwrap();
        assert_eq!(json, "\"550e8400-e29b-41d4-a716-446655440000\"");
        let back: LabelUuid = serde_json::from_str(&json).unwrap();
        assert_eq!(back, u);
    }

    #[test]
    fn round_trips_through_sqlite() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute("CREATE TABLE t (u TEXT NOT NULL)", []).unwrap();
        let u = BellSoundUuid::new("abc-123");
        conn.execute("INSERT INTO t (u) VALUES (?1)", rusqlite::params![u]).unwrap();
        let got: BellSoundUuid = conn
            .query_row("SELECT u FROM t", [], |row| row.get(0))
            .unwrap();
        assert_eq!(got, u);
    }
}
