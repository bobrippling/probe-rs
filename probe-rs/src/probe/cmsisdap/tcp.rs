use std::io::ErrorKind;
use std::net::SocketAddr;
use std::{
    cell::RefCell,
    io,
    io::{Read, Write},
    net::{TcpStream, ToSocketAddrs},
    time::Duration,
};

const ATTEMPTS: usize = 5;

pub struct DurableStream {
    address: SocketAddr,
    socket: RefCell<TcpStream>,
}

impl DurableStream {
    pub fn new(address: &impl ToSocketAddrs) -> Result<Self, io::Error> {
        let address = address.to_socket_addrs()?.next().expect("A valid address");
        let socket = connected_socket(&address)?;

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
                    if !is_disconnect_error(&error) {
                        return Err(error);
                    }

                    tracing::info!(
                        "Reconnect attempt ({}/{}) due to error: {:?}",
                        attempt,
                        ATTEMPTS,
                        error
                    );
                    match connected_socket(&self.address) {
                        Ok(socket) => {
                            *self.socket.borrow_mut() = socket;
                            tracing::info!("reconnected, retrying");
                        }
                        Err(e) => {
                            tracing::error!("error reconnecting: {}", e.kind());
                        }
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
            socket.read(buf)
        })
    }

    pub fn write(&self, buf: &[u8]) -> Result<usize, io::Error> {
        self.with_reconnect(|| {
            let mut socket = self.socket.borrow_mut();
            socket.write(buf)
        })
    }

    pub fn drain(&self, buffer: &mut [u8]) {
        let mut socket = self.socket.borrow_mut();
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

fn connected_socket(address: &SocketAddr) -> Result<TcpStream, io::Error> {
    let connect_timeout = Duration::from_millis(10000);
    let rw_timeout = Duration::from_millis(5000);

    let socket = TcpStream::connect_timeout(&address, connect_timeout)?;

    socket.set_read_timeout(Some(rw_timeout)).expect("couldn't set read timeout");
    socket.set_write_timeout(Some(rw_timeout)).expect("couldn't set write timeout");

    Ok(socket)
}
