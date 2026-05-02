//! UDP multicast receiver and broadcaster — two tiny async loops.

use crate::codec::{decode_inbound, encode_trade, Inbound, PACKET_SIZE};
use crate::types::Trade;
use socket2::{Domain, Protocol, Socket, Type};
use std::io;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use tokio::net::UdpSocket;
use tokio::sync::mpsc;

/// Bind a socket joined to the given multicast group, optionally on a
/// specific interface IP (use `Ipv4Addr::UNSPECIFIED` to let the kernel
/// pick).
pub fn bind_multicast(group: SocketAddr, iface: Ipv4Addr) -> io::Result<UdpSocket> {
    let ip = match group.ip() {
        IpAddr::V4(v4) => v4,
        IpAddr::V6(_) => {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "IPv6 multicast not supported",
            ));
        }
    };
    let sock = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;
    sock.set_reuse_address(true)?;
    sock.set_nonblocking(true)?;
    sock.bind(&SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), group.port()).into())?;
    sock.join_multicast_v4(&ip, &iface)?;
    UdpSocket::from_std(sock.into())
}

/// Bind a sender socket whose outbound multicast traffic egresses on `iface`.
pub fn bind_sender(iface: Ipv4Addr) -> io::Result<UdpSocket> {
    let sock = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;
    sock.set_nonblocking(true)?;
    sock.bind(&SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 0).into())?;
    sock.set_multicast_ttl_v4(1)?;
    sock.set_multicast_loop_v4(true)?;
    if iface != Ipv4Addr::UNSPECIFIED {
        sock.set_multicast_if_v4(&iface)?;
    }
    UdpSocket::from_std(sock.into())
}

/// Receive packets from `socket`, decode, push to `tx`.
/// Malformed packets are silently dropped.
pub async fn receive(socket: UdpSocket, tx: mpsc::Sender<Inbound>) {
    let mut buf = [0u8; PACKET_SIZE];
    loop {
        match socket.recv_from(&mut buf).await {
            Ok((PACKET_SIZE, _)) => {
                if let Ok(msg) = decode_inbound(&buf) {
                    if tx.send(msg).await.is_err() {
                        return;
                    }
                }
            }
            Ok(_) => {} // wrong-size packet, ignore
            Err(_) => return,
        }
    }
}

/// Pull trades from `rx`, encode, send to `dest`.
pub async fn broadcast(socket: UdpSocket, dest: SocketAddr, mut rx: mpsc::Receiver<Trade>) {
    while let Some(t) = rx.recv().await {
        let buf = encode_trade(&t);
        let _ = socket.send_to(&buf, dest).await;
    }
}
