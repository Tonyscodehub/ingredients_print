//! Small calendar helpers (no external crates).

/// Days since 1970-01-01 for a proleptic Gregorian date.
pub fn days_from_civil(y: i32, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y } as i64;
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let m = m as i64;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

/// Inverse of `days_from_civil`.
pub fn civil_from_days(z: i64) -> (i32, u32, u32) {
    let z = z + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    ((if m <= 2 { y + 1 } else { y }) as i32, m, d)
}

/// "11 Nov 2026" for the date `days` after (y, m, d).
pub fn add_days_formatted(y: i32, m: u32, d: u32, days: i64) -> String {
    const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    let (y, m, d) = civil_from_days(days_from_civil(y, m, d) + days);
    format!("{d:02} {} {y}", MONTHS[(m - 1) as usize])
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn adding_days() {
        assert_eq!(add_days_formatted(2026, 11, 8, 3), "11 Nov 2026");
        assert_eq!(add_days_formatted(2026, 12, 30, 3), "02 Jan 2027");
        assert_eq!(add_days_formatted(2028, 2, 27, 3), "01 Mar 2028"); // leap year
        assert_eq!(add_days_formatted(2027, 2, 27, 3), "02 Mar 2027");
    }
}
