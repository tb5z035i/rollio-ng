#![allow(dead_code)]

mod devices;
mod discovery;
mod dispatch;
mod overview;
mod pairings;
mod runtime;
mod save;
mod settings;
mod state;
mod subpanel;

pub use runtime::run;

#[cfg(test)]
mod tests;
