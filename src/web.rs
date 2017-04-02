use std::cell::RefCell;

use rocket;
use rocket_contrib::Template;
use typed_arena;
use arena_tree;

use errors::*;
use nodes;
use nodes::data;
use nodes::root::{RootConfigData, BorderType};
use file;
use charttime;

#[derive(Serialize)]
pub struct TemplateRow {
    what: String,
    who: String,
    line_num: u32,
    done: String,
    left: String,
    plan: String,
    gain: String,
    even: bool,
    notes: Vec<String>,
    notes_html: String,
    cells: Vec<(String, String)>,
}

impl TemplateRow {
    pub fn new(indent: u32, line_num: u32, name: &str) -> TemplateRow {
        TemplateRow {
            what: format!("{}{}",
                          format!("{:width$}", " ", width = (indent * 3) as usize),
                          name)
                    .replace(" ", "&nbsp;"),
            who: "".to_string(),
            done: " ".to_string(),
            gain: " ".to_string(),
            line_num: line_num,
            left: " ".to_string(),
            plan: " ".to_string(),
            even: false,
            cells: Vec::new(),
            notes: Vec::new(),
            notes_html: String::new(),
        }
    }

    pub fn set_who(&mut self, who: &str) {
        self.who = who.to_string();
    }

    fn format_f32(val: f32) -> String {
        if val.abs() < 0.01 {
            String::new()
        } else {
            format!("{:.2}", val).replace(".00", "&nbsp;&nbsp;&nbsp;").replace(".50", ".5&nbsp;")
        }
    }

    pub fn add_cell(&mut self, root: &RootConfigData, val: f32) {
        let style = TemplateContext::cell_border_style(root, 1+self.cells.len() as u32);
        self.cells.push((style, TemplateRow::format_f32(val)));
    }

    pub fn add_note(&mut self, val: &str) {
        self.notes.push(val.to_string());
    }

    pub fn set_done(&mut self, done: f32) {
        self.done = TemplateRow::format_f32(done);
    }

    pub fn set_gain(&mut self, gain: f32) {
        self.gain = TemplateRow::format_f32(gain);
    }

    pub fn set_left(&mut self, left: f32) {
        self.left = TemplateRow::format_f32(left);
    }

    pub fn set_plan(&mut self, plan: f32) {
        self.plan = TemplateRow::format_f32(plan);
    }

    fn prepare_html(&mut self) {

        self.notes_html = String::new();
        if self.notes.len() == 0 {
            return;
        }

        self.notes_html.push_str(&format!("Node at line {}", self.line_num));

        for note in &self.notes {
            // @@@ Improve formatting on multi-line notes

            self.notes_html.push_str("<br>");
            self.notes_html.push_str(&note);
        }


    }
}

#[derive(Serialize)]
pub struct TemplateContext {
    // Tuples of (style, content)
    cell_headers: Vec<(String, String)>,

    // Tuples of (colspan, style, content)
    cell_labels: Vec<(u32, String, String)>,

    rows: Vec<TemplateRow>,
}

impl TemplateContext {

    fn cell_border_style(root: &RootConfigData, week: u32) -> String {
        match root.weekly_left_border(week) {
            BorderType::None => "grid".to_string(),
            BorderType::Start => "grid border".to_string(),
            BorderType::Now => "grid start".to_string(),
            BorderType::Label => "grid label".to_string(),
        }
    }

    pub fn new(root: &RootConfigData) -> TemplateContext {

        let mut t = TemplateContext { cell_headers: Vec::new(), cell_labels: Vec::new(), rows: Vec::new() };

        for s in 1..root.get_weeks() + 1 {
            let style = TemplateContext::cell_border_style(root, s);
            t.cell_headers.push((style, s.to_string()));
        }

        let mut colspan = 0;
        let mut last_style: Option<String> = None;
        let mut last_note: Option<String> = None;
        for s in 1..root.get_weeks() + 1 {
            let style = TemplateContext::cell_border_style(root, s);
            colspan += 1;
            if style != "grid" {
                // Complete the current span
                if let Some(style) = last_style {
                    if let Some(note) = last_note {
                        t.cell_labels.push((colspan, style, note));
                    } else {
                        t.cell_labels.push((colspan, style, "".to_string()));
                    }
                }

                // Start on the next span
                last_style = Some(style);
                last_note = root.weekly_label(s);
                colspan = 0;
            }
        }

        // Complete the current span
        if let Some(note) = last_note {
            t.cell_labels.push((colspan+1, last_style.unwrap(), note));
        } else {
            t.cell_labels.push((colspan+1, last_style.unwrap(), "".to_string()));
        }
        
        t
    }

    pub fn add_row(&mut self, mut row: TemplateRow) {
        row.even = self.rows.len() % 2 == 1;
        self.rows.push(row);
    }

    fn prepare_html(&mut self) {
        for row in &mut self.rows {
            row.prepare_html();
        }
    }
}


fn generate_weekly_output<'a, 'b, 'c>(node: &'a arena_tree::Node<'a, RefCell<nodes::ConfigNode>>, 
                          root_data: &'c RootConfigData,
                          mut context: &'b mut TemplateContext) -> Result<()> {

    let name = node.data.borrow().name.clone();
    let line_num = node.data.borrow().line_num;
    let level = node.data.borrow().level;
    if let Some(ref node_data) = node.data.borrow().node_data {
        node_data.generate_weekly_output(root_data, name, line_num, level, &mut context);
    } else {
        bail!("Internal error - no node_data");
    }

    for child in node.children() {
        generate_weekly_output(child, root_data, context);
    }

    Ok(())
}

#[cfg(not(test))]
fn generate_chart_html<'a>(root: &'a arena_tree::Node<'a, RefCell<nodes::ConfigNode>>) -> Result<Template> {

    let root_node = root.data.borrow();
    if let Some(ref root_data) = root_node.root_data {
        let mut context = TemplateContext::new(root_data);
        root_data.generate_dev_weekly_output(&mut context);

        for child in root.children() {
            generate_weekly_output(child, root_data, &mut context);
        }

        // Do any required preparation before rendering
        context.prepare_html();

        return Ok(Template::render("index", &context));
    }
    bail!("No root data defined");
}


#[derive(Serialize)]
pub struct ErrorTemplate {
    error: String,
}


fn get_index_html() -> Result<Template> {

    // While reading and parsing the config, we generate errors, which cause
    // the processing to be abandoned.
    let mut config =
        file::ConfigLines::new_from_file("config.txt").chain_err(|| "Failed to read config")?;
    let arena = typed_arena::Arena::new();
    let root = nodes::ConfigNode::new_from_config(&arena, &mut config, None, true, 0)
        .chain_err(|| "Failed to set up nodes")?;

    // Only critical errors from now on.  Further problems are displayed in the chart.
    let template = generate_chart_html(&root).chain_err(|| "Error generating output")?;
    Ok(template)
}

/// Unwrap the chained error into one big string, and display it.
#[cfg(not(test))]
fn generate_error_html(e: &Error) -> Template {

    let mut error: String = format!("Error: {}", e);
    for e in e.iter().skip(1) {
        error = format!("{}<br>caused by: {}", error, e);
    }

    // The backtrace is not always generated. Try to run this example
    // with `RUST_BACKTRACE=1`.
    if let Some(backtrace) = e.backtrace() {
        error = format!("{}<br><br>backtrace: {:?}", error, backtrace);
    }

    Template::render("err", &ErrorTemplate { error })
}

#[cfg(not(test))]
#[get("/")]
fn index() -> Template {

    match get_index_html() {
        Ok(template) => template,
        Err(e) => generate_error_html(&e)
    }

}

#[cfg(not(test))]
pub fn serve_web() {
    rocket::ignite().mount("/", routes![index]).launch();
}
