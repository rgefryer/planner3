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
    fn new() -> RootConfigData {
        RootConfigData {
            weeks: 0,
            now: 0,
            start_date: ChartDate::new(),
            manager: None,
            labels: Vec::new(),
            developers: HashMap::new()
        }
    }

    fn add_label(&mut self, defn: &str) -> Result<()> {
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


    fn add_developer(&mut self, name: &str, period: &ChartPeriod) -> Result<()> {

        if self.developers.contains_key(name) {
            bail!("Can't re-define a developer");
        }

        let dev = DeveloperData::new(self.weeks*20, period).chain_err(|| format!("Can't add developer {}", name))?;
        self.developers.insert(name.to_string(), dev);
        Ok(())
    }

    fn is_valid_developer(&self, name: &str) -> bool {
        name == "outsource" || self.developers.contains_key(name)
    }

    fn is_valid_cell(&self, cell: u32) -> bool {
        cell < 20 * self.weeks
    }
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


struct NodeConfigData {
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

    done: Vec<DoneEntry>,

    earliest_start: u32,

    latest_end: u32
}

impl NodeConfigData {
    fn new(num_cells: u32) -> NodeConfigData {
        NodeConfigData { 
            notes: Vec::new(), 
            budget: None, 
            scheduling: SchedulingStrategy::Parallel,
            resourcing: ResourcingStrategy::FrontLoad,
            managed: true,
            dev: None,
            plan: Vec::new(),
            default_plan: Vec::new(),
            done: Vec::new(),
            earliest_start: 0,
            latest_end: num_cells,
            cells: ChartRow::new(num_cells)
        }
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

    fn set_dev(&mut self, root: &RootConfigData, dev: &String) -> Result<()> {
        if !root.is_valid_developer(dev) {
            bail!(format!("Developer \"{}\" not known", dev));
        }

        self.dev = Some(dev.clone());
        Ok(())
    }

    fn add_attribute(&mut self, root: &RootConfigData, key: &String, value: &String) -> Result<()> {

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
}

pub struct ConfigNode {
    pub name: String,
    line_num: u32,
    indent: u32,
    level: u32, // Root node is level 0

    pub root_data: Option<RootConfigData>,
    node_data: Option<NodeConfigData>,
}

// Avoid unnecessary recompilation of the regular expressions
lazy_static! {
    static ref ROOT_NODE_RE: Regex = Regex::new(r"^\[(?P<name>(?:global)|(?:devs))\]$").unwrap();
    static ref PLAN_RE: Regex = Regex::new(r"^(?:(?P<date>\d+(?:/\d){0,2}):)?(?P<time>\d+(?:\.\d{1,2})?)(?P<suffix>pc[ym])?$").unwrap();
    static ref DONE_RE: Regex = Regex::new(r"^(?:(?P<date>\d+(?:/\d){0,2}):)(?P<time>\d+(?:\.\d{1,2})?)$").unwrap();
    static ref LABEL_RE: Regex = Regex::new(r"^(?:(?P<date>\d+(?:/\d){0,2}):\s*)(?P<text>.*)$").unwrap();
}

impl ConfigNode {
    fn new(name: &str, level: u32, indent: u32, line_num: u32, is_root: bool, num_cells: u32) -> ConfigNode {
        ConfigNode {
            name: name.to_string(),
            line_num: line_num,
            indent: indent,
            level: level,
            root_data: if is_root {
                Some(RootConfigData::new())
            } else {
                None
            },
            node_data: if is_root {
                None
            } else {
                Some(NodeConfigData::new(num_cells))
            },
            //attributes: HashMap::new(),
            //people: HashMap::new(),
            //cells: ChartTimeRow::new(),
            //period: None,
        }

    }

    fn add_attribute(&mut self, root: &RootConfigData, key: &String, value: &String) -> Result<()> {

        if let Some(ref mut node_data) = self.node_data {
            node_data.add_attribute(root, key, value)?;
        } else {
            bail!("Attempt to define attribute on root node");
        }

        Ok(())
    }

    /// Generate a new node, and all children
    ///
    /// Panics if called with !is_root, but the next line of config is
    /// not a Node.
    pub fn new_from_config<'a, 'b>
        (arena: &'a typed_arena::Arena<arena_tree::Node<'a, RefCell<ConfigNode>>>,
         config: &'b mut file::ConfigLines,
         root: Option<&RootConfigData>,
         is_root: bool,
         level: u32)
-> Result<&'a arena_tree::Node<'a, RefCell<ConfigNode>>>{

        // Create this node
        let mut node_indent = 0u32;
        let mut node_line_num = 0u32;
        let node: &'a arena_tree::Node<'a, RefCell<ConfigNode>> = if is_root {
            arena.alloc(arena_tree::Node::new(RefCell::new(ConfigNode::new("root",
                                                                           0,
                                                                           0,
                                                                           0,
                                                                           is_root,
                                                                           0))))
        } else {
            if let Some(file::Line::Node(file::LineNode { line_num, indent, name })) =
                config.get_line() {
                node_indent = indent;
                node_line_num = line_num;
                arena.alloc(arena_tree::Node::new(RefCell::new(ConfigNode::new(&name,
                                                                               level,
                                                                               indent,
                                                                               line_num,
                                                                               is_root,
                                                                               20*root.unwrap().weeks))))
            } else {
                // Should not have been called without a Node to read.
                bail!("Internal error: new_from_config called without a node to read");
            }
        };

        // Add any attributes
        while let Some(file::Line::Attribute(file::LineAttribute { key, value })) =
            config.peek_line() {
            config.get_line();
            node.data
                .borrow_mut()
                .add_attribute(root.unwrap(), &key, &value)
                .chain_err(|| {
                               format!("Failed to add attribute \"{}\" into node at line {}",
                                       &key,
                                       node_line_num)
                           })?;
        }

        // Add any children
        while let Some(file::Line::Node(file::LineNode { line_num, indent, name })) =
            config.peek_line() {
            if indent <= node_indent {
                break;
            }

            if is_root && ROOT_NODE_RE.is_match(&name) {
                node.data
                    .borrow_mut()
                    .read_root_config(config)
                    .chain_err(|| {
                                   format!("Failed to read node containing root config at line {}",
                                           line_num)
                               })?;
            } else {
                if is_root {
                    if let Some(ref root_data) = node.data.borrow().root_data {
                        ConfigNode::create_child(node, arena, config, Some(root_data), level+1, line_num)?;                    
                    }
                } else {
                    ConfigNode::create_child(node, arena, config, root, level+1, line_num)?;                    
                }
            }
        }

        Ok(node)
    }

    fn create_child<'a, 'b>(node: &'a arena_tree::Node<'a, RefCell<ConfigNode>>,
arena: &'a typed_arena::Arena<arena_tree::Node<'a, RefCell<ConfigNode>>>,
         config: &'b mut file::ConfigLines,
         root: Option<&RootConfigData>,
         level: u32,
         line_num: u32        
     ) -> Result<()> {

        let child: &'a arena_tree::Node<'a, RefCell<ConfigNode>> =
            ConfigNode::new_from_config(arena, config, root, false, level)?;
        node.append(child);
        Ok(())
    }

    // Handle any "nodes" that define config at the root level
    fn read_root_config(&mut self, mut config: &mut file::ConfigLines) -> Result<()> {

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
                if let Some(ref mut x) = self.root_data {
                    x.weeks = value.parse::<u32>()
                        .chain_err(|| "Error parsing \"weeks\" from [chart] node")?;
                }
            } else if key == "now" {
                let ct = value.parse::<ChartTime>()
                    .chain_err(|| "Error parsing \"now\" from [chart] node")?;
                if let Some(ref mut x) = self.root_data {
                    x.now = ct.to_u32();
                }
            } else if key == "manager" {
                if let Some(ref mut x) = self.root_data {
                    x.manager = Some(value.to_string());
                }
            } else if key == "label" {
                if let Some(ref mut x) = self.root_data {
                    x.add_label(&value);
                }
            } else if key == "start-date" {
                let dt = value.parse::<ChartDate>()
                    .chain_err(|| "Error parsing \"start-date\" from [chart] node")?;
                if let Some(ref mut x) = self.root_data {
                    x.start_date = dt;
                }
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
            if let Some(ref mut x) = self.root_data {
                x.add_developer(&key, &cp).chain_err(|| format!("Error adding \"{}\" in [devs] node", key))?;
            }
        }

        // Check that the manager has been defined
        if let Some(ref root_data) = self.root_data {
            if let Some(ref manager) = root_data.manager {
                if !root_data.developers.contains_key(manager) {
                    bail!(format!("Manager \"{}\" not defined as a dev", manager));
                }
            }
        }

        Ok(())
    }
}
