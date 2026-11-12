use std::io::{self, Read, Write};
use std::net::TcpStream;
use std::time::{Duration, Instant};

pub fn remaining(deadline: Instant) -> Option<Duration> {
    deadline
        .checked_duration_since(Instant::now())
        .filter(|d| !d.is_zero())
}

pub struct DeadlineStream {
    tcp: TcpStream,
    deadline: Instant,
    per_io: Duration,
}

impl DeadlineStream {
    pub fn new(tcp: TcpStream, deadline: Instant, per_io: Duration) -> Self {
        Self {
            tcp,
            deadline,
            per_io,
        }
    }

    fn budget(&self) -> io::Result<Duration> {
        remaining(self.deadline)
            .map(|d| d.min(self.per_io))
            .ok_or_else(|| io::Error::new(io::ErrorKind::TimedOut, "overall deadline exceeded"))
    }
}

impl Read for DeadlineStream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let budget = self.budget()?;
        self.tcp.set_read_timeout(Some(budget))?;
        self.tcp.read(buf)
    }
}

impl Write for DeadlineStream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let budget = self.budget()?;
        self.tcp.set_write_timeout(Some(budget))?;
        self.tcp.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.tcp.flush()
    }
}
