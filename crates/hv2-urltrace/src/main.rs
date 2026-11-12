#![forbid(unsafe_code)]

mod cli;
mod fetch;
mod hints;
mod http;
mod net;
mod report;
mod tls;
mod url;

use std::io::Write;
use std::process::ExitCode;

fn main() -> ExitCode {
    let cfg = match cli::Config::from_args(std::env::args().skip(1)) {
        Ok(cfg) => cfg,
        Err(message) => {
            eprintln!("hv2-urltrace: {message}\n{}", cli::USAGE);
            return ExitCode::from(2);
        }
    };
    let report = fetch::run(&cfg);
    let mut out = std::io::stdout().lock();
    let written = serde_json::to_writer(&mut out, &report)
        .map_err(std::io::Error::from)
        .and_then(|()| out.write_all(b"\n"))
        .and_then(|()| out.flush());
    match written {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("hv2-urltrace: writing the report: {e}");
            ExitCode::from(1)
        }
    }
}
