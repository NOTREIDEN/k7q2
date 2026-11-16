//! Compare the simulator with recordings of the real game.
//!
//!     cargo run --release --example parity -- TRACE... [--resync] [--verbose]
//!         [--client | --server] [--wrong-knockback]
//!
//! Exits with 0 only if every recording is reproduced bit for bit.
use pvpsim::replay::{read_trace, replay, Options};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let has = |f: &str| args.iter().any(|a| a == f);
    let o = Options {
        resync: has("--resync"),
        verbose: has("--verbose"),
        wrong_knockback: has("--wrong-knockback"),
        skip_client: has("--server"),
        skip_server: has("--client"),
    };
    let files: Vec<&String> = args.iter().filter(|a| !a.starts_with("--")).collect();
    if files.is_empty() {
        eprintln!("usage: parity TRACE... [--resync] [--verbose] [--client | --server] [--wrong-knockback]");
        std::process::exit(2);
    }
    let mut ok = true;
    for f in files {
        println!("{f}");
        match read_trace(f).and_then(|t| replay(&t, &o)) {
            Ok(r) => {
                print!("{}", r.describe());
                ok &= r.identical();
            }
            Err(e) => {
                println!("  {e}");
                ok = false;
            }
        }
    }
    std::process::exit(if ok { 0 } else { 1 });
}
