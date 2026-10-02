//! Numeric five-field cron grammar. Calendar planning is integrated separately.
use crate::{JobError, Result};
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
