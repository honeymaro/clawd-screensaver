//! What Clawd Saver lets you choose: how much history the counter adds up, and
//! what Clawd is doing while it does.
//!
//! Stored in `%LOCALAPPDATA%\clawd-saver\settings.json`, beside the cache and
//! the log rather than in the registry, so everything the program owns sits in
//! one directory and `install.ps1 -Uninstall` removing that directory takes the
//! lot.

use std::path::PathBuf;

/// How far back the figure on screen reaches. Every window ends today.
///
/// Two families, because they answer different questions. The calendar ones —
/// today, this week, this month — are what a billing period looks like, and are
/// what you want when the question is "how much have I spent *so far*". They
/// have the property that they drop to almost nothing the moment the period
/// rolls over, which is a feature when you are tracking a budget and a nuisance
/// when you are not.
///
/// The rolling ones answer "how much do I spend, lately" and never reset. On the
/// 1st of a month, `MonthToDate` reads a few dollars while `Last30Days` still
/// reads the real rate.
///
/// The week's first day is not fixed here — it comes from the user's Windows
/// locale, which is the only defensible answer to a question with no universal
/// one.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Period {
    Today,
    WeekToDate,
    MonthToDate,
    Last7Days,
    Last30Days,
}

impl Period {
    /// In the order the settings dialog lists them: calendar first, then
    /// rolling.
    pub const ALL: [Period; 5] = [
        Period::Today,
        Period::WeekToDate,
        Period::MonthToDate,
        Period::Last7Days,
        Period::Last30Days,
    ];

    /// Written into settings.json and used as part of the cache key. Stable:
    /// changing one of these silently invalidates every cache already on disk.
    pub fn key(self) -> &'static str {
        match self {
            Period::Today => "1d",
            Period::WeekToDate => "wtd",
            Period::MonthToDate => "mtd",
            Period::Last7Days => "7d",
            Period::Last30Days => "30d",
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|p| p.key() == key)
    }
}

/// Which scene the page draws below the counter.
///
/// Each one is Clawd doing something that costs money: swinging at ore, feeding
/// a furnace, minding a rack of servers, waiting on a jetty for something to
/// bite, reading a receipt as it prints, stamping parcels off a belt, aiming a
/// dish at whatever is on the other end, or cutting fruit out of the air. The
/// keys are what `ui.html`'s scene registry is indexed by, so they are a
/// contract with the page rather than just a storage format — and `saver.rs`
/// has a test that holds the two together.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Scene {
    Mine,
    Forge,
    Rack,
    Dock,
    Printer,
    Belt,
    Uplink,
    Dojo,
}

impl Scene {
    /// In the order the settings dialog lists them, which is the order they were
    /// added: moving one would move a row under someone's cursor for no reason.
    pub const ALL: [Scene; 8] = [
        Scene::Mine,
        Scene::Forge,
        Scene::Rack,
        Scene::Dock,
        Scene::Printer,
        Scene::Belt,
        Scene::Uplink,
        Scene::Dojo,
    ];

    pub fn key(self) -> &'static str {
        match self {
            Scene::Mine => "mine",
            Scene::Forge => "forge",
            Scene::Rack => "rack",
            Scene::Dock => "dock",
            Scene::Printer => "printer",
            Scene::Belt => "belt",
            Scene::Uplink => "uplink",
            Scene::Dojo => "dojo",
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.key() == key)
    }
}

/// What was chosen, which is not always a scene: `Random` is resolved once per
/// launch rather than stored, so the answer differs each time the saver starts.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SceneChoice {
    Random,
    One(Scene),
}

impl SceneChoice {
    pub const ALL: [SceneChoice; 9] = [
        SceneChoice::Random,
        SceneChoice::One(Scene::Mine),
        SceneChoice::One(Scene::Forge),
        SceneChoice::One(Scene::Rack),
        SceneChoice::One(Scene::Dock),
        SceneChoice::One(Scene::Printer),
        SceneChoice::One(Scene::Belt),
        SceneChoice::One(Scene::Uplink),
        SceneChoice::One(Scene::Dojo),
    ];

    pub fn key(self) -> &'static str {
        match self {
            SceneChoice::Random => "random",
            SceneChoice::One(s) => s.key(),
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|c| c.key() == key)
    }

    /// Turn a choice into the scene this launch will actually draw.
    ///
    /// Must be called exactly once per launch and the result shared, not called
    /// per display: the saver builds one surface per monitor, and a fresh roll
    /// for each would put a different scene on every screen.
    pub fn resolve(self) -> Scene {
        match self {
            SceneChoice::One(s) => s,
            // A `RandomState` is seeded by the OS and then bumped per use, so
            // hashing nothing with a fresh one is a different number every
            // time. Enough for picking one of eight, and it costs no dependency
            // and no stored seed. Eight divides 2^64, so `%` is exactly uniform
            // here and the result rests on the low three bits alone; at a count
            // that is not a power of two the bias would be about one part in
            // 2^61, which would still not be worth correcting.
            SceneChoice::Random => {
                use std::hash::{BuildHasher, Hasher};
                let n = std::collections::hash_map::RandomState::new()
                    .build_hasher()
                    .finish();
                Scene::ALL[(n % Scene::ALL.len() as u64) as usize]
            }
        }
    }
}

/// Everything the dialog can set, read and written as one record.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Settings {
    pub period: Period,
    pub scene: SceneChoice,
}

impl Default for Settings {
    /// What the saver did before there was anything to choose: today's spend,
    /// Clawd at the ore. An install that upgrades and never opens the dialog
    /// behaves exactly as it did.
    fn default() -> Self {
        Settings {
            period: Period::Today,
            scene: SceneChoice::One(Scene::Mine),
        }
    }
}

fn path() -> Option<PathBuf> {
    crate::usage::app_dir().map(|d| d.join("settings.json"))
}

/// Defaults when the file is absent, unreadable or not understood, and per
/// field: a settings.json written before scenes existed still selects its
/// period, and only the missing half falls back.
///
/// Falling back quietly would be its own bug, though: someone who chose 30 days
/// and is being shown one day has no way to tell that from having chosen one
/// day. Everything except a missing file — which is just a fresh install —
/// leaves a line in the log.
pub fn load() -> Settings {
    let Some(path) = path() else {
        crate::usage::log("settings      LOCALAPPDATA is not set, using defaults");
        return Settings::default();
    };
    match std::fs::read_to_string(&path) {
        Ok(raw) => parse(&raw).unwrap_or_else(|| {
            crate::usage::log(&format!(
                "settings      {} is not readable as settings, using defaults",
                path.display()
            ));
            Settings::default()
        }),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Settings::default(),
        Err(e) => {
            crate::usage::log(&format!("settings      unreadable ({e}), using defaults"));
            Settings::default()
        }
    }
}

/// `None` when the file is not a JSON object — including when it is valid JSON
/// of some other shape, because an array is no more a settings record than a
/// stray sentence is, and the difference is worth a line in the log.
///
/// Within an object, a field that is missing or names something this build does
/// not know falls back on its own, so one bad field cannot discard the other.
fn parse(raw: &str) -> Option<Settings> {
    // JSON has no byte order mark and serde refuses one, but this is Windows:
    // Notepad writes UTF-8 with a BOM by default, so anyone who opens this file
    // to change a word by hand hands it back with three bytes on the front and
    // loses every setting in it. Costs one call to be kind about that.
    let raw = raw.trim_start_matches('\u{feff}');
    let v: serde_json::Value = serde_json::from_str(raw).ok()?;
    v.as_object()?;
    let field = |name: &str| v.get(name).and_then(serde_json::Value::as_str);
    let d = Settings::default();
    Some(Settings {
        period: field("period").and_then(Period::from_key).unwrap_or(d.period),
        scene: field("scene").and_then(SceneChoice::from_key).unwrap_or(d.scene),
    })
}

/// The on-disk form. A function rather than a `format!` inside `save`, so the
/// round-trip test reads what `save` writes instead of a second copy of this
/// string — a copy would agree with itself while the file said something else.
fn body(s: Settings) -> String {
    format!(
        "{{\"period\":\"{}\",\"scene\":\"{}\"}}",
        s.period.key(),
        s.scene.key()
    )
}

pub fn save(s: Settings) -> std::io::Result<()> {
    let path = path().ok_or_else(|| std::io::Error::other("LOCALAPPDATA is not set"))?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    // Written to one side and renamed in, the same way the usage cache is. A
    // saver or a detached refresher can be reading this while the dialog writes
    // it, and a torn read would not just fall back to defaults — since load()
    // logs anything it cannot parse, it would also leave a misleading line
    // blaming the file.
    let tmp = path.with_extension(format!("tmp.{}", std::process::id()));
    std::fs::write(&tmp, body(s))?;
    std::fs::rename(&tmp, &path).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_key_round_trips() {
        // The keys are a storage format. A typo here is a settings file that
        // silently reverts to a default.
        for p in Period::ALL {
            assert_eq!(Period::from_key(p.key()), Some(p));
        }
        for s in Scene::ALL {
            assert_eq!(Scene::from_key(s.key()), Some(s));
        }
        for c in SceneChoice::ALL {
            assert_eq!(SceneChoice::from_key(c.key()), Some(c));
        }
    }

    #[test]
    fn the_stored_keys_are_the_documented_ones() {
        // Spelled out rather than derived, because these are a storage format:
        // a settings.json and a cache file on disk both name them, and changing
        // one silently discards what is stored under the old spelling.
        let periods: Vec<_> = Period::ALL.iter().map(|p| p.key()).collect();
        assert_eq!(periods, ["1d", "wtd", "mtd", "7d", "30d"]);
        let scenes: Vec<_> = SceneChoice::ALL.iter().map(|c| c.key()).collect();
        assert_eq!(
            scenes,
            ["random", "mine", "forge", "rack", "dock", "printer", "belt", "uplink", "dojo"]
        );
    }

    #[test]
    fn a_stored_choice_is_read_back() {
        let s = parse(r#"{"period":"7d","scene":"dock"}"#).unwrap();
        assert_eq!(s.period, Period::Last7Days);
        assert_eq!(s.scene, SceneChoice::One(Scene::Dock));
    }

    #[test]
    fn a_file_written_before_scenes_existed_keeps_its_period() {
        // The half that is missing falls back; the half that is there must not.
        let s = parse(r#"{"period":"30d"}"#).unwrap();
        assert_eq!(s.period, Period::Last30Days);
        assert_eq!(s.scene, Settings::default().scene);
    }

    #[test]
    fn one_unrecognised_field_does_not_discard_the_other() {
        let s = parse(r#"{"period":"wtd","scene":"volcano"}"#).unwrap();
        assert_eq!(s.period, Period::WeekToDate);
        assert_eq!(s.scene, Settings::default().scene);

        let s = parse(r#"{"period":"1y","scene":"forge"}"#).unwrap();
        assert_eq!(s.period, Settings::default().period);
        assert_eq!(s.scene, SceneChoice::One(Scene::Forge));
    }

    #[test]
    fn anything_that_is_not_json_falls_back_whole() {
        for raw in ["", "not json", "[]", "\"just a string\""] {
            assert!(parse(raw).is_none(), "{raw:?} should not parse");
        }
        // An empty object is valid JSON, so it parses — into the defaults.
        assert_eq!(parse("{}"), Some(Settings::default()));
        // So does an object whose fields are the right names and the wrong
        // type. `as_str` says no to both, and each falls back on its own.
        assert_eq!(parse(r#"{"period":7,"scene":true}"#), Some(Settings::default()));
    }

    #[test]
    fn a_file_saved_by_notepad_still_parses() {
        // Found by writing this file from PowerShell, whose `-Encoding utf8`
        // adds the mark: the whole record was refused and the log blamed the
        // file. Notepad does the same thing by default.
        let s = parse("\u{feff}{\"period\":\"7d\",\"scene\":\"dojo\"}").unwrap();
        assert_eq!(s.period, Period::Last7Days);
        assert_eq!(s.scene, SceneChoice::One(Scene::Dojo));
    }

    #[test]
    fn what_save_writes_is_what_parse_reads() {
        // Through `body`, which is the function `save` writes with — not a copy
        // of its format string, which would agree with itself for ever.
        for period in Period::ALL {
            for scene in SceneChoice::ALL {
                let s = Settings { period, scene };
                assert_eq!(parse(&body(s)), Some(s));
            }
        }
    }

    #[test]
    fn a_fixed_choice_resolves_to_itself() {
        for s in Scene::ALL {
            assert_eq!(SceneChoice::One(s).resolve(), s);
        }
    }

    #[test]
    fn random_does_not_answer_the_same_thing_every_time() {
        // Not a distribution test — just that it is not a constant, which is
        // what a mis-seeded hasher would give.
        let seen: std::collections::HashSet<_> =
            (0..300).map(|_| SceneChoice::Random.resolve()).collect();
        assert!(seen.len() > 1, "Random always resolved to {seen:?}");
    }

    #[test]
    fn random_can_reach_every_scene() {
        let seen: std::collections::HashSet<_> =
            (0..4000).map(|_| SceneChoice::Random.resolve()).collect();
        for s in Scene::ALL {
            assert!(seen.contains(&s), "Random never produced {s:?}");
        }
    }
}
