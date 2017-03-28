use std;
use std::cmp::Ordering;
use std::str::FromStr;
use regex::Regex;
use errors::*;

#[derive(Debug, Eq, Copy, Clone)]
pub struct ChartTime {
    week: u32,
    day: Option<u32>,
    quarter: Option<u32>,
}

impl Ord for ChartTime {
    fn cmp(&self, other: &ChartTime) -> Ordering {
        self.to_u32().cmp(&other.to_u32())
    }
}

impl PartialOrd for ChartTime {
    fn partial_cmp(&self, other: &ChartTime) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for ChartTime {
    fn eq(&self, other: &ChartTime) -> bool {
        self.to_u32() == other.to_u32()
    }
}

impl FromStr for ChartTime {
    type Err = Error;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        ChartTime::from_str(s)
    }
}

impl ChartTime {
    pub fn from_u32(q: u32) -> ChartTime {
        let week = q / 20;
        let day = (q % 20) / 4;
        let quarter = q - week * 20 - day * 4;
        ChartTime {
            week: week + 1,
            day: Some(day + 1),
            quarter: Some(quarter + 1),
        }
    }

    pub fn from_str(desc: &str) -> Result<ChartTime> {

        // Avoid unnecessary recompilation of the regular expressions
        lazy_static! {
            static ref CHARTTIME_RE: Regex = 
                Regex::new(r"^(?P<week>\d+)(?:/(?P<day>[1-5]))?(?:/(?P<quarter>[1-4]))?$").unwrap();
        }

        let c = CHARTTIME_RE.captures(desc).ok_or(format!("Cannot parse ChartTime: {}", desc))?;
        let week = c["week"].parse::<u32>()
            .chain_err(|| format!("Cannot parse week out of: {}", desc))?;
        let day = c.name("day").map(|d| d.as_str().parse::<u32>().unwrap());
        let quarter = c.name("quarter").map(|q| q.as_str().parse::<u32>().unwrap());

        if week == 0 {
            bail!(format!("Week cannot be 0 in ChartTime: {}", desc));
        }

        Ok(ChartTime {
               week: week,
               day: day,
               quarter: quarter,
           })
    }

    /// Return the quarter that this time starts at
    pub fn to_u32(&self) -> u32 {
        (self.week - 1) * 20 + (self.day.unwrap_or(1) - 1) * 4 + self.quarter.unwrap_or(1) - 1
    }

    pub fn to_string(&self) -> String {
        if let Some(_) = self.quarter {
            format!("{}/{}/{}",
                    self.week,
                    self.day.unwrap(),
                    self.quarter.unwrap())
        } else if let Some(_) = self.day {
            format!("{}/{}", self.week, self.day.unwrap())
        } else {
            format!("{}", self.week)
        }
    }

    pub fn end_as_u32(&self) -> u32 {
        self.to_u32() + self.duration() - 1
    }

    pub fn duration(&self) -> u32 {
        if let Some(_) = self.quarter {
            1
        } else if let Some(_) = self.day {
            4
        } else {
            20
        }
    }
}
