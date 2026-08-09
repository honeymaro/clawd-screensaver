//! The one thing Clawd Saver lets you choose: how much history the counter adds
//! up.
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

fn path() -> Option<PathBuf> {
    crate::usage::app_dir().map(|d| d.join("settings.json"))
}

/// Today when the file is absent, unreadable or not understood. A settings file
/// that has gone bad should leave the counter working rather than blank, and
/// today is what the saver did before the setting existed.
///
/// Falling back quietly would be its own bug, though: someone who chose 30 days
/// and is being shown one day has no way to tell that from having chosen one
/// day. Everything except a missing file — which is just a fresh install —
/// leaves a line in the log.
pub fn load() -> Period {
    let Some(path) = path() else {
        crate::usage::log("settings      LOCALAPPDATA is not set, using today");
        return Period::Today;
    };
    match std::fs::read_to_string(&path) {
        Ok(raw) => parse(&raw).unwrap_or_else(|| {
            crate::usage::log(&format!(
                "settings      {} holds no period this build knows, using today",
                path.display()
            ));
            Period::Today
        }),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Period::Today,
        Err(e) => {
            crate::usage::log(&format!("settings      unreadable ({e}), using today"));
            Period::Today
        }
    }
}

fn parse(raw: &str) -> Option<Period> {
    let v: serde_json::Value = serde_json::from_str(raw).ok()?;
    Period::from_key(v.get("period")?.as_str()?)
}

pub fn save(period: Period) -> std::io::Result<()> {
    let path = path().ok_or_else(|| std::io::Error::other("LOCALAPPDATA is not set"))?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    // Written to one side and renamed in, the same way the usage cache is. A
    // saver or a detached refresher can be reading this while the dialog writes
    // it, and a torn read would not just fall back to today — since load() now
    // logs anything it cannot parse, it would also leave a misleading line
    // blaming the file.
    let tmp = path.with_extension(format!("tmp.{}", std::process::id()));
    std::fs::write(&tmp, format!("{{\"period\":\"{}\"}}", period.key()))?;
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
        // silently reverts to today.
        for p in Period::ALL {
            assert_eq!(Period::from_key(p.key()), Some(p));
        }
    }

    #[test]
    fn the_stored_keys_are_the_documented_ones() {
        // Spelled out rather than derived, because these are a storage format:
        // a settings.json and a cache file on disk both name them, and changing
        // one silently discards what is stored under the old spelling.
        let keys: Vec<_> = Period::ALL.iter().map(|p| p.key()).collect();
        assert_eq!(keys, ["1d", "wtd", "mtd", "7d", "30d"]);
    }

    #[test]
    fn a_stored_choice_is_read_back() {
        assert_eq!(parse(r#"{"period":"7d"}"#), Some(Period::Last7Days));
        assert_eq!(parse(r#"{"period":"wtd"}"#), Some(Period::WeekToDate));
        assert_eq!(
            parse(r#"{"period":"30d","extra":1}"#),
            Some(Period::Last30Days)
        );
    }

    #[test]
    fn the_keys_that_predate_the_calendar_periods_still_mean_what_they_did() {
        // A settings.json written before this-week and this-month existed must
        // keep selecting the rolling window it selected then, not be quietly
        // re-pointed at the calendar one with a similar name.
        assert_eq!(parse(r#"{"period":"1d"}"#), Some(Period::Today));
        assert_eq!(parse(r#"{"period":"7d"}"#), Some(Period::Last7Days));
        assert_eq!(parse(r#"{"period":"30d"}"#), Some(Period::Last30Days));
    }

    #[test]
    fn anything_unrecognised_falls_back_rather_than_failing() {
        // load() turns each of these into Today. None of them may panic.
        for raw in [
            "",
            "{}",
            "not json",
            r#"{"period":"1y"}"#,
            r#"{"period":7}"#,
        ] {
            assert_eq!(parse(raw), None, "{raw:?} should not parse");
        }
    }

    #[test]
    fn what_save_writes_is_what_parse_reads() {
        for p in Period::ALL {
            let body = format!("{{\"period\":\"{}\"}}", p.key());
            assert_eq!(parse(&body), Some(p));
        }
    }
}
