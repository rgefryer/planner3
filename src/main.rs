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
extern crate typed_arena;
extern crate arena_tree;

mod file;
mod nodes;

fn main() {

    match file::ConfigLines::new_from_file("config.txt") {
        Ok(mut config) => {
            let arena = typed_arena::Arena::new();
            match nodes::ConfigNode::new_from_config(&arena, &mut config, true, 0) {
                Ok(root) => {
                    for x in root.descendants() {
                        println!("{}", x.data.borrow().name);
                    }
                    println!("Successful");
                }
                Err(e) => {
                    println!("Failed: {}", e);
                }
            };
        }
        Err(e) => {
            println!("Failed: {}", e);
        }
    };
}
