use errors::*;
use regex::Regex;
use std;
use std::str::FromStr;
use charttime::ChartTime;

#[derive(Debug, Eq, PartialEq, Copy, Clone)]
pub struct ChartPeriod {
    first: u32,
    last: u32,
}

impl FromStr for ChartPeriod {
    type Err = Error;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        ChartPeriod::from_str(s)
    }
}

impl ChartPeriod {
    pub fn from_str(period: &str) -> Result<ChartPeriod> {

        // Avoid unnecessary recompilation of the regular expressions
        lazy_static! {
            static ref CHARTPERIOD_RE: Regex = 
                Regex::new(r"^(?P<start>[\d/]+)\.\.(?P<end>[\d/]+)$").unwrap();
        }

        let c = CHARTPERIOD_RE.captures(period).ok_or(format!("Cannot parse ChartPeriod: {}", period))?;
        let start = c["start"].parse::<ChartTime>()
            .chain_err(|| format!("Cannot parse start out of: {}", period))?;
        let end = c["end"].parse::<ChartTime>()
            .chain_err(|| format!("Cannot parse end out of: {}", period))?;

        let start_q = start.to_u32();
        let end_q = end.end_as_u32();

        ChartPeriod::new(start_q, end_q).chain_err(|| format!("Cannot create period from: {}", period))
    }

    pub fn new(first: u32, last: u32) -> Result<ChartPeriod> {

        if first > last {
            bail!(format!("End of period must be after the start"));
        }

        Ok(ChartPeriod {
                     first: first,
                     last: last,
                 })
    }

    pub fn intersect(&self, other: &ChartPeriod) -> Option<ChartPeriod> {
        if self.first >= other.first && self.first <= other.last {
            if self.last > other.last {
                Some(ChartPeriod {
                         first: self.first,
                         last: other.last,
                     })
            } else {
                Some(ChartPeriod {
                         first: self.first,
                         last: self.last,
                     })
            }

        } else if other.first >= self.first && other.first <= self.last {
            if other.last > self.last {
                Some(ChartPeriod {
                         first: other.first,
                         last: self.last,
                     })
            } else {
                Some(ChartPeriod {
                         first: other.first,
                         last: other.last,
                     })
            }
        } else {
            None
        }
    }

    pub fn union(&self, other: &ChartPeriod) -> Option<ChartPeriod> {
        if self.first >= other.first && self.first <= other.last {
            if self.last > other.last {
                Some(ChartPeriod {
                         first: other.first,
                         last: self.last,
                     })
            } else {
                Some(ChartPeriod {
                         first: other.first,
                         last: other.last,
                     })
            }

        } else if other.first >= self.first && other.first <= self.last {
            if other.last > self.last {
                Some(ChartPeriod {
                         first: self.first,
                         last: other.last,
                     })
            } else {
                Some(ChartPeriod {
                         first: self.first,
                         last: self.last,
                     })
            }
        } else {
            None
        }

    }

    pub fn limit_first(&self, first: u32) -> Option<ChartPeriod> {
        if first > self.get_last() {
            None
        } else if first < self.get_first() {
            Some(*self)
        } else {
            Some(ChartPeriod::new(first, self.get_last()).unwrap())
        }
    }

    pub fn limit_last(&self, last: u32) -> Option<ChartPeriod> {
        if last < self.get_first() {
            None
        } else if last > self.get_last() {
            Some(*self)
        } else {
            Some(ChartPeriod::new(self.get_first(), last).unwrap())
        }
    }

    pub fn get_first(&self) -> u32 {
        self.first
    }

    pub fn get_last(&self) -> u32 {
        self.last
    }

    pub fn length(&self) -> u32 {
        self.last + 1 - self.first
    }
}
