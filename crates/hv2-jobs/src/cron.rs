//! Numeric five-field cron grammar and bounded UTC/timezone occurrence search.
use crate::{JobError, Result};
use chrono::{Datelike, LocalResult, NaiveDate, TimeZone};
use std::str::FromStr;

/// Minute, hour, day-of-month, month, day-of-week numeric selectors.
/// Accepts `*`, numbers, inclusive ascending ranges, comma lists and
/// positive `/step` on `*` or ranges. Sunday is 0; weekday 7 is rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CronExpression {
    pub(crate) masks: [u64; 5],
    pub(crate) day_of_month_wildcard: bool,
    pub(crate) day_of_week_wildcard: bool,
}

impl CronExpression {
    fn selected(&self, field: usize, value: u32) -> bool {
        self.masks[field] & (1_u64 << value) != 0
    }

    fn matches_date(&self, date: NaiveDate) -> bool {
        let dom = self.selected(2, date.day());
        let dow = self.selected(4, date.weekday().num_days_from_sunday());
        let day = if !self.day_of_month_wildcard && !self.day_of_week_wildcard {
            dom || dow
        } else {
            dom && dow
        };
        self.selected(3, date.month()) && day
    }

    /// First local cron minute at or after `time_ms` in an IANA timezone.
    /// Gaps are skipped; both fold occurrences are eligible in UTC order.
    /// Search is bounded to a 400-year civil-calendar horizon. Supported UTC
    /// years are 1970-9999; timezone rules come from the locked chrono-tz build.
    pub fn at_or_after_in_timezone(&self, time_ms: u64, timezone: &str) -> Result<Option<u64>> {
        self.search_timezone(time_ms, timezone, true)
    }

    /// Latest local cron minute at or before the supplied UTC timestamp.
    pub fn at_or_before_in_timezone(&self, time_ms: u64, timezone: &str) -> Result<Option<u64>> {
        self.search_timezone(time_ms, timezone, false)
    }

    fn search_timezone(&self, time_ms: u64, timezone: &str, forward: bool) -> Result<Option<u64>> {
        let timezone: chrono_tz::Tz = timezone
            .parse()
            .map_err(|_| JobError::InvalidSpec("unknown cron timezone".into()))?;
        let signed = i64::try_from(time_ms).map_err(|_| invalid_time())?;
        let start = chrono::DateTime::from_timestamp_millis(signed).ok_or_else(invalid_time)?;
        if !(1970..=9999).contains(&start.year()) {
            return Err(invalid_time());
        }
        if forward && timezone == chrono_tz::UTC {
            // Preserve the named-zone range contract while avoiding civil
            // ambiguity enumeration for a timezone that has no transitions.
            return Ok(self.at_or_after_utc(time_ms)?.filter(|candidate| {
                chrono::DateTime::from_timestamp_millis(*candidate as i64)
                    .is_some_and(|value| value.year() <= 9999)
            }));
        }
        let local_date = start.with_timezone(&timezone).date_naive();
        // UTC offsets are strictly less than a day in magnitude. Starting two
        // civil days earlier covers date-crossing backward transitions.
        let step = |date: NaiveDate| {
            if forward {
                date.succ_opt()
            } else {
                date.pred_opt()
            }
        };
        let mut date = if forward {
            local_date.pred_opt().and_then(|d| d.pred_opt())
        } else {
            local_date.succ_opt().and_then(|d| d.succ_opt())
        }
        .ok_or_else(invalid_time)?;
        let mut best: Option<u64> = None;
        for _ in 0..=146_102 {
            let boundary = date
                .and_hms_opt(0, 0, 0)
                .expect("midnight")
                .and_utc()
                .timestamp_millis();
            if best.is_some_and(|candidate| {
                if forward {
                    boundary - 86_400_000 > candidate as i64
                } else {
                    boundary + 172_800_000 < candidate as i64
                }
            }) {
                return Ok(best);
            }
            if (forward && date.year() > 9999) || (!forward && date.year() < 1969) {
                return Ok(best);
            }
            if (1969..=9999).contains(&date.year()) && self.matches_date(date) {
                for hour in 0..24 {
                    if !self.selected(1, hour) {
                        continue;
                    }
                    for minute in 0..60 {
                        if !self.selected(0, minute) {
                            continue;
                        }
                        let civil = date
                            .and_hms_opt(hour, minute, 0)
                            .expect("bounded clock fields");
                        let candidates = match timezone.from_local_datetime(&civil) {
                            LocalResult::Single(value) => [Some(value), None],
                            LocalResult::Ambiguous(first, second) => [Some(first), Some(second)],
                            LocalResult::None => [None, None],
                        };
                        for candidate in candidates.into_iter().flatten() {
                            let Ok(value) = u64::try_from(candidate.timestamp_millis()) else {
                                continue;
                            };
                            let eligible = if forward {
                                value >= time_ms
                            } else {
                                value <= time_ms
                            };
                            if eligible
                                && best.is_none_or(|previous| {
                                    if forward {
                                        value < previous
                                    } else {
                                        value > previous
                                    }
                                })
                            {
                                best = Some(value);
                            }
                        }
                    }
                }
            }
            let Some(next) = step(date) else {
                return Ok(best);
            };
            date = next;
        }
        Ok(best)
    }

    /// First matching whole minute at or after Unix milliseconds, in UTC.
    /// Searches at most one Gregorian 400-year cycle plus its boundary day.
    /// None means no valid date or representable future occurrence; timestamps
    /// outside the calendar library's supported range are rejected.
    pub fn at_or_after_utc(&self, time_ms: u64) -> Result<Option<u64>> {
        let remainder = time_ms % 60_000;
        let start = if remainder == 0 {
            time_ms
        } else {
            match time_ms.checked_add(60_000 - remainder) {
                Some(value) => value,
                None => return Ok(None),
            }
        };
        let signed = i64::try_from(start).map_err(|_| {
            JobError::InvalidSpec("cron timestamp is outside calendar range".into())
        })?;
        let timestamp = chrono::DateTime::from_timestamp_millis(signed).ok_or_else(|| {
            JobError::InvalidSpec("cron timestamp is outside calendar range".into())
        })?;
        let mut date = timestamp.date_naive();
        for _ in 0..=146_097 {
            if self.matches_date(date) {
                for hour in 0..24 {
                    if !self.selected(1, hour) {
                        continue;
                    }
                    for minute in 0..60 {
                        if !self.selected(0, minute) {
                            continue;
                        }
                        let candidate = date
                            .and_hms_opt(hour, minute, 0)
                            .expect("bounded hour and minute")
                            .and_utc()
                            .timestamp_millis();
                        let candidate = u64::try_from(candidate)
                            .expect("search starts at the Unix epoch or later");
                        if candidate >= start {
                            return Ok(Some(candidate));
                        }
                    }
                }
            }
            let Some(next) = date.succ_opt() else {
                return Ok(None);
            };
            date = next;
        }
        Ok(None)
    }
}

fn invalid_time() -> JobError {
    JobError::InvalidSpec("timezone cron timestamp is outside supported years 1970-9999".into())
}

fn invalid() -> JobError {
    JobError::InvalidSpec("cron requires five numeric fields with valid lists, ranges or positive wildcard/range steps".into())
}

fn number(value: &str) -> Result<u8> {
    if value.is_empty() || !value.bytes().all(|c| c.is_ascii_digit()) {
        return Err(invalid());
    }
    value.parse().map_err(|_| invalid())
}

fn field(value: &str, min: u8, max: u8) -> Result<u64> {
    let mut mask = 0;
    for item in value.split(',') {
        let (base, step) = match item.split_once('/') {
            Some((base, step)) => (base, number(step)?),
            None => (item, 1),
        };
        if step == 0 {
            return Err(invalid());
        }
        let (start, end) = if base == "*" {
            (min, max)
        } else if let Some((start, end)) = base.split_once('-') {
            (number(start)?, number(end)?)
        } else {
            if item.contains('/') {
                return Err(invalid());
            }
            let n = number(base)?;
            (n, n)
        };
        if start < min || end > max || start > end {
            return Err(invalid());
        }
        for n in (start..=end).step_by(usize::from(step)) {
            mask |= 1_u64 << n;
        }
    }
    if mask == 0 {
        return Err(invalid());
    }
    Ok(mask)
}

impl FromStr for CronExpression {
    type Err = JobError;
    fn from_str(expression: &str) -> Result<Self> {
        if expression.len() > 256 {
            return Err(invalid());
        }
        let fields: Vec<_> = expression.split_ascii_whitespace().collect();
        if fields.len() != 5 {
            return Err(invalid());
        }
        let mut masks = [0; 5];
        for (i, (min, max)) in [(0, 59), (0, 23), (1, 31), (1, 12), (0, 6)]
            .into_iter()
            .enumerate()
        {
            masks[i] = field(fields[i], min, max)?;
        }
        Ok(Self {
            masks,
            day_of_month_wildcard: fields[2].contains('*'),
            day_of_week_wildcard: fields[4].contains('*'),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn utc(value: &str) -> u64 {
        chrono::DateTime::parse_from_rfc3339(value)
            .unwrap()
            .timestamp_millis() as u64
    }
    #[test]
    fn named_utc_selection_matches_utc_planner_and_keeps_range_limit() {
        for expression in ["* * * * *", "15 3 * * *", "0 0 29 2 *", "0 0 1 * 1"] {
            let cron: CronExpression = expression.parse().unwrap();
            for from in [
                0,
                1,
                utc("2026-11-01T08:00:00Z"),
                utc("2099-03-01T00:00:01Z"),
            ] {
                assert_eq!(
                    cron.at_or_after_in_timezone(from, "UTC").unwrap(),
                    cron.at_or_after_utc(from).unwrap()
                );
            }
        }
        let yearly: CronExpression = "0 0 1 1 *".parse().unwrap();
        assert_eq!(
            yearly
                .at_or_after_in_timezone(utc("9999-12-31T00:00:00Z"), "UTC")
                .unwrap(),
            None
        );
        assert!(yearly
            .at_or_after_in_timezone(
                chrono::NaiveDate::from_ymd_opt(10000, 1, 1)
                    .unwrap()
                    .and_hms_opt(0, 0, 0)
                    .unwrap()
                    .and_utc()
                    .timestamp_millis() as u64,
                "UTC"
            )
            .is_err());
    }

    #[test]
    fn timezone_search_skips_gaps_and_orders_all_fold_occurrences() {
        let zone = "America/Los_Angeles";
        let gap: CronExpression = "30 2 * * *".parse().unwrap();
        assert_eq!(
            gap.at_or_after_in_timezone(utc("2026-03-08T08:00:00Z"), zone)
                .unwrap(),
            Some(utc("2026-03-09T09:30:00Z"))
        );
        let fold: CronExpression = "30 1 * * *".parse().unwrap();
        let first = utc("2026-11-01T08:30:00Z");
        let second = utc("2026-11-01T09:30:00Z");
        assert_eq!(
            fold.at_or_after_in_timezone(first, zone).unwrap(),
            Some(first)
        );
        assert_eq!(
            fold.at_or_after_in_timezone(first + 1, zone).unwrap(),
            Some(second)
        );
        assert_eq!(
            fold.at_or_after_in_timezone(second + 1, zone).unwrap(),
            Some(utc("2026-11-02T09:30:00Z"))
        );
        let each: CronExpression = "* 1 * * *".parse().unwrap();
        assert_eq!(
            each.at_or_after_in_timezone(utc("2026-11-01T08:45:01Z"), zone)
                .unwrap(),
            Some(utc("2026-11-01T08:46:00Z"))
        );
        assert_eq!(
            each.at_or_after_in_timezone(utc("2026-11-01T08:59:01Z"), zone)
                .unwrap(),
            Some(utc("2026-11-01T09:00:00Z"))
        );
        let midnight: CronExpression = "0 0 * * *".parse().unwrap();
        assert_eq!(
            midnight
                .at_or_after_in_timezone(utc("2011-12-30T09:00:01Z"), "Pacific/Apia")
                .unwrap(),
            Some(utc("2011-12-30T10:00:00Z"))
        );
        let half_hour: CronExpression = "45 1 * * *".parse().unwrap();
        assert_eq!(
            half_hour
                .at_or_after_in_timezone(utc("2026-04-04T14:45:00Z") + 1, "Australia/Lord_Howe")
                .unwrap(),
            Some(utc("2026-04-04T15:15:00Z"))
        );
        let historical: CronExpression = "* * * * *".parse().unwrap();
        assert_eq!(
            historical
                .at_or_after_in_timezone(utc("1971-01-01T00:00:00Z"), "Africa/Monrovia")
                .unwrap(),
            Some(utc("1971-01-01T00:00:30Z"))
        );
        assert_eq!(
            "0 0 31 2 *"
                .parse::<CronExpression>()
                .unwrap()
                .at_or_after_in_timezone(0, zone)
                .unwrap(),
            None
        );
        assert!(fold.at_or_after_in_timezone(0, "unknown/timezone").is_err());
        assert!(fold.at_or_after_in_timezone(u64::MAX, zone).is_err());
    }

    #[test]
    fn utc_search_preserves_boundaries_leap_years_and_day_matching() {
        let every: CronExpression = "* * * * *".parse().unwrap();
        assert_eq!(every.at_or_after_utc(0).unwrap(), Some(0));
        assert_eq!(every.at_or_after_utc(1).unwrap(), Some(60_000));
        assert_eq!(every.at_or_after_utc(u64::MAX).unwrap(), None);
        assert!(every.at_or_after_utc(i64::MAX as u64).is_err());
        for (expression, from, expected) in [
            ("0 0 29 2 *", "2099-03-01T00:00:00Z", "2104-02-29T00:00:00Z"),
            ("0 0 29 2 *", "1999-03-01T00:00:00Z", "2000-02-29T00:00:00Z"),
            ("0 0 1 * *", "2026-12-01T00:00:01Z", "2027-01-01T00:00:00Z"),
            ("0 0 1 * 1", "2026-10-02T00:00:00Z", "2026-10-05T00:00:00Z"),
            ("0 0 31 2 1", "2026-10-02T00:00:00Z", "2027-02-01T00:00:00Z"),
            (
                "0 0 */2 * 1",
                "2026-10-06T00:00:00Z",
                "2026-10-19T00:00:00Z",
            ),
            ("0 0 * * 0", "2026-10-01T00:00:00Z", "2026-10-04T00:00:00Z"),
        ] {
            let cron: CronExpression = expression.parse().unwrap();
            assert_eq!(
                cron.at_or_after_utc(utc(from)).unwrap(),
                Some(utc(expected)),
                "{expression}"
            );
        }
        assert_eq!(
            "0 0 31 2 *"
                .parse::<CronExpression>()
                .unwrap()
                .at_or_after_utc(0)
                .unwrap(),
            None
        );
    }

    #[test]
    fn lists_ranges_and_steps_use_field_bounds_and_anchors() {
        let cron: CronExpression = "1,10-20/3,59 0,23 */2 1-12/3 0,6".parse().unwrap();
        let selected = |mask: u64| {
            (0..64)
                .filter(|n| mask & (1_u64 << n) != 0)
                .collect::<Vec<_>>()
        };
        assert_eq!(selected(cron.masks[0]), vec![1, 10, 13, 16, 19, 59]);
        assert_eq!(selected(cron.masks[1]), vec![0, 23]);
        assert_eq!(
            selected(cron.masks[2]),
            (1..=31).step_by(2).collect::<Vec<_>>()
        );
        assert_eq!(selected(cron.masks[3]), vec![1, 4, 7, 10]);
        assert_eq!(selected(cron.masks[4]), vec![0, 6]);
        assert!(cron.day_of_month_wildcard);
        assert!(!cron.day_of_week_wildcard);
        assert!("  * * * * * ".parse::<CronExpression>().is_ok());
    }
    #[test]
    fn malformed_unsupported_and_out_of_bounds_fields_are_refused() {
        for expression in [
            "",
            "* * * *",
            "* * * * * *",
            "60 * * * *",
            "* 24 * * *",
            "* * 0 * *",
            "* * * 13 *",
            "* * * * 7",
            "*/0 * * * *",
            "3/2 * * * *",
            "9-2 * * * *",
            "1,,2 * * * *",
            "* * * JAN *",
            "* * * * MON",
            "@daily",
            "-1 * * * *",
            "+1 * * * *",
            "*/2/3 * * * *",
            "1-2-3 * * * *",
            "* * ? * *",
        ] {
            assert!(
                expression.parse::<CronExpression>().is_err(),
                "{expression}"
            );
        }
        assert!("*".repeat(257).parse::<CronExpression>().is_err());
    }
}
