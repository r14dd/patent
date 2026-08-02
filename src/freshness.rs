//! Recency of a match — normalising each registry's "last updated" timestamp
//! and turning it into the staleness signal shown in the UI.
//!
//! Registries disagree on how to express a date: crates.io and GitHub send
//! RFC 3339 strings (with wildly varying sub-second precision), Maven sends
//! epoch milliseconds, AUR and Artifact Hub send epoch seconds, and pkg.go.dev
//! only renders a human date into its HTML. Adapters funnel all of these
//! through the `from_*` constructors here so that [`Match::last_updated`] is
//! always one shape: whole-second RFC 3339 in UTC, e.g. `2026-05-15T06:13:41Z`.
//!
//! Every constructor returns [`Option`] and never [`Result`]: a registry that
//! changes its timestamp format must degrade to "no date known", not take out
//! the whole source. That mirrors the crate-wide rule that one source's trouble
//! never fails the run.
//!
//! [`Match::last_updated`]: crate::model::Match::last_updated

use serde::Deserialize;
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

/// Deserialises a registry's date field, degrading a *type* change — a string
/// where a number used to be, a `null`, an object — to `None` instead of
/// failing the entire response.
///
/// `#[serde(default)]` alone is not enough: it only covers an *absent* field.
/// A registry that switched `ts` from a number to a string would otherwise take
/// its adapter down with a deserialisation error, turning a cosmetic upstream
/// change into a whole source going dark. Pair the two on every date field:
///
/// ```ignore
/// #[serde(default, deserialize_with = "crate::freshness::lenient")]
/// ts: Option<i64>,
/// ```
pub fn lenient<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::de::DeserializeOwned,
{
    let raw = serde_json::Value::deserialize(deserializer)?;
    Ok(T::deserialize(raw).ok())
}

/// A match is called stale once it has gone this long without an update.
///
/// Two years is deliberately generous: plenty of finished, still-perfectly-good
/// tools sit untouched for a year or more, and flagging those would cry wolf.
pub const STALE_AFTER_YEARS: i32 = 2;

/// How long ago a match was last updated, ready for display.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Age {
    /// Human-readable label, e.g. `"3 years ago"`, `"5 months ago"`, `"today"`.
    pub label: String,
    /// Whether the match has gone [`STALE_AFTER_YEARS`] without an update.
    pub stale: bool,
}

/// The current time, for passing to [`age`].
///
/// [`age`] takes `now` as an argument rather than reading the clock itself so
/// that the staleness boundary is directly testable; this is the convenience
/// callers use in production.
pub fn now() -> OffsetDateTime {
    OffsetDateTime::now_utc()
}

/// Parses an RFC 3339 timestamp, at any sub-second precision.
pub fn parse(ts: &str) -> Option<OffsetDateTime> {
    OffsetDateTime::parse(ts, &Rfc3339).ok()
}

/// Renders a timestamp in the canonical form stored on [`Match::last_updated`]:
/// whole-second RFC 3339, UTC.
///
/// [`Match::last_updated`]: crate::model::Match::last_updated
fn canonical(dt: OffsetDateTime) -> Option<String> {
    dt.to_offset(time::UtcOffset::UTC)
        .replace_nanosecond(0)
        .ok()?
        .format(&Rfc3339)
        .ok()
}

/// Normalises a registry-supplied RFC 3339 string (crates.io, npm, GitHub, Hex).
///
/// Returns `None` for anything unparseable, so a format change downgrades the
/// match to "no date known" rather than failing its source.
pub fn from_rfc3339(ts: &str) -> Option<String> {
    canonical(parse(ts)?)
}

/// Normalises an epoch-seconds timestamp (AUR, Artifact Hub).
pub fn from_unix_secs(secs: i64) -> Option<String> {
    canonical(OffsetDateTime::from_unix_timestamp(secs).ok()?)
}

/// Normalises an epoch-milliseconds timestamp (Maven).
pub fn from_unix_millis(millis: i64) -> Option<String> {
    canonical(OffsetDateTime::from_unix_timestamp_nanos(millis as i128 * 1_000_000).ok()?)
}

/// Normalises pkg.go.dev's rendered publication date, e.g. `"Feb 28, 2026"`.
///
/// This is the only source whose date is scraped from human-facing HTML rather
/// than read from an API field, so it is also the likeliest to drift — hence
/// the same `None`-on-failure contract as everything else here.
pub fn from_go_date(text: &str) -> Option<String> {
    let cleaned = text.trim().replace(',', "");
    let mut parts = cleaned.split_whitespace();
    let month = match parts.next()?.to_ascii_lowercase().as_str() {
        "jan" => time::Month::January,
        "feb" => time::Month::February,
        "mar" => time::Month::March,
        "apr" => time::Month::April,
        "may" => time::Month::May,
        "jun" => time::Month::June,
        "jul" => time::Month::July,
        "aug" => time::Month::August,
        "sep" => time::Month::September,
        "oct" => time::Month::October,
        "nov" => time::Month::November,
        "dec" => time::Month::December,
        _ => return None,
    };
    let day: u8 = parts.next()?.parse().ok()?;
    let year: i32 = parts.next()?.parse().ok()?;
    let date = time::Date::from_calendar_date(year, month, day).ok()?;
    canonical(date.midnight().assume_utc())
}

/// Describes how long ago `ts` was, relative to `now`.
///
/// `now` is a parameter, not a call to the system clock, so that the
/// [`STALE_AFTER_YEARS`] boundary can be tested at an exact offset instead of
/// drifting with the date the suite happens to run on.
///
/// Returns `None` if `ts` is not a timestamp this crate wrote. A timestamp in
/// the future — clock skew, or a registry publishing a post-dated release —
/// reads as `"today"` and is never stale.
pub fn age(ts: &str, now: OffsetDateTime) -> Option<Age> {
    let then = parse(ts)?;
    let days = (now - then).whole_days();

    // Completed calendar years, deliberately not `days / 365.2425`: five
    // calendar years spanning a leap day is 1826 days, which a mean-year
    // divisor floors to four, so an exact five-year-old release displayed as
    // "4 years ago". Counting anniversaries has no such rounding edge.
    let mut years = now.year() - then.year();
    if (now.month() as u8, now.day()) < (then.month() as u8, then.day()) {
        years -= 1;
    }

    let label = match days {
        d if d <= 0 => "today".to_string(),
        1 => "yesterday".to_string(),
        d if d < 31 => format!("{d} days ago"),
        // Capped at 11 so the 360–364 day range can't read "12 months ago"
        // while still being under a year.
        _ if years < 1 => match (days / 30).clamp(1, 11) {
            1 => "1 month ago".to_string(),
            m => format!("{m} months ago"),
        },
        _ => match years {
            1 => "1 year ago".to_string(),
            y => format!("{y} years ago"),
        },
    };

    Some(Age {
        label,
        stale: years >= STALE_AFTER_YEARS,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(ts: &str) -> OffsetDateTime {
        parse(ts).expect("test timestamp must parse")
    }

    #[test]
    fn normalises_every_registry_format_to_the_same_shape() {
        // The real values probed from each live API, so this pins the actual
        // formats in play rather than an idealised version of them.
        assert_eq!(
            from_rfc3339("2026-05-15T06:13:41.215606Z").as_deref(), // crates.io
            Some("2026-05-15T06:13:41Z")
        );
        assert_eq!(
            from_rfc3339("2026-07-21T15:41:28.716Z").as_deref(), // npm
            Some("2026-07-21T15:41:28Z")
        );
        assert_eq!(
            from_rfc3339("2026-07-31T18:49:43Z").as_deref(), // GitHub
            Some("2026-07-31T18:49:43Z")
        );
        assert_eq!(
            from_unix_millis(1_750_337_811_233).as_deref(), // Maven
            Some("2025-06-19T12:56:51Z")
        );
        assert_eq!(
            from_unix_secs(1_778_477_360).as_deref(), // AUR
            Some("2026-05-11T05:29:20Z")
        );
        assert_eq!(
            from_go_date("Feb 28, 2026").as_deref(), // pkg.go.dev
            Some("2026-02-28T00:00:00Z")
        );
    }

    #[test]
    fn non_utc_offsets_are_converted_rather_than_truncated() {
        assert_eq!(
            from_rfc3339("2026-05-15T06:13:41+02:00").as_deref(),
            Some("2026-05-15T04:13:41Z")
        );
    }

    #[test]
    fn malformed_input_yields_none_and_never_panics() {
        for junk in [
            "",
            "   ",
            "not a date",
            "2026-13-45T99:99:99Z",
            "1750337811233",
            "Feb 2026",
            "Smarch 3, 2026",
            "Feb 30, 2026",
        ] {
            assert_eq!(from_rfc3339(junk), None, "from_rfc3339({junk:?})");
            assert_eq!(from_go_date(junk), None, "from_go_date({junk:?})");
            assert_eq!(age(junk, now()), None, "age({junk:?})");
        }
        // Out-of-range epochs are rejected, not wrapped.
        assert_eq!(from_unix_secs(i64::MAX), None);
        assert_eq!(from_unix_millis(i64::MAX), None);
    }

    #[test]
    fn labels_read_naturally_across_the_scale() {
        let now = at("2026-08-02T00:00:00Z");
        let cases = [
            ("2026-08-02T00:00:00Z", "today"),
            ("2026-08-01T00:00:00Z", "yesterday"),
            ("2026-07-20T00:00:00Z", "13 days ago"),
            ("2026-06-02T00:00:00Z", "2 months ago"),
            ("2026-06-25T00:00:00Z", "1 month ago"),
            ("2025-09-02T00:00:00Z", "11 months ago"),
            ("2025-08-02T00:00:00Z", "1 year ago"),
            ("2021-08-02T00:00:00Z", "5 years ago"),
            // Boundaries that have bitten: 365 days is a hair under one *mean*
            // Gregorian year and floored to "0 years ago"; 360-364 days landed
            // in the months branch and read "12 months ago".
            ("2025-08-07T00:00:00Z", "11 months ago"),
            ("2025-08-03T00:00:00Z", "11 months ago"),
            ("2025-08-01T00:00:00Z", "1 year ago"),
            ("2024-08-03T00:00:00Z", "1 year ago"),
        ];
        for (ts, want) in cases {
            assert_eq!(age(ts, now).unwrap().label, want, "age({ts})");
        }
    }

    #[test]
    fn stale_boundary_sits_exactly_at_the_two_year_anniversary() {
        let now = at("2026-08-02T00:00:00Z");
        // One day short of the second anniversary.
        assert!(!age("2024-08-03T00:00:00Z", now).unwrap().stale);
        // The anniversary itself already counts.
        assert!(age("2024-08-02T00:00:00Z", now).unwrap().stale);
        assert!(age("2024-08-01T00:00:00Z", now).unwrap().stale);
    }

    #[test]
    fn whole_calendar_years_are_not_lost_to_leap_days() {
        // Five calendar years across the 2024 leap day is 1826 days; dividing
        // by a mean year floors that to four and reads "4 years ago".
        let now = at("2026-08-02T00:00:00Z");
        assert_eq!(
            age("2021-08-02T00:00:00Z", now).unwrap().label,
            "5 years ago"
        );
        // Same shape one day either side of the anniversary.
        assert_eq!(
            age("2021-08-01T00:00:00Z", now).unwrap().label,
            "5 years ago"
        );
        assert_eq!(
            age("2021-08-03T00:00:00Z", now).unwrap().label,
            "4 years ago"
        );
    }

    #[test]
    fn future_timestamps_are_not_stale() {
        let now = at("2026-08-02T00:00:00Z");
        let a = age("2027-01-01T00:00:00Z", now).unwrap();
        assert_eq!(a.label, "today");
        assert!(!a.stale, "clock skew must never read as abandonment");
    }
}
