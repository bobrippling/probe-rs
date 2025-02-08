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
        let socket = connected_socket(&address).map_err(|e| {
            tracing::error!("connect: {e:?}");
            e
        })?;

        return Ok(DurableStream {
            address,
            socket: RefCell::new(socket),
        });
    }

    pub fn read(&self, buf: &mut [u8]) -> Result<usize, io::Error> {
        let mut sockref = self.socket.borrow_mut();
        match sockref.read(buf) {
            Ok(n) => Ok(n),
            Err(timeout_err) if timeout_err.kind() == ErrorKind::TimedOut => {
                // we timed out, maybe better checks here, but essentially:
                // - reconnect, then re-raise the error
                // - higher level code will retry the core.status() call (where a write needs reissuing)
                //   and because we've reconnected, it'll work
                if let Err(e) = sockref.shutdown(Shutdown::Both) {
                    if e.kind() != ErrorKind::NotConnected {
                        tracing::warn!("reconnect: couldn't shutdown existing socket: {}", e.kind());
                    }
                }

                for attempt in 0..ATTEMPTS {
                    match connected_socket(&self.address) {
                        Ok(socket) => {
                            *sockref = socket;
                            tracing::warn!("reconnect: success, established");
                            break
                        }
                        Err(e) => {
                            tracing::error!("reconnect ({attempt}/{ATTEMPTS}): {}", e.kind());
                            continue
                        }
                    }
                }

                tracing::error!("giving up reconnect");
                Err(timeout_err)
            }
            Err(e) => Err(e),
        }
    }

    pub fn write(&self, buf: &[u8]) -> Result<usize, io::Error> {
        self.socket.borrow_mut().write(buf)
    }

    pub fn drain(&self, buf: &mut [u8]) {
        match self.socket.borrow_mut().read(buf) {
            Ok(_n) => {}
            Err(e) if e.kind() == ErrorKind::WouldBlock => {}
            Err(e) => {
                tracing::error!("ignoring error during draing: {e:?}");
            }
        }
    }
}

fn connected_socket(address: &SocketAddr) -> Result<TcpStream, io::Error> {
    let connect_timeout = Duration::from_millis(30000);
    let rw_timeout = Duration::from_millis(2000);

    let socket = TcpStream::connect_timeout(&address, connect_timeout)?;

    socket.set_read_timeout(Some(rw_timeout)).expect("couldn't set read timeout");
    socket.set_write_timeout(Some(rw_timeout)).expect("couldn't set write timeout");

    Ok(socket)
}
