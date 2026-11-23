//! Feeds stdin to a terminal and prints what ends up on its screen.
//!
//! ```sh
//! printf 'a\tb\r\n\033[1mbold\033[0m' | cargo run -p artalde-vt --example dump -- --cols 40 --rows 5
//! ```

use std::io::{Read, Write};
use std::process::ExitCode;

use artalde_vt::{Options, Vt};

fn main() -> ExitCode {
    let mut options = Options::default();
    let mut history = 0usize;
    let mut args = std::env::args().skip(1);
    while let Some(flag) = args.next() {
        let value = args.next();
        let parsed = match flag.as_str() {
            "--cols" => value.and_then(|v| v.parse().ok()).map(|v| options.cols = v),
            "--rows" => value.and_then(|v| v.parse().ok()).map(|v| options.rows = v),
            "--history" => value.and_then(|v| v.parse().ok()).map(|v| history = v),
            _ => None,
        };
        if parsed.is_none() {
            eprintln!("usage: dump [--cols N] [--rows N] [--history LINES] < input");
            return ExitCode::from(2);
        }
    }

    let mut vt = match Vt::new(&options) {
        Ok(vt) => vt,
        Err(error) => {
            eprintln!("dump: {error}");
            return ExitCode::FAILURE;
        }
    };
    let mut stdin = std::io::stdin().lock();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        match stdin.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => vt.write(&buffer[..read]),
            Err(error) => {
                eprintln!("dump: {error}");
                return ExitCode::FAILURE;
            }
        }
    }
    let mut stdout = std::io::stdout().lock();
    if writeln!(stdout, "{}", vt.text(history)).is_err() {
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}
