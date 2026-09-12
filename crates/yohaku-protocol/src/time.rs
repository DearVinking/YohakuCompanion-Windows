//! RFC3339 UTC 毫秒时间戳（协议规定的唯一日期形态）。

use chrono::{DateTime, NaiveDateTime, TimeZone, Utc};

const FORMAT: &str = "%Y-%m-%dT%H:%M:%S%.3fZ";
const WIRE_LEN: usize = 24;

pub fn format_rfc3339_millis(t: DateTime<Utc>) -> String {
    t.format(FORMAT).to_string()
}

/// 严格匹配 `^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{3}Z$`，
/// 拒绝缺毫秒、多余小数位、偏移量等一切变体。
pub fn parse_rfc3339_millis(s: &str) -> Option<DateTime<Utc>> {
    let b = s.as_bytes();
    if b.len() != WIRE_LEN {
        return None;
    }
    let fixed = [4, 7, 10, 13, 16, 19, 23];
    let expected = *b"--T::.Z";
    if fixed.iter().zip(expected.iter()).any(|(i, e)| b[*i] != *e) {
        return None;
    }
    if b.iter()
        .enumerate()
        .any(|(i, &c)| !fixed.contains(&i) && !c.is_ascii_digit())
    {
        return None;
    }
    let naive: NaiveDateTime = NaiveDateTime::parse_from_str(s, FORMAT).ok()?;
    Some(Utc.from_utc_datetime(&naive))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeDelta;

    #[test]
    fn date_format_millis() {
        let t = Utc.with_ymd_and_hms(2026, 1, 2, 3, 4, 5).unwrap() + TimeDelta::milliseconds(7);
        assert_eq!(format_rfc3339_millis(t), "2026-01-02T03:04:05.007Z");
    }

    #[test]
    fn date_parse_strict() {
        let t = Utc.with_ymd_and_hms(2026, 1, 2, 3, 4, 5).unwrap() + TimeDelta::milliseconds(7);
        assert_eq!(parse_rfc3339_millis("2026-01-02T03:04:05.007Z"), Some(t));
        assert_eq!(parse_rfc3339_millis("2026-01-02T03:04:05Z"), None); // 必须含毫秒
        assert_eq!(parse_rfc3339_millis("2026-01-02T03:04:05.007000Z"), None); // 不接受多余位
        assert_eq!(parse_rfc3339_millis("2026-01-02T03:04:05.007+08:00"), None); // 必须 UTC Z
        assert_eq!(parse_rfc3339_millis("not-a-date"), None);
        assert_eq!(parse_rfc3339_millis("2026-13-02T03:04:05.007Z"), None); // 月份无效
    }
}
