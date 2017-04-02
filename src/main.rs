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
mod web;    

// Standard main function for outputting chained errors.  See
// run() for the actual work.
fn main() {
    web::serve_web();
}
