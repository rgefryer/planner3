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
use nodes::root::RootConfigData;

// Avoid unnecessary recompilation of the regular expressions
lazy_static! {
    static ref PLAN_RE: Regex = Regex::new(r"^(?:(?P<date>\d+(?:/\d){0,2}):)?(?P<time>\d+(?:\.\d{1,2})?)(?P<suffix>pc[ym])?$").unwrap();
    static ref DONE_RE: Regex = Regex::new(r"^(?:(?P<date>\d+(?:/\d){0,2}):)(?P<time>\d+(?:\.\d{1,2})?)$").unwrap();
}

/// Strategy for scheduling child nodes
#[derive(Debug, Eq, PartialEq)]
pub enum SchedulingStrategy {
    /// The child nodes must be completed in order; no
    /// work on child 2 until child 1 is complete.
    Serial,

    /// The children can be worked on at the same time.
    /// However, resources are allocated for the children
    /// in the order they are defined.
    Parallel,
}

/// Strategy for allocating the budget
#[derive(Debug, Eq, PartialEq)]
pub enum ResourcingStrategy {
    /// Allocated on a weekly rate, calculated quarterly.
    /// 4 quarters management for every 20 quarters managees
    /// (when the manager is present).  Calculated after
    /// non-managed tasks have been removed.
    Management,

    /// Take the plan value, pro-rata it across the remaining
    /// time, subtract any future commitments, then smear the
    /// remainder.
    ///
    /// Warn if this means that the allocated resource does
    /// not match the plan.
    ///
    /// This is typically used for overheads, which anticipate
    /// a steady cost over the entire period.
    SmearProRata,

    /// Take the plan value, subtract commitments, and smear
    /// the remainder across the remaining time.  The smearing ignores
    /// existing commitments - ie the remaining costs are smeared
    /// across the quarters that are currently empty.
    ///
    /// This is typically used for fixed costs, where failure
    /// to use them early in the plan means more costs later.
    SmearRemaining,

    /// Allocate all of the plan asap.
    ///
    /// This is typically used for PRD work.  It can only
    /// be scheduled after the smeared resources.
    FrontLoad,

    /// Like FrontLoad, but allocated from the end of the period.
    BackLoad,

    /// ProdSFR is a special-case of SmearRemaining, where 20% of the
    /// remaining costs are smeared, and the other 80% are back-
    /// filled at the end of the period.
    ProdSFR,
}

struct PlanEntry {

    // When this plan was added
    when: u32,

    // Number of quarter days in the plan
    plan: u32,

    suffix: Option<String>
}

impl PlanEntry {
    fn new(when: u32, plan: u32, suffix: Option<String>) -> PlanEntry {
        PlanEntry { when, plan, suffix }
    }
}

struct DoneEntry {
    // Time the work started
    start: ChartTime,

    // How much work, in quarter days.  If the time <= the
    // span of start (eg start covers a week, and time <= 5 days)
    // then the time must be scheduled from that period.  Otherwise,
    // the time must be scheduled forward from the start time with
    // no interruptions.
    time: u32
}

impl DoneEntry {    
    fn new(start: ChartTime, time: u32) -> DoneEntry {
        DoneEntry { start, time }
    }
}


pub struct NodeConfigData {
    // Cells are only used on leaf nodes
    cells: ChartRow,

    // Budget, in quarter days
    budget: Option<u32>,

    scheduling: SchedulingStrategy,

    resourcing: ResourcingStrategy,

    // Flag that this task requires management oversight
    managed: bool,

    // Notes are problems to display on the chart
    notes: Vec<String>,

    dev: Option<String>,

    plan: Vec<PlanEntry>,

    default_plan: Vec<PlanEntry>,

    // Derived plan information
    initial_plan: Option<u32>,
    now_plan: Option<u32>,

    done: Vec<DoneEntry>,

    earliest_start: u32,

    latest_end: u32
}

impl NodeConfigData {
    pub fn new(num_cells: u32) -> NodeConfigData {
        NodeConfigData { 
            notes: Vec::new(), 
            budget: None, 
            scheduling: SchedulingStrategy::Parallel,
            resourcing: ResourcingStrategy::FrontLoad,
            managed: true,
            dev: None,
            plan: Vec::new(),
            default_plan: Vec::new(),
            initial_plan: None,
            now_plan: None,
            done: Vec::new(),
            earliest_start: 0,
            latest_end: num_cells,
            cells: ChartRow::new(num_cells)
        }
    }

    pub fn get_dev(&self, root_data: &RootConfigData, node_name: &str) -> Option<String> {
        if let Some(ref d) = self.dev {
            Some(d.clone())
        } else if root_data.is_valid_developer(node_name) {
            Some(node_name.to_string())
        } else {
            None
        }
    }

    pub fn set_dev(&mut self, root: &RootConfigData, dev: &String) -> Result<()> {
        if !root.is_valid_developer(dev) {
            bail!(format!("Developer \"{}\" not known", dev));
        }

        self.dev = Some(dev.clone());
        Ok(())
    }

    fn set_budget(&mut self, budget: f32) -> Result<()> {

        if budget < 0.0 {
            bail!("Budget must be >= 0");
        }

        self.budget = Some((budget * 4.0).round() as u32);
        Ok(())
    }

    fn add_note(&mut self, note: &str) -> Result<()> {

        self.notes.push(note.to_string());

        Ok(())
    }

    fn set_non_managed(&mut self, non_managed: &str) -> Result<()> {

        if non_managed == "true" {
            self.managed = false;
        } else if non_managed == "false" {
            self.managed = true;
        } else {
            bail!(format!("Failed to parse non-managed value \"{}\"", non_managed))
        }

        Ok(())
    }

    fn set_earliest_start(&mut self, root: &RootConfigData, when: &str) -> Result<()> {

        let ct = when.parse::<ChartTime>().chain_err(|| format!("Failed to parse earliest-start \"{}\"", when))?;
        if ct.to_u32() > self.earliest_start {
            self.earliest_start = ct.to_u32();
        }

        Ok(())
    }

    fn set_latest_end(&mut self, root: &RootConfigData, when: &str) -> Result<()> {

        let ct = when.parse::<ChartTime>().chain_err(|| format!("Failed to parse latest-end \"{}\"", when))?;
        if ct.end_as_u32() < self.latest_end {
            self.latest_end = ct.end_as_u32();
        }

        Ok(())
    }

    fn new_plan_entry(&mut self, plan: &str) -> Result<PlanEntry> {

        let c = PLAN_RE.captures(plan).ok_or(format!("Cannot parse plan part: {}", plan))?;
        let mut date = 0u32;
        if let Some(d) = c.name("date") {
            date = ChartTime::from_str(d.as_str())
                                         .map(|x| x.to_u32())
                                                   .chain_err(|| format!("Failed to parse chart time \"{}\" from plan", d.as_str()))?;
        }

        let time = c["time"].parse::<f32>().chain_err(|| format!("Failed to parse plan duration \"{}\" from plan", &c["time"]))?;
        let suffix = c.name("suffix").map(|x| x.as_str().to_string());

        Ok(PlanEntry::new(date, (time*4.0).round() as u32, suffix))   
    }

    fn set_plan(&mut self, plan: &str) -> Result<()> {

        let mut count = 0;
        for part in plan.split(", ") {
            let p = self.new_plan_entry(part)?;
            self.plan.push(p);
            count += 1;
        }

        if count == 0 {
            bail!(format!("Failed to parse plan \"{}\"", plan));
        }

        Ok(())
    }

    fn set_default_plan(&mut self, plan: &str) -> Result<()> {

        let mut count = 0;
        for part in plan.split(", ") {
            let p = self.new_plan_entry(part)?;
            self.default_plan.push(p);
            count += 1;
        }

        if count == 0 {
            bail!(format!("Failed to parse default-plan \"{}\"", plan));
        }

        Ok(())
    }

    /// Store derived information about the plan numbers for this node.
    pub fn set_derived_plan(&mut self, initial: Option<u32>, now: Option<u32>) -> Result<()> {
        self.initial_plan = initial;
        self.now_plan = now;
        Ok(())
    }

    pub fn get_plan(&self, root: &RootConfigData, dev: &Option<String>, when: u32) -> Option<u32> {
        None
    }

    pub fn get_default_plan(&self, root: &RootConfigData, dev: &Option<String>, when: u32) -> Option<u32> {
        None
    }

    fn add_done(&mut self, root: &RootConfigData, done: &str) -> Result<()> {

        let c = DONE_RE.captures(done).ok_or(format!("Cannot parse done part: \"{}\"", done))?;
        let date = c["date"].parse::<ChartTime>().chain_err(|| format!("Failed to parse done start time \"{}\" from done", &c["date"]))?;
        let time = c["time"].parse::<f32>().chain_err(|| format!("Failed to parse done duration \"{}\" from done", &c["time"]))?;
        let time_q = (time*4.0).round() as u32;


        if !root.is_valid_cell(date.to_u32() + time_q - 1) {
            bail!(format!("Done time period \"{}\" falls outside the chart", done));
        }

        self.done.push(DoneEntry::new(date, time_q));   
        Ok(())
    }

    fn set_done(&mut self, root: &RootConfigData, done: &str) -> Result<()> {

        let mut count = 0;
        for part in done.split(", ") {
            self.add_done(root, part)?;
            count += 1;
        }

        if count == 0 {
            bail!(format!("Failed to parse done \"{}\"", done));
        }

        Ok(())
    }

    fn set_schedule(&mut self, strategy: &str) -> Result<()> {

        if strategy == "serial" {
            self.scheduling = SchedulingStrategy::Serial;
        } else if strategy == "parallel" {
            self.scheduling = SchedulingStrategy::Parallel;
        } else {
            bail!(format!("Failed to parse scheduling strategy \"{}\"", strategy))
        }

        Ok(())
    }

    fn set_resource(&mut self, strategy: &str) -> Result<()> {

        if strategy == "management" {
            self.resourcing = ResourcingStrategy::Management;
        } else if strategy == "smearprorata" {
            self.resourcing = ResourcingStrategy::SmearProRata;
        } else if strategy == "smearremaining" {
            self.resourcing = ResourcingStrategy::SmearRemaining;
        } else if strategy == "frontload" {
            self.resourcing = ResourcingStrategy::FrontLoad;
        } else if strategy == "backload" {
            self.resourcing = ResourcingStrategy::BackLoad;
        } else if strategy == "prodsfr" {
            self.resourcing = ResourcingStrategy::ProdSFR;
        } else {
            bail!(format!("Failed to parse resourcing strategy \"{}\"", strategy))
        }

        Ok(())
    }

    pub fn add_attribute(&mut self, root: &RootConfigData, key: &String, value: &String) -> Result<()> {

        if key == "budget" {
            let budget = value.parse::<f32>().chain_err(|| "Failed to parse budget")?;
            self.set_budget(budget).chain_err(|| "Failed to set budget")?;
        } else if key == "schedule" {
            self.set_schedule(value).chain_err(|| "Failed to set schedule")?;
        } else if key == "resource" {
            self.set_resource(value).chain_err(|| "Failed to set resource")?;
        } else if key == "non-managed" {
            self.set_non_managed(value).chain_err(|| "Failed to set non-managed")?;
        } else if key == "dev" {
            self.set_dev(root, value).chain_err(|| "Failed to set dev")?;
        } else if key == "note" {
            self.add_note(value).chain_err(|| "Failed to add note")?;
        } else if key == "plan" {
            self.set_plan(value).chain_err(|| "Failed to set plan")?;
        } else if key == "default-plan" {
            self.set_default_plan(value).chain_err(|| "Failed to set default-plan")?;
        } else if key == "done" {
            self.set_done(root, value).chain_err(|| "Failed to set done")?;
        } else if key == "earliest-start" {
            self.set_earliest_start(root, value).chain_err(|| "Failed to set earliest-start")?;
        } else if key == "latest-end" {
            self.set_latest_end(root, value).chain_err(|| "Failed to set latest-end")?;
        } else {
            bail!(format!("Unrecognised attribute \"{}\"", key));
        }

        Ok(())
    }

    pub fn generate_weekly_output(&self,
        root_data: &RootConfigData,
        name: String, 
        line_num: u32,
        level: u32,
        context: &mut web::TemplateContext) -> Result<()> {
        
        // Set up row data for self
        let mut row = web::TemplateRow::new(level,
                                       line_num,
                                       &name);
        let mut count = 0;
        for val in &self.cells.get_weekly_numbers() {
            row.add_cell(root_data, *val as f32 / 4.0);
            count += 1;
        }

        let done = self.cells
            .count_range(&ChartPeriod::new(0, root_data.get_now()-1).unwrap()) as f32 / 4.0;
        row.set_done(done);
        let dev = self.get_dev(root_data, &name).ok_or("".to_string())?;
        row.set_who(&dev);

        if let Some(p) = self.now_plan {
            row.set_plan(p as f32 / 4.0);

            if let Some(old_p) = self.initial_plan {
                row.set_gain((p as i32 - old_p as i32) as f32 / 4.0);
            }
        }

        // @@@ Add code to work these out
        row.set_left(0.0);
        

        for n in self.notes
                .iter() {
            row.add_note(n);
        }

        context.add_row(row);

        Ok(())
    }
}
