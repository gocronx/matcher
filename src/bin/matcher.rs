//! Single-product matching engine.
//!
//! Usage:
//!
//!   matcher --in 239.0.0.1:5000 --out 239.0.0.2:5001
//!
//! Reads orders from the `--in` multicast group, broadcasts trades to `--out`.

use matcher::{matcher as engine, net};
use std::net::{Ipv4Addr, SocketAddr};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::mpsc;

const CHANNEL_DEPTH: usize = 1024;

struct Args {
    in_addr: SocketAddr,
    out_addr: SocketAddr,
    iface: Ipv4Addr,
}

fn parse_args() -> Result<Args, String> {
    let mut in_addr = "239.0.0.1:5000".to_string();
    let mut out_addr = "239.0.0.2:5001".to_string();
    let mut iface = "0.0.0.0".to_string();
    let mut iter = std::env::args().skip(1);
    while let Some(flag) = iter.next() {
        match flag.as_str() {
            "--in" => in_addr = iter.next().ok_or("--in needs a value")?,
            "--out" => out_addr = iter.next().ok_or("--out needs a value")?,
            "--iface" => iface = iter.next().ok_or("--iface needs a value")?,
            "-h" | "--help" => {
                eprintln!("Usage: matcher [--in <ip:port>] [--out <ip:port>] [--iface <ipv4>]");
                std::process::exit(0);
            }
            other => return Err(format!("unknown flag: {other}")),
        }
    }
    Ok(Args {
        in_addr: in_addr.parse().map_err(|e| format!("bad --in: {e}"))?,
        out_addr: out_addr.parse().map_err(|e| format!("bad --out: {e}"))?,
        iface: iface.parse().map_err(|e| format!("bad --iface: {e}"))?,
    })
}

fn now_ns() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = parse_args().map_err(|e| {
        eprintln!("{e}");
        e
    })?;

    let recv_sock = net::bind_multicast(args.in_addr, args.iface)?;
    let send_sock = net::bind_sender(args.iface)?;
    eprintln!("matcher: in={} out={} iface={}", args.in_addr, args.out_addr, args.iface);

    let (in_tx, in_rx) = mpsc::channel(CHANNEL_DEPTH);
    let (trade_tx, trade_rx) = mpsc::channel(CHANNEL_DEPTH);

    let recv = tokio::spawn(net::receive(recv_sock, in_tx));
    let match_task = tokio::spawn(engine::run(in_rx, trade_tx, now_ns));
    let bcast = tokio::spawn(net::broadcast(send_sock, args.out_addr, trade_rx));

    tokio::select! {
        _ = recv => eprintln!("matcher: receiver exited"),
        _ = match_task => eprintln!("matcher: matcher exited"),
        _ = bcast => eprintln!("matcher: broadcaster exited"),
    }
    Ok(())
}
