use chrono::{DateTime, NaiveDate, TimeZone, Timelike, Utc};

#[derive(Clone, Debug, Copy, Eq, PartialEq)]
pub struct DTChatTime {
    date_time: DateTime<Utc>,
}

impl DTChatTime {
    pub fn now() -> Self {
        Self {
            date_time: Utc::now(),
        }
    }

    pub fn from_seconds(seconds: f64) -> DTChatTime {
        let secs = seconds.trunc() as i64;
        let nsecs = ((seconds.fract()) * 1_000_000_000.0).round() as u32;
        let naive = DateTime::from_timestamp(secs, nsecs).expect("Invalid timestamp");
        Self {
            date_time: DateTime::from_naive_utc_and_offset(naive.naive_utc(), Utc),
        }
    }

    pub fn timestamp_millis(&self) -> i64 {
        self.date_time.timestamp_millis()
    }

    pub fn from_timestamp_millis(timestamp: i64) -> Option<Self> {
        match DateTime::from_timestamp_millis(timestamp) {
            Some(dt) => Some(Self { date_time: dt }),
            None => None,
        }
    }
    pub fn date_naive(&self) -> NaiveDate {
        return self.date_time.date_naive();
    }

    pub fn mins_hours<Tz: TimeZone>(&self, tz: &Tz) -> (u32, u32) {
        let with_time_zone: DateTime<Tz> = self.date_time.with_timezone(tz);
        return (
            Timelike::minute(&with_time_zone),
            Timelike::hour(&with_time_zone),
        );
    }

    pub fn ts_to_str<Tz>(&self, date: bool, time: bool, separator: Option<&str>, tz: &Tz) -> String
    where
        Tz: TimeZone,
        Tz::Offset: std::fmt::Display,
    {
        let with_time_zone: DateTime<Tz> = self.date_time.with_timezone(tz);
        let mut res = String::new();

        if date {
            res += &with_time_zone.format("%Y-%m-%d").to_string();
        }
        if date && time {
            res += separator.unwrap_or(" ");
        }
        if time {
            res += &with_time_zone.format("%H:%M:%S").to_string();
        }

        res
    }
}

use std::cmp::Ordering;

impl Ord for DTChatTime {
    fn cmp(&self, other: &Self) -> Ordering {
        self.date_time.cmp(&other.date_time)
    }
}

impl PartialOrd for DTChatTime {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

pub fn f64_to_utc(timestamp: f64) -> DTChatTime {
    let secs = timestamp.trunc() as i64;
    let nsecs = ((timestamp.fract()) * 1_000_000_000.0).round() as u32;
    let naive = DateTime::from_timestamp(secs, nsecs).expect("Invalid timestamp");
    DTChatTime {
        date_time: DateTime::from_naive_utc_and_offset(naive.naive_utc(), Utc),
    }
}
