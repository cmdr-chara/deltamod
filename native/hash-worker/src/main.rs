#![forbid(unsafe_code)]

use std::env;
use std::io::{self, Write};
use std::path::Path;

fn main() {
    let Some(root) = env::args_os().nth(1) else {
        eprintln!("usage: deltamod-hash-worker <game-root>");
        std::process::exit(2);
    };

    let stdout = io::stdout();
    let mut output = stdout.lock();
    let result = deltamod_hash_worker::run(Path::new(&root), |event| {
        serde_json::to_writer(&mut output, event)?;
        output.write_all(b"\n")?;
        output.flush()
    });
    if let Err(error) = result {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
