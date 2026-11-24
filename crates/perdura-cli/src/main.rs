//! Command-line node for the Perdura network.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::fs;
use std::io;
use std::net::{Ipv6Addr, SocketAddr, SocketAddrV6, TcpListener, TcpStream};
use std::os::unix::fs::DirBuilderExt;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::{Duration, Instant};

use perdura_core::channel::{CONTACT_PORT, Channel};
use perdura_core::discovery::{self, Event};
use perdura_core::identity::{Identity, fingerprint};

const USAGE: &str = "usage:
  perdura
  perdura identity [--data-dir <path>]
  perdura discover --ifindex <n> [--duration <seconds>]
  perdura listen [--data-dir <path>] [--duration <seconds>]
  perdura send --to <address> --message <text> [--data-dir <path>]";

const IO_TIMEOUT: Duration = Duration::from_secs(10);
const ACCEPT_POLL: Duration = Duration::from_millis(50);

type Flags = HashMap<String, String>;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        None => {
            println!("perdura {}", perdura_core::version());
            ExitCode::SUCCESS
        }
        Some("identity") => identity(&args[1..]),
        Some("discover") => discover(&args[1..]),
        Some("listen") => listen(&args[1..]),
        Some("send") => send(&args[1..]),
        Some(_) => usage_error(),
    }
}

fn usage_error() -> ExitCode {
    eprintln!("{USAGE}");
    ExitCode::from(2)
}

fn report(error: &io::Error) -> ExitCode {
    eprintln!("error: {error}");
    ExitCode::FAILURE
}

/// Parses `--flag value` pairs. Returns `None` for unknown, repeated or
/// incomplete flags.
fn parse_flags(args: &[String], allowed: &[&str]) -> Option<Flags> {
    let mut flags = Flags::new();
    let mut iter = args.iter();
    while let Some(flag) = iter.next() {
        let value = iter.next()?;
        if !allowed.contains(&flag.as_str()) {
            return None;
        }
        if flags.insert(flag.clone(), value.clone()).is_some() {
            return None;
        }
    }
    Some(flags)
}

/// Reads an optional numeric flag. `Err(())` means the value is not a number.
fn number(flags: &Flags, name: &str) -> Result<Option<u64>, ()> {
    match flags.get(name) {
        None => Ok(None),
        Some(text) => text.parse().map(Some).map_err(|_| ()),
    }
}

fn load_identity(flags: &Flags) -> io::Result<(Identity, bool)> {
    let data_dir = match flags.get("--data-dir") {
        Some(dir) => PathBuf::from(dir),
        None => default_data_dir().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                "cannot determine the data directory; use --data-dir",
            )
        })?,
    };
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(&data_dir)?;
    Identity::load_or_create(&data_dir.join("identity"))
}

fn default_data_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("XDG_DATA_HOME").filter(|value| !value.is_empty()) {
        return Some(PathBuf::from(dir).join("perdura"));
    }
    let home = std::env::var_os("HOME").filter(|value| !value.is_empty())?;
    Some(PathBuf::from(home).join(".local/share/perdura"))
}

fn identity(args: &[String]) -> ExitCode {
    let Some(flags) = parse_flags(args, &["--data-dir"]) else {
        return usage_error();
    };
    match load_identity(&flags) {
        Ok((identity, created)) => {
            println!("identity {}", identity.fingerprint());
            println!("status {}", if created { "created" } else { "loaded" });
            ExitCode::SUCCESS
        }
        Err(error) => report(&error),
    }
}

fn discover(args: &[String]) -> ExitCode {
    let Some(flags) = parse_flags(args, &["--ifindex", "--duration"]) else {
        return usage_error();
    };
    let (Ok(ifindex), Ok(duration)) = (number(&flags, "--ifindex"), number(&flags, "--duration"))
    else {
        return usage_error();
    };
    let Some(ifindex) = ifindex
        .and_then(|index| u32::try_from(index).ok())
        .filter(|index| *index != 0)
    else {
        return usage_error();
    };

    let node_id = match discovery::new_node_id() {
        Ok(id) => id,
        Err(error) => return report(&error),
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
        Err(error) => report(&error),
    }
}

fn listen(args: &[String]) -> ExitCode {
    let Some(flags) = parse_flags(args, &["--data-dir", "--duration"]) else {
        return usage_error();
    };
    let Ok(duration) = number(&flags, "--duration") else {
        return usage_error();
    };
    match run_listen(&flags, duration) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => report(&error),
    }
}

/// Accepts one channel, prints the first message and echoes it back.
fn run_listen(flags: &Flags, duration: Option<u64>) -> io::Result<()> {
    let (identity, _) = load_identity(flags)?;
    let listener = TcpListener::bind(SocketAddrV6::new(Ipv6Addr::UNSPECIFIED, CONTACT_PORT, 0, 0))?;
    listener.set_nonblocking(true)?;
    let deadline = duration.map(|seconds| Instant::now() + Duration::from_secs(seconds));

    let stream = loop {
        match listener.accept() {
            Ok((stream, SocketAddr::V6(peer))) if is_link_local(&peer) => break stream,
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                if deadline.is_some_and(|limit| Instant::now() >= limit) {
                    return Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        "no connection before the deadline",
                    ));
                }
                std::thread::sleep(ACCEPT_POLL);
            }
            Err(error) => return Err(error),
        }
    };
    stream.set_nonblocking(false)?;
    stream.set_read_timeout(Some(IO_TIMEOUT))?;
    stream.set_write_timeout(Some(IO_TIMEOUT))?;

    let mut channel = Channel::respond(&identity, stream)?;
    println!("peer {}", fingerprint(&channel.peer()));
    let message = channel.receive()?;
    println!("received {}", printable(&message));
    channel.send(&message)
}

fn send(args: &[String]) -> ExitCode {
    let Some(flags) = parse_flags(args, &["--data-dir", "--to", "--message"]) else {
        return usage_error();
    };
    let (Some(to), Some(message)) = (flags.get("--to"), flags.get("--message")) else {
        return usage_error();
    };
    let Ok(to) = to.parse::<SocketAddrV6>() else {
        return usage_error();
    };
    match run_send(&flags, to, message) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => report(&error),
    }
}

/// Opens one channel, sends a message and prints the reply.
fn run_send(flags: &Flags, to: SocketAddrV6, message: &str) -> io::Result<()> {
    if !is_link_local(&to) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "destination is not a link-local address",
        ));
    }
    let (identity, _) = load_identity(flags)?;
    let stream = TcpStream::connect_timeout(&SocketAddr::V6(to), IO_TIMEOUT)?;
    stream.set_read_timeout(Some(IO_TIMEOUT))?;
    stream.set_write_timeout(Some(IO_TIMEOUT))?;

    let mut channel = Channel::initiate(&identity, stream)?;
    println!("peer {}", fingerprint(&channel.peer()));
    channel.send(message.as_bytes())?;
    let reply = channel.receive()?;
    println!("reply {}", printable(&reply));
    Ok(())
}

fn is_link_local(addr: &SocketAddrV6) -> bool {
    (addr.ip().segments()[0] & 0xffc0) == 0xfe80
}

/// Renders received bytes for the terminal, replacing anything that is not
/// printable ASCII.
fn printable(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|&byte| {
            if byte == b' ' || byte.is_ascii_graphic() {
                char::from(byte)
            } else {
                '?'
            }
        })
        .collect()
}

fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(out, "{byte:02x}");
    }
    out
}
