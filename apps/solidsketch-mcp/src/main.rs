//! `solidsketch-mcp`: SolidSketch as an MCP server, over stdio (docs/ARCHITECTURE.md §7 septies).
//!
//! ```text
//! solidsketch-mcp                  the running app if its control channel answers, else headless
//! solidsketch-mcp --remote         the running app (found again at each call, even if started later)
//! solidsketch-mcp --connect 7858   the app on that port
//! solidsketch-mcp --headless       an editor of its own, no window
//! solidsketch-mcp --in a.solidsketch   headless, on that document
//! ```
//!
//! In Claude Code: `claude mcp add solidsketch -- <path>/solidsketch-mcp`.

mod backend;
mod base64;
mod mcp;

use std::io::{BufRead, Write};
use std::path::PathBuf;

use backend::{Backend, Headless, Remote};
use solidsketch_control::{Call, Method, Outcome};

const USAGE: &str = "\
solidsketch-mcp: SolidSketch as an MCP server (stdio)
  (no option)        the running app if its control channel answers, else headless
  --remote           the running app, found again at each call
  --connect PORT     the app listening on PORT
  --headless         an editor of its own, without a window
  --in FILE          headless, on the document FILE (.solidsketch)";

impl Backend for Box<dyn Backend> {
    fn call(&mut self, call: &Call) -> Outcome {
        (**self).call(call)
    }

    fn serves(&self, m: &Method) -> bool {
        (**self).serves(m)
    }

    fn describe(&self) -> String {
        (**self).describe()
    }
}

/// What the command line asks for.
#[derive(Debug, Default, PartialEq)]
struct Options {
    remote: bool,
    headless: bool,
    port: Option<u16>,
    input: Option<PathBuf>,
}

fn options(args: impl Iterator<Item = String>) -> Result<Options, String> {
    let mut o = Options::default();
    let mut args = args.peekable();
    while let Some(a) = args.next() {
        match a.as_str() {
            "--remote" => o.remote = true,
            "--headless" => o.headless = true,
            "--connect" => {
                let p = args.next().ok_or("--connect: a port")?;
                o.port = Some(p.parse().map_err(|_| format!("--connect: not a port: {p}"))?);
            }
            "--in" => o.input = Some(PathBuf::from(args.next().ok_or("--in: a file")?)),
            "-h" | "--help" => return Err(USAGE.into()),
            other => return Err(format!("unknown argument {other}\n{USAGE}")),
        }
    }
    if (o.remote || o.port.is_some()) && (o.headless || o.input.is_some()) {
        return Err(format!("--remote / --connect and --headless / --in exclude each other\n{USAGE}"));
    }
    Ok(o)
}

fn backend(o: &Options) -> Result<Box<dyn Backend>, String> {
    if o.headless || o.input.is_some() {
        return Ok(Box::new(Headless::new(o.input.clone())?));
    }
    let remote = Remote::new(o.port);
    if o.remote || o.port.is_some() || remote.reachable() {
        return Ok(Box::new(remote));
    }
    Ok(Box::new(Headless::new(None)?))
}

fn main() {
    let o = match options(std::env::args().skip(1)) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(2);
        }
    };
    let backend = match backend(&o) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("solidsketch-mcp: {e}");
            std::process::exit(1);
        }
    };
    // The log is stderr's: stdout carries the protocol only.
    eprintln!("solidsketch-mcp: {}", backend.describe());
    let mut server = mcp::Server::new(backend);
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout().lock();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        if let Some(reply) = server.handle(&line)
            && writeln!(stdout, "{reply}").and_then(|_| stdout.flush()).is_err()
        {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<Options, String> {
        options(args.iter().map(|a| a.to_string()))
    }

    #[test]
    fn the_command_line() {
        assert_eq!(parse(&[]).unwrap(), Options::default());
        assert_eq!(parse(&["--connect", "7868"]).unwrap().port, Some(7868));
        assert_eq!(parse(&["--in", "a.solidsketch"]).unwrap().input, Some(PathBuf::from("a.solidsketch")));
        assert!(parse(&["--connect", "x"]).is_err());
        assert!(parse(&["--remote", "--headless"]).is_err());
        assert!(parse(&["--nope"]).is_err());
    }
}
