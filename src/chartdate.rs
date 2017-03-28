use std;
use std::cmp::Ordering;
use std::str::FromStr;
use regex::Regex;
use errors::*;
use chrono::prelude::*;
use chrono;

#[derive(Debug, Eq, Copy, Clone)]
pub struct ChartDate {
    dt: DateTime<UTC>,
}

impl Ord for ChartDate {
    fn cmp(&self, other: &ChartDate) -> Ordering {
        self.dt.cmp(&other.dt)
    }
}

impl PartialOrd for ChartDate {
    fn partial_cmp(&self, other: &ChartDate) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for ChartDate {
    fn eq(&self, other: &ChartDate) -> bool {
        self.dt == other.dt
    }
}

impl FromStr for ChartDate {
    type Err = Error;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        ChartDate::from_str(s)
    }
}

impl ChartDate {
    pub fn new() -> ChartDate {
        ChartDate { dt: UTC.ymd(2001, 1, 1).and_hms(0, 0, 0) }
    }

    pub fn from_str(date: &str) -> Result<ChartDate> {

        // Avoid unnecessary recompilation of the regular expressions
        lazy_static! {
            static ref CHARTDATE_RE: Regex = 
                Regex::new(r"^(?P<day>\d{1,2})/(?P<month>\d{1,2})?/(?P<year>\d\d)?$").unwrap();
        }

        let c = CHARTDATE_RE.captures(date).ok_or(format!("Cannot parse ChartDate: {}", date))?;
        let day = c["day"].parse::<u32>()
            .chain_err(|| format!("Cannot parse day out of: {}", date))?;
        let month = c["month"].parse::<u32>()
            .chain_err(|| format!("Cannot parse month out of: {}", date))?;
        let year = c["year"].parse::<i32>()
            .chain_err(|| format!("Cannot parse year out of: {}", date))?;
        if let chrono::LocalResult::Single(dt) =
            UTC.ymd_opt(2000i32 + year, month, day).and_hms_opt(0, 0, 0) {
            return Ok(ChartDate { dt: dt });
        } else {
            bail!(format!("Cannot create date from: {}", date));
        }
    }

    pub fn to_string(&self) -> String {
        format!("{}/{}/{:02}",
                self.dt.day(),
                self.dt.month(),
                self.dt.year() % 100)
    }
}
