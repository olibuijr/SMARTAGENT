//! 5-field cron expressions (min hour dom mon dow) with *, lists, ranges,
//! and */step. Next-fire computation over a simple civil-time model (UTC).

#[derive(Debug, Clone, PartialEq)]
pub struct Cron {
    pub minutes: Vec<u8>,  // 0-59
    pub hours: Vec<u8>,    // 0-23
    pub dom: Vec<u8>,      // 1-31
    pub months: Vec<u8>,   // 1-12
    pub dow: Vec<u8>,      // 0-6, 0 = Sunday
    dom_star: bool,
    dow_star: bool,
}

impl Cron {
    pub fn parse(expr: &str) -> Result<Cron, String> {
        let fields: Vec<&str> = expr.split_whitespace().collect();
        if fields.len() != 5 {
            return Err(format!("expected 5 cron fields, got {}", fields.len()));
        }
        Ok(Cron {
            minutes: parse_field(fields[0], 0, 59)?,
            hours: parse_field(fields[1], 0, 23)?,
            dom: parse_field(fields[2], 1, 31)?,
            months: parse_field(fields[3], 1, 12)?,
            dow: parse_field(fields[4], 0, 6)?,
            dom_star: fields[2] == "*",
            dow_star: fields[4] == "*",
        })
    }

    /// Standard cron dom/dow semantics: if both are restricted, match on OR.
    fn day_matches(&self, dom: u8, dow: u8) -> bool {
        let dom_hit = self.dom.contains(&dom);
        let dow_hit = self.dow.contains(&dow);
        match (self.dom_star, self.dow_star) {
            (true, true) => true,
            (false, true) => dom_hit,
            (true, false) => dow_hit,
            (false, false) => dom_hit || dow_hit,
        }
    }

    /// Next fire time strictly after `after` (unix seconds, UTC).
    /// Scans minute-by-minute — bounded to 5 years to guarantee termination.
    pub fn next_after(&self, after: i64) -> Option<i64> {
        let mut t = (after / 60 + 1) * 60; // next whole minute
        let limit = after + 5 * 366 * 86_400;
        while t < limit {
            let c = Civil::from_unix(t);
            // If the date (month/day) can't match, skip the whole rest of the
            // day instead of stepping minute-by-minute — turns an impossible
            // expression (e.g. Feb 30) from ~2.6M iterations into ~1830.
            if !self.months.contains(&c.month) || !self.day_matches(c.day, c.dow) {
                let into_day = c.hour as i64 * 3600 + c.minute as i64 * 60;
                t = t - into_day + 86_400; // 00:00 next day (t is minute-aligned)
                continue;
            }
            if self.hours.contains(&c.hour) && self.minutes.contains(&c.minute) {
                return Some(t);
            }
            t += 60;
        }
        None
    }
}

fn parse_field(field: &str, min: u8, max: u8) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    for part in field.split(',') {
        let (range_part, step) = match part.split_once('/') {
            Some((r, s)) => (r, s.parse::<u8>().map_err(|_| format!("bad step '{s}'"))?),
            None => (part, 1),
        };
        if step == 0 {
            return Err("step 0".into());
        }
        let (lo, hi) = if range_part == "*" {
            (min, max)
        } else if let Some((a, b)) = range_part.split_once('-') {
            (
                a.parse().map_err(|_| format!("bad range '{part}'"))?,
                b.parse().map_err(|_| format!("bad range '{part}'"))?,
            )
        } else {
            let v: u8 = range_part.parse().map_err(|_| format!("bad value '{part}'"))?;
            (v, v)
        };
        if lo < min || hi > max || lo > hi {
            return Err(format!("field '{part}' out of range {min}-{max}"));
        }
        let mut v = lo;
        while v <= hi {
            if !out.contains(&v) {
                out.push(v);
            }
            match v.checked_add(step) {
                Some(n) => v = n,
                None => break,
            }
        }
    }
    out.sort_unstable();
    Ok(out)
}

/// Civil time decomposition of a unix timestamp (UTC), std-only.
pub struct Civil {
    pub minute: u8,
    pub hour: u8,
    pub day: u8,
    pub month: u8,
    pub year: i64,
    /// 0 = Sunday.
    pub dow: u8,
}

impl Civil {
    pub fn from_unix(t: i64) -> Civil {
        let days = t.div_euclid(86_400);
        let secs = t.rem_euclid(86_400);
        // 1970-01-01 was a Thursday (dow 4).
        let dow = ((days + 4).rem_euclid(7)) as u8;
        // Howard Hinnant's civil_from_days algorithm.
        let z = days + 719_468;
        let era = z.div_euclid(146_097);
        let doe = z.rem_euclid(146_097);
        let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
        let y = yoe + era * 400;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let d = (doy - (153 * mp + 2) / 5 + 1) as u8;
        let m = if mp < 10 { mp + 3 } else { mp - 9 } as u8;
        let year = if m <= 2 { y + 1 } else { y };
        Civil {
            minute: ((secs / 60) % 60) as u8,
            hour: (secs / 3600) as u8,
            day: d,
            month: m,
            year,
            dow,
        }
    }
}

impl Civil {
    /// Inverse of `from_unix` for a wall-clock date/time (UTC). Howard
    /// Hinnant's days_from_civil algorithm.
    pub fn to_unix(year: i64, month: u32, day: u32, hour: u32, min: u32) -> i64 {
        let y = if month <= 2 { year - 1 } else { year };
        let era = y.div_euclid(400);
        let yoe = y - era * 400;
        let mp = if month > 2 { month - 3 } else { month + 9 } as i64;
        let doy = (153 * mp + 2) / 5 + day as i64 - 1;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        let days = era * 146_097 + doe - 719_468;
        days * 86_400 + hour as i64 * 3600 + min as i64 * 60
    }
}

/// Days in a month, leap-aware.
pub fn days_in_month(year: i64, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if (year % 4 == 0 && year % 100 != 0) || year % 400 == 0 { 29 } else { 28 }
        }
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn to_unix_roundtrips() {
        for t in [0i64, 1_782_952_200, 951_827_040 /* 2000-02-29 */] {
            let c = Civil::from_unix(t);
            assert_eq!(Civil::to_unix(c.year, c.month as u32, c.day as u32, c.hour as u32, c.minute as u32), t - t.rem_euclid(60));
        }
    }

    #[test]
    fn month_lengths() {
        assert_eq!(days_in_month(2026, 2), 28);
        assert_eq!(days_in_month(2028, 2), 29);
        assert_eq!(days_in_month(2100, 2), 28);
        assert_eq!(days_in_month(2026, 4), 30);
    }

    #[test]
    fn parses_fields() {
        let c = Cron::parse("*/15 0 1,15 * 1-5").unwrap();
        assert_eq!(c.minutes, vec![0, 15, 30, 45]);
        assert_eq!(c.hours, vec![0]);
        assert_eq!(c.dom, vec![1, 15]);
        assert_eq!(c.dow, vec![1, 2, 3, 4, 5]);
    }

    #[test]
    fn rejects_bad() {
        assert!(Cron::parse("* * *").is_err());
        assert!(Cron::parse("61 * * * *").is_err());
        assert!(Cron::parse("*/0 * * * *").is_err());
    }

    #[test]
    fn civil_known_date() {
        // 2026-07-02 00:30:00 UTC = 1782952200; it's a Thursday (dow 4).
        let c = Civil::from_unix(1_782_952_200);
        assert_eq!((c.year, c.month, c.day, c.hour, c.minute, c.dow), (2026, 7, 2, 0, 30, 4));
    }

    #[test]
    fn next_every_five_minutes() {
        let c = Cron::parse("*/5 * * * *").unwrap();
        // From 00:31 → 00:35.
        let t = 1_782_952_260; // 2026-07-02 00:31
        let next = c.next_after(t).unwrap();
        assert_eq!(Civil::from_unix(next).minute, 35);
    }

    #[test]
    fn month_end_rollover() {
        let c = Cron::parse("0 0 31 * *").unwrap();
        // After June 30 (no June 31), next fire is July 31.
        let june30 = 1_782_777_600; // 2026-06-30 00:00
        let next = c.next_after(june30).unwrap();
        let civ = Civil::from_unix(next);
        assert_eq!((civ.month, civ.day), (7, 31));
    }

    #[test]
    fn dom_dow_or_semantics() {
        // "0 0 13 * 5" fires on the 13th OR on Fridays.
        let c = Cron::parse("0 0 13 * 5").unwrap();
        let start = 1_782_952_200; // 2026-07-02 (Thu)
        let next = c.next_after(start).unwrap();
        let civ = Civil::from_unix(next);
        // 2026-07-03 is a Friday — OR semantics fire there, before the 13th.
        assert_eq!((civ.month, civ.day, civ.dow), (7, 3, 5));
    }

    #[test]
    fn unsatisfiable_returns_none_without_hanging() {
        // Feb 30 never exists — must return None, and (with the day-skip
        // optimization) quickly rather than after ~2.6M minute steps.
        let c = Cron::parse("0 0 30 2 *").unwrap();
        assert_eq!(c.next_after(1_782_952_200), None);
    }

    #[test]
    fn leap_day_fires() {
        // Feb 29 exists in 2028 (a leap year); a job for it must fire.
        let c = Cron::parse("0 0 29 2 *").unwrap();
        let start = 1_782_952_200; // 2026-07-02
        let civ = Civil::from_unix(c.next_after(start).unwrap());
        assert_eq!((civ.year, civ.month, civ.day), (2028, 2, 29));
    }
}
