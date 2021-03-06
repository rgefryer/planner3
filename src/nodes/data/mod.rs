use regex::Regex;

use errors::*;
use charttime::ChartTime;
use chartperiod::ChartPeriod;
use chartrow::{ChartRow, TransferResult};
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
#[derive(Debug, Eq, PartialEq, Copy, Clone)]
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
    ProdSFR_part2,
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

    resourcing: Option<ResourcingStrategy>,

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

    latest_end: u32,

    resource_transferred: bool
}

impl NodeConfigData {
    pub fn new(num_cells: u32) -> NodeConfigData {
        NodeConfigData { 
            notes: Vec::new(), 
            budget: None, 
            scheduling: SchedulingStrategy::Parallel,
            resourcing: None,
            managed: true,
            dev: None,
            plan: Vec::new(),
            default_plan: Vec::new(),
            initial_plan: None,
            now_plan: None,
            done: Vec::new(),
            earliest_start: 0,
            latest_end: num_cells,
            resource_transferred: false,
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

    /// Transfer resource specified in "done" from the developer to 
    /// this node's cells.
    pub fn transfer_done(&mut self, root: &mut RootConfigData, past: bool) -> Result<()> {

        let now = root.get_now();
        if let Some(ref dev) = self.dev {
            if let Some(dev_data) = root.get_dev_data(dev) {
                for done in &self.done {

                    if past && done.start.to_u32() >= now {
                        continue;
                    }
                    if !past && done.start.to_u32() < now {
                        continue;
                    }

                    let period = if done.time <= done.start.duration() {
                        ChartPeriod::new(done.start.to_u32(), done.start.end_as_u32()).unwrap()
                    } else {
                        ChartPeriod::new(done.start.to_u32(), done.start.to_u32()+done.time-1).unwrap()
                    };

                    let result = dev_data.cells.fill_transfer_to(&mut self.cells, done.time, &period).chain_err(|| format!("Failed to add resource at time {}", done.start.to_string()))?;
                    if result.failed != 0 {
                        // @@@ Convert time to weekly format
                        bail!(format!("Failed to add {} quarters of resource at time {}", result.failed, done.start.to_string()));
                    }
                }
            }
        }

        Ok(())
    }

    pub fn transfer_past_done(&mut self, root: &mut RootConfigData) -> Result<()> {
        self.transfer_done(root, true)
    }
    pub fn transfer_future_done_managed(&mut self, root: &mut RootConfigData) -> Result<()> {
        if !self.managed {
            return Ok(());
        }
        self.transfer_done(root, false)
    }
    pub fn transfer_future_done_unmanaged(&mut self, root: &mut RootConfigData) -> Result<()> {
        if self.managed {
            return Ok(());
        }
        self.transfer_done(root, false)
    }

    pub fn transfer_future_smear(&mut self, root: &mut RootConfigData) -> Result<()> {
        if self.resourcing.is_none() {
            return Ok(());
        }
        let r = self.resourcing.unwrap();
        if r == ResourcingStrategy::SmearRemaining || r == ResourcingStrategy::SmearProRata || r == ResourcingStrategy::ProdSFR {
            return self.transfer_future_resource(root, Some(r));
        }

        Ok(())
    }

    pub fn transfer_future_frontload(&mut self, root: &mut RootConfigData) -> Result<()> {
        if self.resourcing.is_none() {
            return Ok(());
        }
        let r = self.resourcing.unwrap();
        if r == ResourcingStrategy::FrontLoad {
            return self.transfer_future_resource(root, Some(r));
        }

        Ok(())
    }

    pub fn transfer_future_backload(&mut self, root: &mut RootConfigData) -> Result<()> {
        if self.resourcing.is_none() {
            return Ok(());
        }
        let r = self.resourcing.unwrap();
        if r == ResourcingStrategy::BackLoad {
            return self.transfer_future_resource(root, Some(r));
        } else if r == ResourcingStrategy::ProdSFR {
            return self.transfer_future_resource(root, Some(ResourcingStrategy::ProdSFR_part2));
        }

        Ok(())
    }

    pub fn transfer_future_unmanaged_resource(&mut self, root: &mut RootConfigData) -> Result<()> {
        if self.managed {
            return Ok(());
        }
        self.transfer_future_resource(root, None)
    }

    pub fn transfer_future_management_resource(&mut self, root: &mut RootConfigData) -> Result<()> {


        if let Some(ResourcingStrategy::Management) = self.resourcing {

            if let Some(ref dev) = self.dev {

                // Verify that the manager for this row matches that for the chart
                if let Some(mgr) = root.get_manager() {
                    if mgr != *dev {
                        bail!(format!("\"{}\" is not the configured manager, expected \"{}\"", dev, mgr));
                    }
                } else {
                    bail!("No manager defined in global config");
                }

                root.transfer_management_resource(&mut self.cells)?;
            }

            self.resource_transferred = true;
        }

        Ok(())
    }

    pub fn transfer_future_remaining_resource(&mut self, root: &mut RootConfigData) -> Result<()> {
        self.transfer_future_resource(root, None)
    }


    /// Transfer resource specified in "done" from the developer to 
    /// this node's cells.
    pub fn transfer_future_resource(&mut self, root: &mut RootConfigData, resourcing: Option<ResourcingStrategy>) -> Result<()> {

        if self.resource_transferred {
            return Ok(());
        }

        if self.now_plan.is_none() {
            return Ok(());
        }

        let plan = self.now_plan.unwrap();   // Total quarters we want set in the row

        if let Some(ref dev) = self.dev {
            let quarters_in_chart = root.get_weeks() * 20;
            let chart_period = ChartPeriod::new(0, quarters_in_chart-1).unwrap();
            let quarters_left_in_plan = if plan > self.cells.count_range(&chart_period) {
                plan - self.cells.count_range(&chart_period)
            } else {
                0
            };
            let resource_period = root.get_dev_period(dev).unwrap_or(chart_period);
            let remaining_period_opt = ChartPeriod::new(root.get_now(), quarters_in_chart-1).unwrap().intersect(&resource_period);
            if remaining_period_opt.is_none() {
                if quarters_left_in_plan == 0 {
                    return Ok(());
                } else {
                    bail!(format!("Failed to write {} days because {} is not available.", quarters_left_in_plan as f32 / 4.0, dev));
                }
            }
            let remaining_period = remaining_period_opt.unwrap();

            if let Some(dev_data) = root.get_dev_data(dev) {

                // Get allocation type
                let mut transfer_result = TransferResult::new(quarters_left_in_plan);
                let mut r = if resourcing.is_none() {
                    self.resourcing
                } else {
                    resourcing
                };

                match r {
                    Some(ResourcingStrategy::Management) => {
                        // No-op - the management row is handled out-of-band
                        transfer_result = TransferResult::new(0);
                    },
                    Some(ResourcingStrategy::SmearProRata) => {

                        // Time to spend per quarter day on this task
                        let time_per_quarter = plan as f32 / (resource_period.length() as f32);

                        // Time to spend in the rest of the period
                        let mut time_to_spend = (remaining_period.length() as f32 * time_per_quarter).ceil();

                        // Subtract any time already committed.
                        time_to_spend -= self.cells
                            .count_range(&remaining_period) as f32;
                        if time_to_spend < -0.01 {
                            bail!(format!("Over-committed by {} days; update plan",
                                                   time_to_spend * -1.0));
                        }

                        // Smear the remainder.
                        transfer_result = dev_data.cells.smear_transfer_to(&mut self.cells,
                                                                 time_to_spend as u32,
                                                                 &remaining_period)?;
                        self.resource_transferred = true;
                    },
                    Some(ResourcingStrategy::SmearRemaining) => {
                        transfer_result = dev_data.cells.smear_transfer_to(&mut self.cells,
                                                                      quarters_left_in_plan,
                                                                      &remaining_period)?;
                        self.resource_transferred = true;
                    },
                    Some(ResourcingStrategy::FrontLoad) => {
                        transfer_result = dev_data.cells.fill_transfer_to(&mut self.cells,
                                                                     quarters_left_in_plan,
                                                                     &remaining_period)?;
                        self.resource_transferred = true;
                    },
                    Some(ResourcingStrategy::BackLoad) => {
                        transfer_result = dev_data.cells.reverse_fill_transfer_to(&mut self.cells,
                                                                             quarters_left_in_plan,
                                                                             &remaining_period)?;
                        self.resource_transferred = true;
                    },
                    Some(ResourcingStrategy::ProdSFR) => {
                        // Smear 20%, then backfill 80%.  If the smear fails, add the remaining
                        // work te the backfill.  It's unlikely to help, but we'll end up with 
                        // an accurate result to display.
                        let smeared_resource = quarters_left_in_plan * 20 / 100;

                        transfer_result = dev_data.cells.smear_transfer_to(&mut self.cells,
                                                                      smeared_resource,
                                                                      &remaining_period).chain_err(|| "Failed to smear initial 20%")?;

                        // Don't flag resource transferred yet until part 2 has been done
                    }
                    Some(ResourcingStrategy::ProdSFR_part2) => {
                        // Backfill the remaining resource.
                        transfer_result = dev_data.cells.reverse_fill_transfer_to(&mut self.cells,
                                                                             quarters_left_in_plan,
                                                                             &remaining_period).chain_err(|| "Failed to backfill 80%")?;
                        self.resource_transferred = true;
                    }
                    None => {
                        bail!("ResourcingStrategy not specified!");
                    }
                };

                if transfer_result.failed != 0 {
                    dev_data.unallocated += transfer_result.failed;
                    bail!(format!("{} days unallocated", transfer_result.failed as f32 / 4.0));
                }
                // @@@ Handle the result - propagation of serialized constraints.
            }
        }

        Ok(())
    }

    fn set_budget(&mut self, budget: f32) -> Result<()> {

        if budget < 0.0 {
            bail!("Budget must be >= 0");
        }

        self.budget = Some((budget * 4.0).round() as u32);
        Ok(())
    }

    pub fn add_note(&mut self, note: &str) -> Result<()> {

        self.notes.push(note.to_string());

        Ok(())
    }

    pub fn get_managed(&self) -> bool {
        self.managed
    }

    pub fn set_managed(&mut self, managed: bool)  {
        self.managed = managed
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

    fn set_earliest_start(&mut self, when: &str) -> Result<()> {

        let ct = when.parse::<ChartTime>().chain_err(|| format!("Failed to parse earliest-start \"{}\"", when))?;
        if ct.to_u32() > self.earliest_start {
            self.earliest_start = ct.to_u32();
        }

        Ok(())
    }

    fn set_latest_end(&mut self, when: &str) -> Result<()> {

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

    fn get_plan_internal(&self, root: &RootConfigData, dev: &Option<String>, when: u32, vec: &Vec<PlanEntry>) -> Option<u32> {

        let mut found_val: Option<u32> = None;
        let mut found_suffix: Option<String> = None;
        for plan_entry in vec {
            if when >= plan_entry.when  {
                found_val = Some(plan_entry.plan);
                if let Some(ref suffix) = plan_entry.suffix {
                    found_suffix = Some(suffix.clone());
                } else {
                    found_suffix = None;
                }
            }
        }

        if let Some(mut plan) = found_val {
            if let Some(ref suffix) = found_suffix {
                let duration = root.get_plan_dev_duration(dev);
                if suffix == "pcy" {
                    plan = (plan as f32 * duration as f32 / (20.0 * 52.0)).ceil() as u32;
                } else { // pcm
                    plan = (plan as f32 * duration as f32 / (20.0 * 52.0 / 12.0)).ceil() as u32;
                }
            }

            return Some(plan);

        } else {
            return None;
        }
    }

    pub fn get_plan(&self, root: &RootConfigData, dev: &Option<String>, when: u32) -> Option<u32> {
        self.get_plan_internal(root, dev, when, &self.plan)
    }

    pub fn get_default_plan(&self, root: &RootConfigData, dev: &Option<String>, when: u32) -> Option<u32> {
        self.get_plan_internal(root, dev, when, &self.default_plan)
    }

    fn add_done(&mut self, root: &RootConfigData, done: &str) -> Result<()> {

        let c = DONE_RE.captures(done).ok_or(format!("Cannot parse done part: \"{}\"", done))?;
        let date = c["date"].parse::<ChartTime>().chain_err(|| format!("Failed to parse done start time \"{}\" from done", &c["date"]))?;
        let time = c["time"].parse::<f32>().chain_err(|| format!("Failed to parse done duration \"{}\" from done", &c["time"]))?;
        let time_q = (time*4.0).round() as u32;

        if time_q == 0 {
            bail!("Specified done time as 0");
        }

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
            self.resourcing = Some(ResourcingStrategy::Management);
        } else if strategy == "smearprorata" {
            self.resourcing = Some(ResourcingStrategy::SmearProRata);
        } else if strategy == "smearremaining" {
            self.resourcing = Some(ResourcingStrategy::SmearRemaining);
        } else if strategy == "frontload" {
            self.resourcing = Some(ResourcingStrategy::FrontLoad);
        } else if strategy == "backload" {
            self.resourcing = Some(ResourcingStrategy::BackLoad);
        } else if strategy == "prodsfr" {
            self.resourcing = Some(ResourcingStrategy::ProdSFR);
        } else {
            bail!(format!("Failed to parse resourcing strategy \"{}\"", strategy))
        }

        Ok(())
    }

    pub fn get_resourcing(&self, root_data: &RootConfigData, node_name: &str) -> Option<ResourcingStrategy> {
        self.resourcing
    }

    pub fn set_resourcing(&mut self, root_data: &RootConfigData, r: ResourcingStrategy) -> Result<()> {
        self.resourcing = Some(r);
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
            self.set_earliest_start(value).chain_err(|| "Failed to set earliest-start")?;
        } else if key == "latest-end" {
            self.set_latest_end(value).chain_err(|| "Failed to set latest-end")?;
        } else {
            bail!(format!("Unrecognised attribute \"{}\"", key));
        }

        Ok(())
    }

    // Work out the pro-rata plan at a given date
    pub fn pro_rata_plan_at_date(&self, when: u32, plan: u32, root: &RootConfigData) -> u32 {

        // First off, get the per-cell resource allocation
        let duration = root.get_plan_dev_duration(&self.dev);
        let work_per_cell = plan as f32 / duration as f32;

        // Work out work remaining
        let period = ChartPeriod::new(when, root.get_weeks() * 20 - 1).unwrap();
        let mut cells_remaining = period.length();
        if let Some(ref d) = self.dev {
            if let Some(ref dp) = root.get_dev_period(d) {
                if let Some(p) = period.intersect(dp) {
                    cells_remaining = p.length();
                } else {
                    cells_remaining = 0;
                }
            }
        }

        let work_remaining = cells_remaining as f32 * work_per_cell;
        let work_remaining = work_remaining.ceil() as u32;

        if when == 0 {
            return work_remaining;
        }

        let time_until_now = ChartPeriod::new(0, when-1).unwrap();
        let done = self.cells.count_range(&time_until_now);

        done + work_remaining
    }

    pub fn generate_weekly_output(&self,
        root_data: &RootConfigData,
        node_name: String, 
        line_num: u32,
        level: u32,
        context: &mut web::TemplateContext) -> Result<()> {
        
        // Set up row data for self
        let mut row = web::TemplateRow::new(level,
                                       line_num,
                                       &node_name);
        for val in &self.cells.get_weekly_numbers() {
            row.add_cell(root_data, *val as f32 / 4.0);
        }

        let time_until_now = ChartPeriod::new(0, root_data.get_now()-1).unwrap();
        let done = self.cells.count_range(&time_until_now);
        row.set_done(done as f32 / 4.0);
        if let Some(dev) = self.get_dev(root_data, &node_name) {
            row.set_who(&dev);
        }

        if let Some(p) = self.now_plan {

            if let Some(ResourcingStrategy::SmearProRata) = self.resourcing {
                // For pro-rata resourcing, the plan value must be calculated,
                // from the actual past, plus pro-rata-ing the future.


                let new_plan = self.pro_rata_plan_at_date(root_data.get_now(), p, root_data);

                row.set_plan(new_plan as f32 / 4.0);

                if let Some(old_p) = self.initial_plan {
                    let old_plan = self.pro_rata_plan_at_date(0, old_p, root_data);
                    row.set_gain((old_plan as i32 - new_plan as i32) as f32 / 4.0);
                }

                if self.cells.count() > new_plan {
                    row.add_note(&format!("Overspent by {}", (self.cells.count() - new_plan) as f32 / 4.0));
                }

                let left: i32 = new_plan as i32 - done as i32;
                if left != 0 {
                    row.set_left(left as f32 / 4.0);
                }

            } else {
                // For most resourcing strategies, the value in the plan
                // is fixed.
                row.set_plan(p as f32 / 4.0);

                if let Some(old_p) = self.initial_plan {
                    row.set_gain((old_p as i32 - p as i32) as f32 / 4.0);
                }

                if self.cells.count() > p {
                    row.add_note(&format!("Overspent by {}", (self.cells.count() - p) as f32 / 4.0));
                }

                let left: i32 = p as i32 - done as i32;
                if left != 0 {
                    row.set_left(left as f32 / 4.0);
                }
            }
        }

        for n in self.notes
                .iter() {
            row.add_note(n);
        }

        context.add_row(row);

        Ok(())
    }
}
