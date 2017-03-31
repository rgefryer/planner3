#![feature(plugin)]
#![plugin(rocket_codegen)]

// `error_chain!` can recurse deeply
#![recursion_limit = "1024"]

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
extern crate chrono;

// Import the macro. Don't forget to add `error-chain` in your
// `Cargo.toml`!
#[macro_use]
extern crate error_chain;

mod file;
mod nodes;
mod errors;
mod charttime;
mod chartdate;
mod chartperiod;
mod chartrow;

use errors::*;

// Standard main function for outputting chained errors.  See
// run() for the actual work.
fn main() {
    if let Err(ref e) = run() {
        use std::io::Write;
        let stderr = &mut ::std::io::stderr();
        let errmsg = "Error writing to stderr";

        writeln!(stderr, "error: {}", e).expect(errmsg);

        for e in e.iter().skip(1) {
            writeln!(stderr, "caused by: {}", e).expect(errmsg);
        }

        // The backtrace is not always generated. Try to run this example
        // with `RUST_BACKTRACE=1`.
        if let Some(backtrace) = e.backtrace() {
            writeln!(stderr, "backtrace: {:?}", backtrace).expect(errmsg);
        }

        ::std::process::exit(1);
    }
}

// Main work function for the app
fn run() -> Result<()> {

    // Test code in development by reading in the config file and building
    // the node tree.
    let mut config =
        file::ConfigLines::new_from_file("config.txt").chain_err(|| "Failed to read config")?;
    let arena = typed_arena::Arena::new();
    let root = nodes::ConfigNode::new_from_config(&arena, &mut config, None, true, 0)
        .chain_err(|| "Failed to set up nodes")?;

    // Iterate through the node tree to demonstrate that it exists
    for x in root.descendants() {
        println!("{}", x.data.borrow().name);
    }

    Ok(())
}
