use std::collections::HashMap;
use regex::Regex;

use errors::*;
use file;
use charttime::ChartTime;
use chartdate::ChartDate;
use chartperiod::ChartPeriod;
use chartrow::ChartRow;
use nodes::ROOT_NODE_RE;
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
            for val in &cells.get_weekly_numbers() {
                row.add_cell(self, *val as f32 / 4.0);
            }
            row.set_left(cells.count() as f32 / 4.0);
            context.add_resource_row(row);
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

    pub fn get_dev_cells<'a, 'b>(&'a mut self, name: &'b str) -> Option<&'a mut ChartRow> {
        if !self.developers.contains_key(name) {
            return None;
        }

        return Some(&mut self.developers.get_mut(name).unwrap().cells);
    }

    pub fn get_dev_period(&self, name: &str) -> Option<ChartPeriod> {
        if !self.developers.contains_key(name) {
            return None;
        }

        return Some(self.developers[name].period);
    }

    pub fn is_valid_developer(&self, name: &str) -> bool {
        name == "outsource" || self.developers.contains_key(name)
    }

    pub fn is_valid_cell(&self, cell: u32) -> bool {
        cell < 20 * self.weeks
    }

    // Work out the future, weekly resource needed to manage the non-managers, then 
    // transfer it from the manager to the row passed in. 
    //
    // Caller is responsible for checking that there is a manager configured.
    pub fn transfer_management_resource(&mut self, mut row: &mut ChartRow) -> Result<()> {

        let quarters_in_chart = self.get_weeks() * 20;
        let remaining_period = ChartPeriod::new(self.get_now(), quarters_in_chart-1).unwrap();
        let mut manager: String = String::new();
        if let Some(ref m) = self.manager {
            manager = m.clone();
        }


        // Initialize the resource tracking
        let mut weekly_resource = 0.0f32;
        let mut total_failures = 0;


        for q in 0 .. quarters_in_chart {

            if q < self.get_now() {
                continue;
            }

            let mut quarterly_resource = 0.0f32;
            for (dev, data) in &self.developers {
                if *dev != manager {
                    if data.cells.is_set(q) {
                        quarterly_resource += 0.2;
                    }
                } else {
                    if !data.cells.is_set(q) {
                        quarterly_resource = 0.0;
                        break
                    }
                }
            }

            weekly_resource += quarterly_resource;

            // If this was the last day of the week, do the resource transfer
            if q % 20 == 19 {

                for (dev, ref mut data) in self.developers.iter_mut() {
                    if *dev == manager {
                        let transfer_result = data.cells.fill_transfer_to(&mut row,
                                                                         weekly_resource.ceil() as u32,
                                                                         &ChartPeriod::new(q-19, q).unwrap())?;

                        total_failures += transfer_result.failed;
                    }
                }

                // Reset the resource tracking
                weekly_resource = 0.0f32;
            }
        }

        if total_failures != 0 {
            bail!(format!("Failed to allocate {} days of management resource", total_failures as f32 / 4.0));
        }

        Ok(())
    }

    // Handle any "nodes" that define config at the root level
    pub fn read_config(&mut self, mut config: &mut file::ConfigLines) -> Result<()> {

        if let Some(file::Line::Node(file::LineNode { line_num: _, indent: _, name })) =
            config.get_line() {

            let c = ROOT_NODE_RE.captures(&name).unwrap();
            if &c["name"] == "global" {
                self.read_global_config(&mut config).chain_err(|| "Failed to read [global] node")?;
            } else if &c["name"] == "devs" {
                self.read_devs_config(&mut config).chain_err(|| "Failed to read [devs] node")?;
            } else {
                bail!("Internal error: Unexpected node type");
            }
        } else {
            // Should not have been called without a Node to read.
            bail!("Internal error: read_root_config called without a node to read");
        }

        Ok(())
    }

    /// Store any configuration stored under [global]
    fn read_global_config(&mut self, config: &mut file::ConfigLines) -> Result<()> {
        while let Some(file::Line::Attribute(file::LineAttribute { key, value })) =
            config.peek_line() {

            config.get_line();

            if key == "weeks" {
                let weeks = value.parse::<u32>()
                    .chain_err(|| "Error parsing \"weeks\" from [chart] node")?;

                self.set_weeks(weeks);
            } else if key == "now" {
                let ct = value.parse::<ChartTime>()
                    .chain_err(|| "Error parsing \"now\" from [chart] node")?;
                self.set_now(ct.to_u32());
            } else if key == "manager" {
                self.set_manager(&value);
            } else if key == "label" {
                self.add_label(&value).chain_err(|| "Failed to add label")?;
            } else if key == "start-date" {
                let dt = value.parse::<ChartDate>()
                    .chain_err(|| "Error parsing \"start-date\" from [chart] node")?;
                self.set_start_date(&dt);
            } else {
                bail!(format!("Unrecognised attribute \"{}\" in [chart] node", key));
            }
        }
        Ok(())
    }

    /// Store any configuration stored under [devs]
    fn read_devs_config(&mut self, config: &mut file::ConfigLines) -> Result<()> {
        while let Some(file::Line::Attribute(file::LineAttribute { key, value })) =
            config.peek_line() {

            config.get_line();
            let cp = value.parse::<ChartPeriod>()
                    .chain_err(|| format!("Error parsing \"time range\" for \"{}\" in [devs] node", key))?;
            self.add_developer(&key, &cp).chain_err(|| format!("Error adding \"{}\" in [devs] node", key))?;
        }

        // Check that the manager has been defined
        if let Some(ref manager) = self.get_manager() {
            if !self.is_valid_developer(manager) {
                bail!(format!("Manager \"{}\" not defined as a dev", manager));
            }
        }

        Ok(())
    }
}
