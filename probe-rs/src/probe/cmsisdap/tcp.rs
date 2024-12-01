use std::io::ErrorKind;
use std::net::SocketAddr;
use std::{
    cell::RefCell,
    io,
    io::{Read, Write},
    net::{TcpStream, ToSocketAddrs},
    time::Duration,
};

const TIMEOUT: Duration = Duration::from_millis(10000);
const ATTEMPTS: usize = 5;

pub struct DurableStream {
    address: SocketAddr,
    socket: RefCell<TcpStream>,
}

impl DurableStream {
    pub fn new(address: &impl ToSocketAddrs) -> Result<Self, io::Error> {
        let address = address.to_socket_addrs()?.next().expect("A valid address");
        let socket = TcpStream::connect_timeout(&address, TIMEOUT)?;
        return Ok(DurableStream {
            address,
            socket: RefCell::new(socket),
        });
    }

    fn with_reconnect(
        &self,
        mut func: impl FnMut() -> Result<usize, io::Error>,
    ) -> Result<usize, io::Error> {
        for attempt in 1..=ATTEMPTS {
            tracing::info!("Attempt {}/{}", attempt, ATTEMPTS);
            match func() {
                Ok(count) => return Ok(count),
                Err(error) => {
                    tracing::info!(
                        "Failed to read/write from socket due to error: {:?}",
                        error
                    );
                    if is_disconnect_error(&error) {
                        tracing::info!(
                            "Reconnect attempt ({}/{}) due to error: {:?}",
                            attempt,
                            ATTEMPTS,
                            error
                        );
                        if let Ok(socket) = TcpStream::connect_timeout(&self.address, TIMEOUT) {
                            *self.socket.borrow_mut() = socket;
                            tracing::info!("Reconnected to socket");
                        } else {
                            tracing::info!("Failed to reconnect to socket {}", error);
                        }
                    } else {
                        return Err(error);
                    }
                }
            }
        }
        Err(io::Error::new(
            ErrorKind::TimedOut,
            format!("Failed to reconnect after {} attempts", ATTEMPTS),
        ))
    }

    pub fn read(&self, buf: &mut [u8]) -> Result<usize, io::Error> {
        self.with_reconnect(|| {
            let mut socket = self.socket.borrow_mut();
            socket
                .set_read_timeout(Some(TIMEOUT))
                .expect("Non-zero read timeout");
            socket.read(buf)
        })
    }

    pub fn write(&self, buf: &[u8]) -> Result<usize, io::Error> {
        self.with_reconnect(|| {
            let mut socket = self.socket.borrow_mut();
            socket
                .set_write_timeout(Some(TIMEOUT))
                .expect("Non-zero write timeout");
            socket.write(buf)
        })
    }

    pub fn drain(&self, buffer: &mut [u8]) {
        let mut socket = self.socket.borrow_mut();
        socket
            .set_read_timeout(Some(Duration::from_millis(1)))
            .expect("Non-zero read timeout");
        loop {
            match socket.read(buffer) {
                Ok(n) if n != 0 => continue,
                // TODO: Should this reconnect?
                _ => break,
            }
        }
    }
}

// The following is heavily inspired by -
// https://github.com/craftytrickster/stubborn-io/blob/bda25e38345f7bc2886877897ba70c2742867df1/src/tokio/io.rs#L27C5-L43C6

fn is_disconnect_error(err: &io::Error) -> bool {
    use ErrorKind::*;

    match err.kind() {
        NotFound | PermissionDenied | ConnectionRefused | ConnectionReset | ConnectionAborted
        | NotConnected | AddrInUse | AddrNotAvailable | BrokenPipe | AlreadyExists | WouldBlock => true,
        _ => false,
    }
}
