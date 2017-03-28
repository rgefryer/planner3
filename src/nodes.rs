use std::cell::RefCell;
use file;
use typed_arena;
use arena_tree;
use regex::Regex;
use errors::*;
use charttime::ChartTime;

struct RootConfigData {
    // People are only defined on the root node
    //people: HashMap<String, PersonData>,
    weeks: u32,

    // Today
    now: u32,
}

impl RootConfigData {
    fn new() -> RootConfigData {
        RootConfigData { weeks: 0, now: 0 }
    }
}

struct NodeConfigData {
    // Cells are only used on leaf nodes
    //cells: ChartTimeRow,

    // Period during which the task can be worked on
    //period: Option<ChartPeriod>,
    
    // Notes are problems to display on the chart
    notes: Vec<String>,
}

impl NodeConfigData {
    fn new() -> NodeConfigData {
        NodeConfigData { notes: Vec::new() }
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
    static ref ROOT_NODE_RE: Regex = Regex::new(r"^\[(?P<name>(?:chart)|(?:people))\]$").unwrap();
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

    fn add_attribute(&mut self, key: &String, value: &String) -> Result<()> {

        // Nonsense code to avoid compiler complaints
        self.num_attrs += 1;
        if key == value {
            self.num_attrs += 2;
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
                .add_attribute(&key, &value)
                .chain_err(|| {
                               format!("Failed to add attribute \"{}\" to node at line {}",
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
                let child: &'a arena_tree::Node<'a, RefCell<ConfigNode>> =
                    ConfigNode::new_from_config(arena, config, false, level + 1).chain_err(|| {
                                       format!("Failed to generate child node \
                                               from config at line {}",
                                               line_num)
                                   })?;
                node.append(child);
            }
        }

        Ok(node)
    }

    // Handle any "nodes" that define config at the root level
    fn read_root_config(&mut self, mut config: &mut file::ConfigLines) -> Result<()> {

        if let Some(file::Line::Node(file::LineNode { line_num: _, indent: _, name })) =
            config.get_line() {

            let c = ROOT_NODE_RE.captures(&name).unwrap();
            if &c["name"] == "chart" {
                self.read_chart_config(&mut config).chain_err(|| "Failed to read [chart] node")?;
            } else if &c["name"] == "people" {
                self.read_people_config(&mut config).chain_err(|| "Failed to read [people] node")?;
            } else {
                bail!("Internal error: Unexpected node type");
            }
        } else {
            // Should not have been called without a Node to read.
            bail!("Internal error: read_root_config called without a node to read");
        }

        Ok(())
    }

    /// Store any configuration stored under [chart]
    fn read_chart_config(&mut self, config: &mut file::ConfigLines) -> Result<()> {
        println!("Reading chart config");
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
            } else if key == "start-date" {
                bail!(format!("Unrecognised attribute \"{}\" in [chart] node", key));
            } else {
                bail!(format!("Unrecognised attribute \"{}\" in [chart] node", key));
            }
        }
        Ok(())
    }

    /// Store any configuration stored under [people]
    fn read_people_config(&mut self, config: &mut file::ConfigLines) -> Result<()> {
        println!("Reading people config");
        while let Some(file::Line::Attribute(file::LineAttribute { key, value })) =
            config.peek_line() {

            config.get_line();
            self.add_attribute(&key, &value).chain_err(|| "Failed to add attribute")?;
        }
        Ok(())
    }
}
