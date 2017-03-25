use std::io::prelude::*;
use std::io::BufReader;
use std::fs::File;

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct LineNode {
    pub line_num: u32,
    pub indent: u32,
    pub name: String,
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct LineAttribute {
    pub key: String,
    pub value: String,
}

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

    pub fn new_from_file(filename: &str) -> Result<ConfigLines, String> {

        let f = try!(File::open(filename).map_err(|e| {
                                                      format!("Error opening {}, {}",
                                                              filename,
                                                              e.to_string())
                                                  }));
        let mut file_data = ConfigLines::new();
        let mut line_num = 0;

        let reader = BufReader::new(f);
        for line_rc in reader.lines() {

            line_num += 1;
            let line = try!(line_rc.map_err(|e| e.to_string()));
            try!(file_data.process_line(&line, line_num));
        }

        Ok(file_data)
    }

    fn process_line(&mut self, input_line: &str, line_num: u32) -> Result<(), String> {

        let mut line = input_line;

        // Discard trailing comments
        match line.find('#') {
            None => {}
            Some(ix) => {
                line = &line[0..ix];
            }
        };

        // Trim the RHS
        line = line.trim_right();

        // Get the length
        let len_with_indent = line.len();

        // Trim the LHS
        line = line.trim_left();

        // Get the indent
        let indent = len_with_indent - line.len();

        // Discard empty lines
        if line.len() == 0 {
            return Ok(());
        }

        // Work out if this is a node or an attribute.
        let node = match line.find("- ") {
            Some(0) => false,
            _ => true,
        };

        // If new node, write note line
        if node {
            self.add_line(Line::new_node_line(line_num, (indent + 1) as u32, line));
        }
        // Else if attribute, splt the attribute and values and
        // write attribute line
        else {
            line = line[2..].trim_left();
            match line.find(':') {
                Some(0) => return Err("Attribute with no key".to_string()),
                Some(pos) => {
                    let attr_name = line[..pos].trim();
                    let attr_val = line[pos + 1..].trim();

                    self.add_line(Line::new_attribute_line(attr_name, attr_val));
                }
                None => return Err("Attribute with no value".to_string()),
            };
        }

        Ok(())
    }
}
