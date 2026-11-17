use std::time::{SystemTime, UNIX_EPOCH};

pub fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

pub fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

pub fn date_at(secs: i64) -> String {
    let (y, m, d) = civil_from_days(secs.div_euclid(86_400));
    format!("{y:04}-{m:02}-{d:02}")
}

pub fn stamp_at(secs: i64) -> String {
    let rem = secs.rem_euclid(86_400);
    format!(
        "{}T{:02}:{:02}:{:02}Z",
        date_at(secs),
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60
    )
}

pub fn today() -> String {
    date_at(now_secs())
}

pub fn stamp() -> String {
    stamp_at(now_secs())
}

pub fn days_ago(n: i64) -> String {
    date_at(now_secs() - n * 86_400)
}

pub fn clock_of(stamp: &str) -> String {
    stamp.get(11..16).map(|s| format!("{s}Z")).unwrap_or_default()
}

pub fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = if m > 2 { m - 3 } else { m + 9 };
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

pub fn parse_date(iso: &str) -> Option<i64> {
    let head: String = iso.chars().take(10).collect();
    let parts: Vec<&str> = head.split('-').collect();
    if parts.len() != 3 {
        return None;
    }
    let y: i64 = parts[0].parse().ok()?;
    let m: i64 = parts[1].parse().ok()?;
    let d: i64 = parts[2].parse().ok()?;
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    let days = days_from_civil(y, m, d);
    if civil_from_days(days) != (y, m as u32, d as u32) {
        return None;
    }
    Some(days)
}

pub fn days_since(iso: &str, today: i64) -> Option<i64> {
    parse_date(iso).map(|d| today - d)
}

pub fn today_days() -> i64 {
    now_secs().div_euclid(86_400)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_epochs_round_trip() {
        assert_eq!(date_at(0), "1970-01-01");
        assert_eq!(stamp_at(0), "1970-01-01T00:00:00Z");
        assert_eq!(date_at(1_789_516_800), "2026-09-16");
        assert_eq!(stamp_at(1_789_516_800 + 3661), "2026-09-16T01:01:01Z");
    }

    #[test]
    fn leap_day_is_not_skipped() {
        assert_eq!(date_at(1_709_164_800), "2024-02-29");
        assert_eq!(date_at(1_709_164_800 + 86_400), "2024-03-01");
    }

    #[test]
    fn clock_reads_the_time_out_of_a_stamp() {
        assert_eq!(clock_of("2026-09-16T10:04:12Z"), "10:04Z");
        assert_eq!(clock_of("short"), "");
    }
}
