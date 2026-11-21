//! Schedule pattern parsing and next-run calculation.
//!
//! Supports:
//! - Standard 5-field cron: `minute hour day-of-month month day-of-week`
//!   (e.g. `0 9 * * *`, `*/15 * * * *`, `0 9,17 * * 1-5`)
//! - Standard cron macros: `@hourly`, `@daily`, `@midnight`, `@weekly`, `@monthly`
//! - Relative intervals: `every 30m`, `every 2h`, `every 1d`, `every 45s`
//! - One-off ISO 8601 timestamps: `2026-11-22T09:00:00Z` or `2026-11-22 09:00:00`

use anyhow::{bail, Context, Result};
use chrono::{DateTime, Datelike, Duration, TimeZone, Timelike, Utc};

use crate::agent::{Approver, Decision};
use crate::voice::is_low_risk_action;

/// Approver for unattended scheduled task runs.
///
/// Under unattended rules:
/// - Low-risk actions (read-only queries, memory recalls, web fetches) are approved automatically.
/// - Risky actions (file modifications, shell commands, destructive writes) are denied unless approved.
#[derive(Debug, Clone, Copy, Default)]
pub struct UnattendedApprover;

impl Approver for UnattendedApprover {
    fn approve(&self, tool: &str, summary: &str) -> Decision {
        if is_low_risk_action(tool, summary) {
            Decision::Yes
        } else {
            Decision::No
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SchedulePattern {
    OneOff(DateTime<Utc>),
    Interval(Duration),
    Cron(CronExpr),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CronExpr {
    /// 0..=59
    pub minutes: Vec<u32>,
    /// 0..=23
    pub hours: Vec<u32>,
    /// 1..=31
    pub days_of_month: Vec<u32>,
    /// 1..=12
    pub months: Vec<u32>,
    /// 0..=6 (0 = Sunday, 1 = Monday, ..., 6 = Saturday) or 7 for Sunday
    pub days_of_week: Vec<u32>,
}

impl SchedulePattern {
    /// Parse a schedule string into a [`SchedulePattern`].
    pub fn parse(input: &str) -> Result<Self> {
        let s = input.trim();
        if s.is_empty() {
            bail!("schedule string cannot be empty");
        }

        // 1. Cron macros
        match s.to_ascii_lowercase().as_str() {
            "@hourly" => return Ok(Self::Cron(CronExpr::parse("0 * * * *")?)),
            "@daily" | "@midnight" => return Ok(Self::Cron(CronExpr::parse("0 0 * * *")?)),
            "@weekly" => return Ok(Self::Cron(CronExpr::parse("0 0 * * 0")?)),
            "@monthly" => return Ok(Self::Cron(CronExpr::parse("0 0 1 * *")?)),
            _ => {}
        }

        // 2. Relative interval: "every <N><unit>" (e.g. "every 15m", "every 2h", "every 1d")
        if let Some(rest) = s.strip_prefix("every ") {
            let dur = parse_interval_duration(rest.trim())?;
            return Ok(Self::Interval(dur));
        }

        // 3. One-off ISO datetime (e.g. "2026-11-22T10:00:00Z" or "2026-11-22 10:00:00")
        if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
            return Ok(Self::OneOff(dt.with_timezone(&Utc)));
        }
        if let Ok(naive) = chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S") {
            return Ok(Self::OneOff(Utc.from_utc_datetime(&naive)));
        }

        // 4. Standard 5-field cron
        let parts: Vec<&str> = s.split_whitespace().collect();
        if parts.len() == 5 {
            return Ok(Self::Cron(CronExpr::parse(s)?));
        }

        bail!("unrecognized schedule format '{s}'. Use '0 9 * * *', 'every 2h', '@daily', or an ISO datetime.")
    }

    /// Calculate the next run time strictly after `from_time`.
    pub fn next_run_after(&self, from_time: DateTime<Utc>) -> Option<DateTime<Utc>> {
        match self {
            Self::OneOff(target) => {
                if *target > from_time {
                    Some(*target)
                } else {
                    None
                }
            }
            Self::Interval(dur) => {
                if dur.num_seconds() <= 0 {
                    return None;
                }
                Some(from_time + *dur)
            }
            Self::Cron(expr) => expr.next_run_after(from_time),
        }
    }
}

fn parse_interval_duration(s: &str) -> Result<Duration> {
    let s = s.trim();
    if s.is_empty() {
        bail!("empty interval duration");
    }

    // Split numeric prefix and unit suffix
    let num_end = s.find(|c: char| !c.is_ascii_digit()).unwrap_or(s.len());
    if num_end == 0 {
        bail!("interval must start with a number (e.g. '15m', '2h')");
    }

    let val: i64 = s[..num_end].parse().context("invalid interval number")?;
    let unit = s[num_end..].trim().to_ascii_lowercase();

    match unit.as_str() {
        "s" | "sec" | "secs" | "second" | "seconds" => Ok(Duration::seconds(val)),
        "m" | "min" | "mins" | "minute" | "minutes" => Ok(Duration::minutes(val)),
        "h" | "hr" | "hrs" | "hour" | "hours" => Ok(Duration::hours(val)),
        "d" | "day" | "days" => Ok(Duration::days(val)),
        _ => bail!("unknown interval unit '{unit}'. Use s, m, h, or d (e.g. '30m', '2h')."),
    }
}

impl CronExpr {
    pub fn parse(s: &str) -> Result<Self> {
        let parts: Vec<&str> = s.split_whitespace().collect();
        if parts.len() != 5 {
            bail!("cron expression must have 5 fields: minute hour day-of-month month day-of-week");
        }

        let minutes = parse_field(parts[0], 0, 59)?;
        let hours = parse_field(parts[1], 0, 23)?;
        let days_of_month = parse_field(parts[2], 1, 31)?;
        let months = parse_field(parts[3], 1, 12)?;
        let days_of_week = parse_field(parts[4], 0, 7)?;

        // Normalize 7 (Sunday) to 0
        let mut dows: Vec<u32> = days_of_week
            .into_iter()
            .map(|d| if d == 7 { 0 } else { d })
            .collect();
        dows.sort_unstable();
        dows.dedup();

        Ok(Self {
            minutes,
            hours,
            days_of_month,
            months,
            days_of_week: dows,
        })
    }

    pub fn next_run_after(&self, from_time: DateTime<Utc>) -> Option<DateTime<Utc>> {
        // Start scanning from the next minute boundary (seconds and nanoseconds zeroed out)
        let mut cur = (from_time + Duration::minutes(1))
            .with_second(0)?
            .with_nanosecond(0)?;

        // Safety limit: max 5 years ahead
        let max_time = from_time + Duration::days(365 * 5);

        while cur <= max_time {
            let m = cur.month();
            if !self.months.contains(&m) {
                // Advance to start of next month
                let (next_y, next_m) = if m == 12 {
                    (cur.year() + 1, 1)
                } else {
                    (cur.year(), m + 1)
                };
                cur = Utc.with_ymd_and_hms(next_y, next_m, 1, 0, 0, 0).single()?;
                continue;
            }

            let dom = cur.day();
            let dow = cur.weekday().num_days_from_sunday(); // 0 = Sunday .. 6 = Saturday

            let dom_matches = self.days_of_month.contains(&dom);
            let dow_matches = self.days_of_week.contains(&dow);

            if !dom_matches || !dow_matches {
                // Advance to start of next day
                cur += Duration::days(1);
                cur = cur.with_hour(0)?.with_minute(0)?.with_second(0)?;
                continue;
            }

            let h = cur.hour();
            if !self.hours.contains(&h) {
                // Advance to start of next hour
                cur += Duration::hours(1);
                cur = cur.with_minute(0)?.with_second(0)?;
                continue;
            }

            let min = cur.minute();
            if self.minutes.contains(&min) {
                return Some(cur);
            }

            cur += Duration::minutes(1);
        }

        None
    }
}

fn parse_field(field: &str, min_val: u32, max_val: u32) -> Result<Vec<u32>> {
    let mut values = Vec::new();

    for part in field.split(',') {
        let part = part.trim();
        if part == "*" {
            values.extend(min_val..=max_val);
            continue;
        }

        if let Some(rest) = part.strip_prefix("*/") {
            let step: u32 = rest
                .parse()
                .with_context(|| format!("invalid cron step in '{part}'"))?;
            if step == 0 {
                bail!("cron step cannot be 0 in '{part}'");
            }
            let mut val = min_val;
            while val <= max_val {
                values.push(val);
                val += step;
            }
            continue;
        }

        if let Some((start_str, end_str)) = part.split_once('-') {
            let start: u32 = start_str
                .parse()
                .with_context(|| format!("invalid range start in '{part}'"))?;
            let end: u32 = end_str
                .parse()
                .with_context(|| format!("invalid range end in '{part}'"))?;
            if start > end || start < min_val || end > max_val {
                bail!("cron range '{part}' out of bounds ({min_val}..={max_val})");
            }
            values.extend(start..=end);
            continue;
        }

        let num: u32 = part
            .parse()
            .with_context(|| format!("invalid cron number in '{part}'"))?;
        if num < min_val || num > max_val {
            bail!("cron value {num} out of bounds ({min_val}..={max_val}) in '{part}'");
        }
        values.push(num);
    }

    values.sort_unstable();
    values.dedup();
    if values.is_empty() {
        bail!("empty cron field '{field}'");
    }

    Ok(values)
}

/// Helper function to parse schedule and calculate next run.
pub fn next_run_after(
    schedule_str: &str,
    from_time: DateTime<Utc>,
) -> Result<Option<DateTime<Utc>>> {
    let pattern = SchedulePattern::parse(schedule_str)?;
    Ok(pattern.next_run_after(from_time))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_cron_macros() {
        let hourly = SchedulePattern::parse("@hourly").unwrap();
        assert!(matches!(hourly, SchedulePattern::Cron(_)));

        let daily = SchedulePattern::parse("@daily").unwrap();
        assert!(matches!(daily, SchedulePattern::Cron(_)));
    }

    #[test]
    fn test_parse_interval() {
        let pat = SchedulePattern::parse("every 30m").unwrap();
        assert_eq!(pat, SchedulePattern::Interval(Duration::minutes(30)));

        let pat2 = SchedulePattern::parse("every 2h").unwrap();
        assert_eq!(pat2, SchedulePattern::Interval(Duration::hours(2)));
    }

    #[test]
    fn test_parse_one_off_iso() {
        let dt = Utc
            .with_ymd_and_hms(2026, 12, 1, 10, 0, 0)
            .single()
            .unwrap();
        let s = dt.to_rfc3339();
        let pat = SchedulePattern::parse(&s).unwrap();
        assert_eq!(pat, SchedulePattern::OneOff(dt));
    }

    #[test]
    fn test_cron_next_run_daily_9am() {
        let cron = SchedulePattern::parse("0 9 * * *").unwrap();
        let now = Utc
            .with_ymd_and_hms(2026, 11, 22, 8, 30, 0)
            .single()
            .unwrap();
        let next = cron.next_run_after(now).unwrap();
        assert_eq!(
            next,
            Utc.with_ymd_and_hms(2026, 11, 22, 9, 0, 0)
                .single()
                .unwrap()
        );

        // If after 9am today, next run is 9am tomorrow
        let now_after = Utc
            .with_ymd_and_hms(2026, 11, 22, 9, 15, 0)
            .single()
            .unwrap();
        let next_tomorrow = cron.next_run_after(now_after).unwrap();
        assert_eq!(
            next_tomorrow,
            Utc.with_ymd_and_hms(2026, 11, 23, 9, 0, 0)
                .single()
                .unwrap()
        );
    }

    #[test]
    fn test_cron_next_run_step_minutes() {
        let cron = SchedulePattern::parse("*/15 * * * *").unwrap();
        let now = Utc
            .with_ymd_and_hms(2026, 11, 22, 10, 1, 0)
            .single()
            .unwrap();
        let next = cron.next_run_after(now).unwrap();
        assert_eq!(
            next,
            Utc.with_ymd_and_hms(2026, 11, 22, 10, 15, 0)
                .single()
                .unwrap()
        );
    }

    #[test]
    fn test_unattended_approver_decision_boundaries() {
        let app = UnattendedApprover;
        assert_eq!(app.approve("recall", "recall project facts"), Decision::Yes);
        assert_eq!(
            app.approve("web_fetch", "fetch documentation"),
            Decision::Yes
        );
        assert_eq!(app.approve("edit_file", "modify file"), Decision::No);
        assert_eq!(app.approve("shell", "rm -rf"), Decision::No);
    }
}
