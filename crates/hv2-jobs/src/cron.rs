//! Numeric five-field cron grammar and bounded UTC calendar occurrence search.
use crate::{JobError, Result};
use chrono::{Datelike, NaiveDate};
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
