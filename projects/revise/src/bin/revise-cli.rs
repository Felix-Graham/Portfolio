//! `revise-cli` — direct request: "an API... It should be callable
//! from other applications and through the terminal from anywhere...
//! This would allow other revision applications... to be registered
//! and accounted for."
//!
//! A minimal, GUI-less binary, same idea as `revise-reminderd` applied
//! to writes instead of reads: it opens the exact same SQLite database
//! the main app does and exits — no network server, no daemon to keep
//! running, nothing beyond a single call. Every view in the app
//! already queries the database fresh on each render, so whatever this
//! writes shows up in the Dashboard's charts on its very next repaint.
//! All the actual parsing and database logic lives in `revise::cli`,
//! unit-tested there — this file is deliberately just argv in, an exit
//! code out.

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();

    let command = match revise::cli::parse_args(&args) {
        Ok(command) => command,
        Err(message) => {
            eprintln!("{message}");
            std::process::exit(1);
        }
    };

    let db = match revise::database::Database::open_default() {
        Ok(db) => db,
        Err(err) => {
            eprintln!("revise-cli: could not open the Revise database ({err})");
            std::process::exit(1);
        }
    };

    match revise::cli::run(&db, command) {
        Ok(message) => println!("{message}"),
        Err(message) => {
            eprintln!("{message}");
            std::process::exit(1);
        }
    }
}
