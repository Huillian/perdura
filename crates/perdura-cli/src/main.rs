//! Command-line node for the Perdura network.

use std::fmt::Write as _;
use std::process::ExitCode;
use std::time::{Duration, Instant};

use perdura_core::discovery::{self, Event};

const USAGE: &str = "usage:
  perdura
  perdura discover --ifindex <n> [--duration <seconds>]";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        None => {
            println!("perdura {}", perdura_core::version());
            ExitCode::SUCCESS
        }
        Some("discover") => discover(&args[1..]),
        Some(_) => usage_error(),
    }
}

fn usage_error() -> ExitCode {
    eprintln!("{USAGE}");
    ExitCode::from(2)
}

fn discover(args: &[String]) -> ExitCode {
    let mut ifindex: Option<u32> = None;
    let mut duration: Option<u64> = None;

    let mut iter = args.iter();
    while let Some(flag) = iter.next() {
        let value = iter.next().map(|text| text.parse::<u64>());
        match (flag.as_str(), value) {
            ("--ifindex", Some(Ok(number))) => ifindex = u32::try_from(number).ok(),
            ("--duration", Some(Ok(number))) => duration = Some(number),
            _ => return usage_error(),
        }
    }

    let Some(ifindex) = ifindex.filter(|index| *index != 0) else {
        return usage_error();
    };

    let node_id = match discovery::new_node_id() {
        Ok(id) => id,
        Err(error) => {
            eprintln!("error: {error}");
            return ExitCode::FAILURE;
        }
    };
    println!("node {}", hex(&node_id));

    let deadline = duration.map(|seconds| Instant::now() + Duration::from_secs(seconds));
    let mut on_event = |event: Event| match event {
        Event::PeerFound { id, addr } => println!("found {} {addr}", hex(&id)),
        Event::PeerLost { id } => println!("lost {}", hex(&id)),
    };

    match discovery::run(ifindex, node_id, deadline, &mut on_event) {
        Ok(stats) => {
            println!("stats {stats:?}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(out, "{byte:02x}");
    }
    out
}
