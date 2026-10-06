//! Bounded datagram framing for an already authorized private UDP stream.
//! Callers own bounded packet queues and cancel this future on gateway teardown.
use super::Upstream;
use std::{io, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    sync::{mpsc, watch},
};

pub const MAX_PAYLOAD: usize = 65_507;
pub const QUEUE_DEPTH: usize = 2;

/// Relay whole datagrams. Any completed frame resets inactivity; partial frames
/// do not. All waits, including queue backpressure, remain subject to both the
/// inactivity limit and absolute lifetime. No Internet socket is opened here.
pub async fn relay(
    stream: Box<dyn Upstream>,
    mut from_guest: mpsc::Receiver<Vec<u8>>,
    to_guest: mpsc::Sender<Vec<u8>>,
    idle: Duration,
    lifetime: Duration,
) -> io::Result<()> {
    if idle.is_zero() || lifetime.is_zero() {
        return Err(io::ErrorKind::InvalidInput.into());
    }
    let (mut reader, mut writer) = tokio::io::split(stream);
    let (activity, mut observed) = watch::channel(tokio::time::Instant::now());
    let read_activity = activity.clone();
    let read = async {
        loop {
            let length = reader.read_u16().await? as usize;
            if length > MAX_PAYLOAD {
                return Err(io::ErrorKind::InvalidData.into());
            }
            let mut payload = vec![0; length];
            reader.read_exact(&mut payload).await?;
            read_activity.send_replace(tokio::time::Instant::now());
            if to_guest.send(payload).await.is_err() {
                return Ok(());
            }
        }
    };
    let write = async {
        while let Some(payload) = from_guest.recv().await {
            if payload.len() > MAX_PAYLOAD {
                return Err(io::ErrorKind::InvalidInput.into());
            }
            writer.write_u16(payload.len() as u16).await?;
            writer.write_all(&payload).await?;
            writer.flush().await?;
            activity.send_replace(tokio::time::Instant::now());
        }
        Ok(())
    };
    let inactivity = async {
        loop {
            let deadline = *observed.borrow_and_update() + idle;
            tokio::select! {
                _ = tokio::time::sleep_until(deadline) => {
                    if *observed.borrow() + idle <= tokio::time::Instant::now() { return; }
                }
                changed = observed.changed() => { if changed.is_err() { return; } }
            }
        }
    };
    tokio::select! {
        result = read => result,
        result = write => result,
        _ = to_guest.closed() => Ok(()),
        _ = inactivity => Err(io::ErrorKind::TimedOut.into()),
        _ = tokio::time::sleep(lifetime) => Err(io::ErrorKind::TimedOut.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    type Queues = (
        mpsc::Sender<Vec<u8>>,
        mpsc::Receiver<Vec<u8>>,
        mpsc::Sender<Vec<u8>>,
        mpsc::Receiver<Vec<u8>>,
    );
    fn queues() -> Queues {
        let (a, b) = mpsc::channel(QUEUE_DEPTH);
        let (c, d) = mpsc::channel(QUEUE_DEPTH);
        (a, b, c, d)
    }
    #[tokio::test]
    async fn exact_empty_binary_and_maximum_datagrams_preserve_boundaries() {
        let (client, mut server) = tokio::io::duplex(1024);
        let (send, input, output, mut receive) = queues();
        let task = tokio::spawn(relay(
            Box::new(client),
            input,
            output,
            Duration::from_secs(5),
            Duration::from_secs(10),
        ));
        let peer = tokio::spawn(async move {
            for _ in 0..3 {
                let size = server.read_u16().await.unwrap();
                let mut payload = vec![0; size as usize];
                server.read_exact(&mut payload).await.unwrap();
                server.write_u16(size).await.unwrap();
                server.write_all(&payload).await.unwrap();
            }
            // Keep the stream open until the relay explicitly releases it.
            assert!(server.read_u8().await.is_err());
        });
        for payload in [vec![], vec![0, 255, 13, 10], vec![42; MAX_PAYLOAD]] {
            send.send(payload.clone()).await.unwrap();
            assert_eq!(receive.recv().await.unwrap(), payload);
        }
        drop(send);
        task.await.unwrap().unwrap();
        peer.await.unwrap();
    }
    #[tokio::test]
    async fn oversized_and_truncated_frames_close_the_session() {
        for size in [MAX_PAYLOAD + 1, 4] {
            let (client, mut server) = tokio::io::duplex(32);
            let (_send, input, output, _receive) = queues();
            let task = tokio::spawn(relay(
                Box::new(client),
                input,
                output,
                Duration::from_secs(5),
                Duration::from_secs(10),
            ));
            server.write_u16(size as u16).await.unwrap();
            if size == 4 {
                server.write_all(&[1]).await.unwrap();
                server.shutdown().await.unwrap();
            }
            let error = task.await.unwrap().unwrap_err();
            assert_eq!(
                error.kind(),
                if size == 4 {
                    io::ErrorKind::UnexpectedEof
                } else {
                    io::ErrorKind::InvalidData
                }
            );
        }
        let (client, mut server) = tokio::io::duplex(32);
        let (send, input, output, _receive) = queues();
        let task = tokio::spawn(relay(
            Box::new(client),
            input,
            output,
            Duration::from_secs(5),
            Duration::from_secs(10),
        ));
        send.send(vec![0; MAX_PAYLOAD + 1]).await.unwrap();
        assert_eq!(
            task.await.unwrap().unwrap_err().kind(),
            io::ErrorKind::InvalidInput
        );
        assert!(server.read_u8().await.is_err());
    }
    #[tokio::test]
    async fn backpressure_is_bounded_and_idle_or_lifetime_closes() {
        for (idle, lifetime) in [
            (Duration::from_millis(50), Duration::from_secs(5)),
            (Duration::from_secs(5), Duration::from_millis(50)),
        ] {
            let (client, mut server) = tokio::io::duplex(32);
            let (_send, input, output, _receive) = queues();
            let task = tokio::spawn(relay(Box::new(client), input, output, idle, lifetime));
            assert_eq!(
                tokio::time::timeout(Duration::from_secs(2), task)
                    .await
                    .unwrap()
                    .unwrap()
                    .unwrap_err()
                    .kind(),
                io::ErrorKind::TimedOut
            );
            assert!(server.read_u8().await.is_err());
        }
        let (client, mut server) = tokio::io::duplex(32);
        let (_send, input, output, _receive) = queues();
        let task = tokio::spawn(relay(
            Box::new(client),
            input,
            output,
            Duration::from_millis(50),
            Duration::from_secs(5),
        ));
        for _ in 0..3 {
            server.write_all(&[0, 1, 42]).await.unwrap();
        }
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(2), task)
                .await
                .unwrap()
                .unwrap()
                .unwrap_err()
                .kind(),
            io::ErrorKind::TimedOut
        );
        assert!(server.read_u8().await.is_err());
    }
    #[tokio::test]
    async fn consumer_close_and_cancellation_release_transport() {
        for cancel in [false, true] {
            let (client, mut server) = tokio::io::duplex(32);
            let (_send, input, output, receive) = queues();
            let task = tokio::spawn(relay(
                Box::new(client),
                input,
                output,
                Duration::from_secs(5),
                Duration::from_secs(10),
            ));
            if cancel {
                task.abort();
                assert!(task.await.unwrap_err().is_cancelled());
            } else {
                drop(receive);
                task.await.unwrap().unwrap();
            }
            assert!(server.read_u8().await.is_err());
        }
    }
}
