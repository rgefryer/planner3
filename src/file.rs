// Types and methods for reading a config file into data
// structures that can be easily iterated through.
use std::io::prelude::*;
use std::io::BufReader;
use std::fs::File;
use regex::Regex;
use errors::*;

// Data from a line representing a new node
#[derive(Debug, PartialEq, Eq, Clone)]
pub struct LineNode {
    pub line_num: u32,
    pub indent: u32,
    pub name: String,
}

// Data from a line representing a node attribute
#[derive(Debug, PartialEq, Eq, Clone)]
pub struct LineAttribute {
    pub key: String,
    pub value: String,
}

// Enum encapsulating any type of "interesting" line
#[derive(Debug, PartialEq, Eq, Clone)]
pub enum Line {
    Node(LineNode),
    Attribute(LineAttribute),
}

impl Line {
    fn new_node_line(line_num: u32, indent: u32, name: &str) -> Line {
        Line::Node(LineNode {
                       line_num: line_num,
                       indent: indent,
                       name: name.to_string(),
                   })
    }

    fn new_attribute_line(key: &str, value: &str) -> Line {
        Line::Attribute(LineAttribute {
                            key: key.to_string(),
                            value: value.to_string(),
                        })
    }
}

pub struct ConfigLines {
    lines: Vec<Line>,
    pos: usize,
}

impl ConfigLines {
    fn new() -> ConfigLines {
        ConfigLines {
            lines: Vec::new(),
            pos: 0,
        }
    }

    fn add_line(&mut self, line: Line) {
        self.lines.push(line);
    }

    pub fn peek_line(&self) -> Option<Line> {
        if self.lines.len() > self.pos {
            Some(self.lines[self.pos].clone())
        } else {
            None
        }
    }

    pub fn get_line(&mut self) -> Option<Line> {
        if self.lines.len() > self.pos {
            self.pos += 1;
            Some(self.lines[self.pos - 1].clone())
        } else {
            None
        }
    }

    pub fn new_from_file(filename: &str) -> Result<ConfigLines> {

        let f = File::open(filename).chain_err(|| format!("Error opening {}", filename))?;
        let mut file_data = ConfigLines::new();
        let mut line_num = 0;

        let reader = BufReader::new(f);
        for line_rc in reader.lines() {

            line_num += 1;
            let line = try!(line_rc.map_err(|e| e.to_string()));
            file_data.process_line(&line, line_num)
                .chain_err(|| format!("Failed reading {} at line {}", filename, line_num))?;
        }

        Ok(file_data)
    }

    fn process_line(&mut self, input_line: &str, line_num: u32) -> Result<()> {

        // Avoid unnecessary recompilation of the regular expressions
        lazy_static! {
            static ref COMMENT_RE: Regex = Regex::new(r"^(?P<content>[^#]*).*$").unwrap();
            static ref BLANK_RE: Regex = Regex::new(r"^\s*$").unwrap();
            static ref NODE_RE: Regex = Regex::new(r"^(?P<indent>\s*)(?P<name>[\w\]\[/\s]+)$")
                .unwrap();
            static ref ATTR_RE: Regex =
                Regex::new(r"^\s*\-\s*(?P<key>[\w\-\./]+)\s*:\s*(?P<value>.*)$").unwrap();
        }

        // Strip comments, ignore blank lines.
        let content = &COMMENT_RE.captures(input_line).unwrap()["content"];
        if BLANK_RE.is_match(content) {
            return Ok(());
        }

        // Try to parse as a node, or failing that as an attribute
        match NODE_RE.captures(content) {
            Some(c) => {
                let indent = c["indent"].len();
                self.add_line(Line::new_node_line(line_num, (indent + 1) as u32, &c["name"]));
            }
            None => {
                let c = ATTR_RE.captures(content)
                    .ok_or("Unable to parse line as a node or an attribute")?;
                self.add_line(Line::new_attribute_line(&c["key"], &c["value"].trim()));
            }
        };

        Ok(())
    }
}
