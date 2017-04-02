use std::cell::RefCell;
use std::collections::HashMap;
use regex::Regex;

use typed_arena;
use arena_tree;

pub mod root;
pub mod data;

use errors::*;
use file;
use charttime::ChartTime;
use chartdate::ChartDate;
use chartperiod::ChartPeriod;
use chartrow::ChartRow;
use web;
use self::root::RootConfigData;
use self::data::NodeConfigData;

// Avoid unnecessary recompilation of the regular expressions
lazy_static! {
    pub static ref ROOT_NODE_RE: Regex = Regex::new(r"^\[(?P<name>(?:global)|(?:devs))\]$").unwrap();
}

pub struct ConfigNode {
    pub name: String,
    pub line_num: u32,
    indent: u32,
    pub level: u32, // Root node is level 0

    pub root_data: Option<RootConfigData>,
    pub node_data: Option<NodeConfigData>,
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
                                                                               20*root.unwrap().get_weeks()))))
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

    fn add_attribute(&mut self, root: &RootConfigData, key: &String, value: &String) -> Result<()> {

        if let Some(ref mut node_data) = self.node_data {
            node_data.add_attribute(root, key, value)?;
        } else {
            bail!("Attempt to define attribute on root node");
        }

        Ok(())
    }

    // Handle any "nodes" that define config at the root level
    fn read_root_config(&mut self, mut config: &mut file::ConfigLines) -> Result<()> {

        if let Some(ref mut root_data) = self.root_data {
            root_data.read_config(config)?;
        } else {
            bail!("Attempt to read root config on non-root node");
        }

        Ok(())
   }


}
