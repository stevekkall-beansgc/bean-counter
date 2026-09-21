use crate::{Error, Result};
use serde::{Deserialize, Serialize};
/// Checked Gregorian UTC microseconds; parsing never reads a clock.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Timestamp {
    canonical: String,
    micros: i64,
}
impl Timestamp {
    pub fn parse(s: &str) -> Result<Self> {
        fn fail() -> Error {
            Error::new("TIMESTAMP","expected Gregorian timestamp with explicit offset and at most six fractional digits")
        }
        fn number(s: &str) -> Result<i64> {
            if !s.bytes().all(|b| b.is_ascii_digit()) {
                return Err(fail());
            }
            s.parse().map_err(|_| fail())
        }
        if !s.is_ascii()
            || s.len() < 20
            || s.get(4..5) != Some("-")
            || s.get(7..8) != Some("-")
            || s.get(10..11) != Some("T")
            || s.get(13..14) != Some(":")
            || s.get(16..17) != Some(":")
        {
            return Err(fail());
        }
        let y = number(&s[0..4])?;
        let m = number(&s[5..7])?;
        let d = number(&s[8..10])?;
        let h = number(&s[11..13])?;
        let min = number(&s[14..16])?;
        let sec = number(&s[17..19])?;
        if !(1..=9999).contains(&y)
            || !(1..=12).contains(&m)
            || d < 1
            || d > month_days(y, m)
            || h > 23
            || min > 59
            || sec > 59
        {
            return Err(fail());
        }
        let tail = &s[19..];
        let (fraction, offset) = if let Some(t) = tail.strip_prefix('.') {
            let n = t.bytes().take_while(u8::is_ascii_digit).count();
            if n == 0 || n > 6 {
                return Err(fail());
            }
            (&t[..n], &t[n..])
        } else {
            ("", tail)
        };
        let frac = if fraction.is_empty() {
            0
        } else {
            number(fraction)? * 10i64.pow((6 - fraction.len()) as u32)
        };
        let shift = if offset == "Z" {
            0
        } else {
            if offset.len() != 6
                || !matches!(offset.as_bytes()[0], b'+' | b'-')
                || &offset[3..4] != ":"
            {
                return Err(fail());
            }
            let oh = number(&offset[1..3])?;
            let om = number(&offset[4..6])?;
            if oh > 23 || om > 59 {
                return Err(fail());
            }
            (oh * 60 + om) * if offset.starts_with('-') { -1 } else { 1 }
        };
        let days = days_before_year(y) + (1..m).map(|mm| month_days(y, mm)).sum::<i64>() + d - 1;
        let seconds = days * 86400 + h * 3600 + min * 60 + sec - shift * 60;
        if seconds < 0 || seconds >= days_before_year(10000) * 86400 {
            return Err(fail());
        }
        let day = seconds / 86400;
        let within = seconds % 86400;
        let mut year = (day / 366 + 1).min(9999);
        while days_before_year(year + 1) <= day {
            year += 1;
        }
        let mut rest = day - days_before_year(year);
        let mut month = 1;
        while rest >= month_days(year, month) {
            rest -= month_days(year, month);
            month += 1;
        }
        let canonical = format!(
            "{year:04}-{month:02}-{:02}T{:02}:{:02}:{:02}.{frac:06}Z",
            rest + 1,
            within / 3600,
            (within / 60) % 60,
            within % 60
        );
        let micros = (seconds - days_before_year(1970) * 86400) * 1_000_000 + frac;
        Ok(Self { canonical, micros })
    }
    pub fn micros(&self) -> i64 {
        self.micros
    }
    pub fn as_str(&self) -> &str {
        &self.canonical
    }
}
fn days_before_year(y: i64) -> i64 {
    let y = y - 1;
    y * 365 + y / 4 - y / 100 + y / 400
}
fn month_days(y: i64, m: i64) -> i64 {
    match m {
        4 | 6 | 9 | 11 => 30,
        2 => {
            if y % 4 == 0 && (y % 100 != 0 || y % 400 == 0) {
                29
            } else {
                28
            }
        }
        _ => 31,
    }
}
impl TryFrom<String> for Timestamp {
    type Error = Error;
    fn try_from(v: String) -> Result<Self> {
        Self::parse(&v)
    }
}
impl From<Timestamp> for String {
    fn from(v: Timestamp) -> Self {
        v.canonical
    }
}
