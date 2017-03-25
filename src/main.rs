#![feature(plugin)]
#![plugin(rocket_codegen)]
extern crate rocket;
extern crate rocket_contrib;
extern crate serde_json;
#[macro_use]
extern crate serde_derive;
#[macro_use]
extern crate lazy_static;
extern crate regex;
extern crate arena_tree;

mod file;

use file::ConfigLines;

fn main() {

    match ConfigLines::new_from_file("config.txt") {
        Ok(_) => {
            println!("Successful");
        }
        Err(e) => {
            println!("Failed: {}", e);
        }
    };
}
