use std::cell::RefCell;
use std::collections::HashMap;
use regex::Regex;

use typed_arena;
use arena_tree;

use errors::*;
use file;
use charttime::ChartTime;
use chartdate::ChartDate;
use chartperiod::ChartPeriod;
use chartrow::ChartRow;
use web;

// Avoid unnecessary recompilation of the regular expressions
lazy_static! {
    static ref LABEL_RE: Regex = Regex::new(r"^(?:(?P<date>\d+(?:/\d){0,2}):\s*)(?P<text>.*)$").unwrap();
}

struct DeveloperData {

    // Unallocated resource for this person
    cells: ChartRow,

    // Period for which this dev is available
    period: ChartPeriod
}

impl DeveloperData {
    fn new(cells: u32, period: &ChartPeriod) -> Result<DeveloperData> {
        let mut data = DeveloperData { cells: ChartRow::new(cells), period: *period };
        data.cells.set_range(period).chain_err(|| "Developer time range not valid")?;

        Ok(data)
    }
}

struct LabelData {
    when: u32,

    text: String
}

impl LabelData {
    fn new(defn: &str) -> Result<LabelData> {

        let c = LABEL_RE.captures(defn).ok_or(format!("Couldn't parse label definition \"{}\"", defn))?;
        let date = c["date"].parse::<ChartTime>().chain_err(|| format!("Failed to parse label date \"{}\"", &c["date"]))?;

        Ok(LabelData{ when: date.to_u32(), text: c["text"].to_string()})
    }
}

pub struct RootConfigData {
    // People are only defined on the root node
    //people: HashMap<String, PersonData>,
    weeks: u32,

    // Today
    now: u32,

    // Date of the first day in the chart
    start_date: ChartDate,

    // Identity of the manager
    manager: Option<String>,

    // Mapping from name to data
    developers: HashMap<String, DeveloperData>,

    labels: Vec<LabelData>,
}

pub enum BorderType {
    None,
    Start,
    Now,
    Label
}

impl RootConfigData {
    pub fn new() -> RootConfigData {
        RootConfigData {
            weeks: 0,
            now: 0,
            start_date: ChartDate::new(),
            manager: None,
            labels: Vec::new(),
            developers: HashMap::new()
        }
    }

    pub fn add_label(&mut self, defn: &str) -> Result<()> {
        let label = LabelData::new(defn)?;
        self.labels.push(label);
        Ok(())
    }

    pub fn get_label(&self, when: &ChartTime) -> Option<String> {
        for d in &self.labels {
            if d.when >= when.to_u32() && d.when <= when.end_as_u32() {
                return Some(d.text.clone());
            }
        }
        return None;
    }

    pub fn get_weeks(&self) -> u32 {
        self.weeks

    }

    pub fn set_weeks(&mut self, weeks: u32) {
        self.weeks = weeks;

    }

    pub fn get_start_date(&self) -> ChartDate {
        self.start_date

    }

    pub fn set_start_date(&mut self, start_date: &ChartDate) {
        self.start_date = *start_date;

    }

    pub fn get_manager(&self) -> Option<String> {
        if let Some(ref manager) = self.manager {
            Some(manager.clone())
        } else {
            None
        }
    }

    pub fn set_manager(&mut self, manager: &str) {
        self.manager = Some(manager.to_string());

    }

    pub fn get_now(&self) -> u32 {
        self.now

    }

    pub fn set_now(&mut self, now: u32) {
        self.now = now;

    }

    pub fn get_now_week(&self) -> u32 {
        1 + self.now / 20
    }

    pub fn weekly_left_border(&self, week: u32) -> BorderType {
        if week == self.get_now_week() {
             BorderType::Now
        } else if week == 1 {
            BorderType::Start
        } else if self.weekly_label(week).map_or(false, |x| x.len() != 0) {
            BorderType::Label
        } else {
            BorderType::None
        }
    }

    pub fn weekly_label(&self, week: u32) -> Option<String> {
        if week == self.get_now_week() {
            Some("Now".to_string())
        } else {
            let ct = ChartTime::from_str(&format!("{}", week)).unwrap();
            self.get_label(&ct) 
        }
    }

    pub fn generate_dev_weekly_output(&self, context: &mut web::TemplateContext) {

        // Set up row data for people
        for (dev, &DeveloperData{ref cells, period: _}) in &self.developers {

            let mut row = web::TemplateRow::new(0, 0, &dev);
            let mut count = 0;
            for val in &cells.get_weekly_numbers() {
                count += 1;
                row.add_cell(*val as f32 / 4.0, count == self.get_now_week(), self.get_label(&ChartTime::from_str(&format!("{}", count)).unwrap()).map_or(false, |x| x.len() != 0));
            }
            row.set_left(cells.count() as f32 / 4.0);
            context.add_row(row);
        }
    }


    pub fn add_developer(&mut self, name: &str, period: &ChartPeriod) -> Result<()> {

        if self.developers.contains_key(name) {
            bail!("Can't re-define a developer");
        }

        let dev = DeveloperData::new(self.weeks*20, period).chain_err(|| format!("Can't add developer {}", name))?;
        self.developers.insert(name.to_string(), dev);
        Ok(())
    }

    pub fn is_valid_developer(&self, name: &str) -> bool {
        name == "outsource" || self.developers.contains_key(name)
    }

    pub fn is_valid_cell(&self, cell: u32) -> bool {
        cell < 20 * self.weeks
    }
}
