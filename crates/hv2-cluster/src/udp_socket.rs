//! UDP sockets that carry a full-size datagram on every platform.
//!
//! The native ports forward datagrams up to 65,507 bytes, the IPv4 maximum.
//! Linux and Windows send that from any socket. macOS does not: a UDP
//! socket's largest datagram is its send buffer, 9,216 bytes by default
//! (`net.inet.udp.maxdgram`), and anything larger fails with `EMSGSIZE`.
//! Every UDP socket here is bound through [`bind`], which raises the buffer
//! where that limit exists, so the same datagram goes out on every host.

use std::io;

use tokio::net::{ToSocketAddrs, UdpSocket};

/// Room for the largest datagram, with headroom for the stack's own use.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
const FULL_DATAGRAM_BUFFER: usize = 256 * 1024;

/// Bind a UDP socket able to send and receive a full-size datagram.
pub async fn bind(address: impl ToSocketAddrs) -> io::Result<UdpSocket> {
    let socket = UdpSocket::bind(address).await?;
    allow_full_datagrams(&socket)?;
    Ok(socket)
}

/// Raise `socket`'s buffers where the platform otherwise caps a datagram
/// below the protocol maximum. Leaves larger buffers alone.
pub fn allow_full_datagrams(socket: &UdpSocket) -> io::Result<()> {
    #[cfg(target_os = "macos")]
    {
        let raw = socket2::SockRef::from(socket);
        if raw.send_buffer_size()? < FULL_DATAGRAM_BUFFER {
            raw.set_send_buffer_size(FULL_DATAGRAM_BUFFER)?;
        }
        if raw.recv_buffer_size()? < FULL_DATAGRAM_BUFFER {
            raw.set_recv_buffer_size(FULL_DATAGRAM_BUFFER)?;
        }
    }
    #[cfg(not(target_os = "macos"))]
    let _ = socket;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The largest IPv4 datagram crosses loopback through sockets bound here.
    #[tokio::test]
    async fn a_full_size_datagram_round_trips() {
        let receiver = bind("127.0.0.1:0").await.unwrap();
        let sender = bind("127.0.0.1:0").await.unwrap();
        let payload = vec![7u8; 65_507];
        sender
            .send_to(&payload, receiver.local_addr().unwrap())
            .await
            .unwrap();
        let mut buffer = vec![0u8; 65_535];
        let (n, _) = receiver.recv_from(&mut buffer).await.unwrap();
        assert_eq!(&buffer[..n], &payload[..]);
    }
}
