use std::cell::RefCell;
use file;
use typed_arena;
use arena_tree;
use regex::Regex;

pub struct ConfigNode {
    pub name: String,
    line_num: u32,
    indent: u32,
    level: u32, // Root node is level 0

    // People are only defined on the root node
    //people: HashMap<String, PersonData>,

    // Cells are only used on leaf nodes
    //cells: ChartTimeRow,

    // Period during which the task can be worked on
    //period: Option<ChartPeriod>,

    // Notes are problems to display on the chart
    notes: Vec<String>,

    num_attrs: u32,
}

// Avoid unnecessary recompilation of the regular expressions
lazy_static! {
    static ref ROOT_NODE_RE: Regex = Regex::new(r"^\[(?P<name>(?:chart)|(?:people))\]$").unwrap();
}

impl ConfigNode {
    fn new(name: &str, level: u32, indent: u32, line_num: u32) -> ConfigNode {
        ConfigNode {
            name: name.to_string(),
            line_num: line_num,
            indent: indent,
            level: level,
            //attributes: HashMap::new(),
            //people: HashMap::new(),
            //cells: ChartTimeRow::new(),
            //period: None,
            notes: Vec::new(),
            num_attrs: 0,
        }

    }

    fn add_attribute(&mut self, key: String, value: String) -> Result<(), String> {

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
         -> Result<&'a arena_tree::Node<'a, RefCell<ConfigNode>>, String> {

        // Create this node
        let mut node_indent = 0u32;
        let node: &'a arena_tree::Node<'a, RefCell<ConfigNode>> = if is_root {
            arena.alloc(arena_tree::Node::new(RefCell::new(ConfigNode::new("root", 0, 0, 0))))
        } else {
            if let Some(file::Line::Node(file::LineNode { line_num, indent, name })) =
                config.get_line() {
                node_indent = indent;
                arena.alloc(arena_tree::Node::new(RefCell::new(ConfigNode::new(&name,
                                                                               level,
                                                                               indent,
                                                                               line_num))))
            } else {
                // Should not have been called without a Node to read.
                return Err("Internal error: new_from_config called without a node to read"
                               .to_string());
            }
        };

        // Add any attributes
        while let Some(file::Line::Attribute(file::LineAttribute { key, value })) =
            config.peek_line() {
            config.get_line();
            try!(node.data.borrow_mut().add_attribute(key, value));
        }

        // Add any children
        while let Some(file::Line::Node(file::LineNode { line_num: _, indent, name })) =
            config.peek_line() {
            if indent <= node_indent {
                break;
            }

            if is_root && ROOT_NODE_RE.is_match(&name) {
                try!(node.data.borrow_mut().read_root_config(config));
            } else {
                let child: &'a arena_tree::Node<'a, RefCell<ConfigNode>> =
                    try!(ConfigNode::new_from_config(arena, config, false, level + 1));
                node.append(child);
            }
        }

        Ok(node)
    }

    // Handle any "nodes" that define config at the root level
    fn read_root_config(&mut self, mut config: &mut file::ConfigLines) -> Result<(), String> {

        if let Some(file::Line::Node(file::LineNode { line_num, indent, name })) =
            config.get_line() {

            let c = ROOT_NODE_RE.captures(&name).unwrap();
            if &c["name"] == "chart" {
                try!(self.read_chart_config(&mut config));
            } else if &c["name"] == "people" {
                try!(self.read_people_config(&mut config));
            } else {
                return Err(format!("Internal error: read_root_config \
                                    called with unexpected node {} at line {}",
                                   name,
                                   line_num));
            }
        } else {
            // Should not have been called without a Node to read.
            return Err("Internal error: read_root_config called without a node to read"
                           .to_string());
        }

        Ok(())
    }

    /// Store any configuration stored under [chart]
    fn read_chart_config(&mut self, config: &mut file::ConfigLines) -> Result<(), String> {
        println!("Reading chart config");
        while let Some(file::Line::Attribute(file::LineAttribute { key, value })) =
            config.peek_line() {
            config.get_line();
            try!(self.add_attribute(key, value));
        }
        Ok(())
    }

    /// Store any configuration stored under [people]
    fn read_people_config(&mut self, config: &mut file::ConfigLines) -> Result<(), String> {
        println!("Reading people config");
        while let Some(file::Line::Attribute(file::LineAttribute { key, value })) =
            config.peek_line() {
            config.get_line();
            try!(self.add_attribute(key, value));
        }
        Ok(())
    }
}
