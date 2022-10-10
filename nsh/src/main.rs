#[macro_use]
extern crate clap;

use clap::{Parser, Subcommand};

#[derive(Clone, Debug)]
#[derive(Parser)]
#[command(author, version, about)]
struct Args {
    #[command(subcommand)]
    command: Command
}

#[derive(Clone, Eq, PartialEq, Debug)]
#[derive(Subcommand)]
enum Command {
    /// does testing things
    Test {
        /// lists test values
        #[arg(short, long)]
        list: bool,
    },
}

fn main() {
    let args = Args::parse();
}
