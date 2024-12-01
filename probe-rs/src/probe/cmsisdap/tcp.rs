use std::io::ErrorKind;
use std::net::SocketAddr;
use std::{
    cell::RefCell,
    io,
    io::{Read, Write},
    net::{TcpStream, ToSocketAddrs, Shutdown},
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
        operation: &str,
        mut func: impl FnMut() -> Result<usize, io::Error>,
        shortcircuit_wouldblock: bool,
    ) -> Result<usize, io::Error> {
        for attempt in 1..=ATTEMPTS {
            match func() {
                Ok(count) => return Ok(count),

                Err(error) => {
                    if error.kind() == ErrorKind::WouldBlock {
                        if shortcircuit_wouldblock {
                            return Ok(0);
                        }

                        if attempt < ATTEMPTS {
                            // don't reconnect - we're connected and there's just nothing
                            // in the tcp stream
                            tracing::warn!("{operation} timeout, waiting...");
                            continue;
                        }

                        // give up on this stream, fall through and reconnect

                    } else if !is_disconnect_error(&error) {
                        return Err(error)
                    }

                    tracing::warn!("{operation} on socket: {}", error.kind());

                    // in lieu of dropping the socket:
                    let sockref = self.socket.borrow();
                    if let Err(e) = sockref.shutdown(Shutdown::Both) {
                        if e.kind() != ErrorKind::NotConnected {
                            tracing::warn!("couldn't shutdown existing socket: {}", e.kind());
                        }
                    }
                    drop(sockref);

                    tracing::error!(
                        "reconnect attempt {}/{}...",
                        attempt,
                        ATTEMPTS,
                    );

                    match connected_socket(&self.address) {
                        Ok(socket) => {
                            *self.socket.borrow_mut() = socket;
                            tracing::info!("reconnected, retrying {operation}");
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
            format!("Failed to reconnect ({} attempts)", ATTEMPTS),
        ))
    }

    pub fn read(&self, buf: &mut [u8]) -> Result<usize, io::Error> {
        self.with_reconnect("read", || self.socket.borrow_mut().read(buf), false)
    }

    pub fn write(&self, buf: &[u8]) -> Result<usize, io::Error> {
        self.with_reconnect("write", || self.socket.borrow_mut().write(buf), false)
    }

    pub fn drain(&self, buf: &mut [u8]) {
        match self.with_reconnect("drain", || self.socket.borrow_mut().read(buf), true) {
            Ok(_count) => {}
            Err(e) if e.kind() == ErrorKind::WouldBlock => unreachable!(),
            Err(e) => {
                tracing::warn!("socket drain: {}", e.kind());
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
        | NotConnected | AddrInUse | AddrNotAvailable | BrokenPipe | AlreadyExists | InvalidInput => true,
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
