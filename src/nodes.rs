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
}

impl RootConfigData {
    fn new() -> RootConfigData {
        RootConfigData {
            weeks: 0,
            now: 0,
            start_date: ChartDate::new(),
            manager: None,
            developers: HashMap::new()
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
        self.developers.contains_key(name)
    }
}

struct NodeConfigData {
    // Cells are only used on leaf nodes
    //cells: ChartTimeRow

    // Period during which the task can be worked on
    //period: Option<ChartPeriod>

    // Budget, in quarter days
    budget: Option<u32>,

    scheduling: SchedulingStrategy,

    resourcing: ResourcingStrategy,

    // Flag that this task requires management oversight
    managed: bool,

    // Notes are problems to display on the chart
    notes: Vec<String>,

    dev: Option<String>,

}

impl NodeConfigData {
    fn new() -> NodeConfigData {
        NodeConfigData { 
            notes: Vec::new(), 
            budget: None, 
            scheduling: SchedulingStrategy::Parallel,
            resourcing: ResourcingStrategy::FrontLoad,
            managed: true,
            dev: None }
    }

    fn set_budget(&mut self, budget: f32) -> Result<()> {

        if budget < 0.0 {
            bail!("Budget must be >= 0");
        }

        self.budget = Some((budget * 4.0).round() as u32);
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

    root_data: Option<RootConfigData>,
    node_data: Option<NodeConfigData>,

    num_attrs: u32,
}

// Avoid unnecessary recompilation of the regular expressions
lazy_static! {
    static ref ROOT_NODE_RE: Regex = Regex::new(r"^\[(?P<name>(?:global)|(?:devs))\]$").unwrap();
}

impl ConfigNode {
    fn new(name: &str, level: u32, indent: u32, line_num: u32, is_root: bool) -> ConfigNode {
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
                Some(NodeConfigData::new())
            },
            //attributes: HashMap::new(),
            //people: HashMap::new(),
            //cells: ChartTimeRow::new(),
            //period: None,
            num_attrs: 0,
        }

    }

    fn add_attribute(&mut self, root: &RootConfigData, key: &String, value: &String) -> Result<()> {

        if let Some(ref mut node_data) = self.node_data {
            node_data.add_attribute(root, key, value).chain_err(|| "Failed to add attribute")?;
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
                                                                           is_root))))
        } else {
            if let Some(file::Line::Node(file::LineNode { line_num, indent, name })) =
                config.get_line() {
                node_indent = indent;
                node_line_num = line_num;
                arena.alloc(arena_tree::Node::new(RefCell::new(ConfigNode::new(&name,
                                                                               level,
                                                                               indent,
                                                                               line_num,
                                                                               is_root))))
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
                               format!("Failed to add attribute \"{}\"",
                                       &key)
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
            ConfigNode::new_from_config(arena, config, root, false, level).chain_err(|| {
                               format!("Failed to generate child node \
                                       from config at line {}",
                                       line_num)
                           })?;
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
