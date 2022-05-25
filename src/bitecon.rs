use std::io::ErrorKind::{BrokenPipe, Interrupted, WouldBlock, WriteZero};
use std::io::{self, Read, Write};
use std::net::TcpStream;

pub struct Bite {
    pub id: usize,
    pub belong_id: usize,
    pub socket: TcpStream,
    pub received: Vec<Vec<u8>>,
    pub to_write: Vec<Vec<u8>>,
    pub closed: bool,
}

impl Bite {
    pub fn new(id: usize, belong_id: usize, ip: &str) -> Option<Bite> {
        let socket = match TcpStream::connect(ip) {
            Ok(stream) => {
                stream.set_nonblocking(true).unwrap();
                stream
            }
            Err(_) => return None,
        };

        let received = Vec::<Vec<u8>>::new();
        let to_write = Vec::<Vec<u8>>::new();

        Some(Bite {
            id,
            belong_id,
            socket,
            received,
            to_write,
            closed: false,
        })
    }

    pub fn try_read(&mut self) {
        let data = match read(&mut self.socket) {
            Ok(data) => data,
            Err(error) => {
                println!("Bite #{} closed, read failed: {}", self.id, error);
                self.closed = true;
                return;
            }
        };

        self.received.push(data);
    }

    pub fn try_write(&mut self) {
        let data = self.to_write.remove(0);

        if let Err(err) = write(&mut self.socket, data) {
            println!("Bite #{} closed, write failed: {}", self.id, err);
            self.closed = true;
        }
    }
}

fn read(socket: &mut TcpStream) -> io::Result<Vec<u8>> {
    let mut received = vec![0; 4096]; // @todo What could be the correct size for this?
    let mut bytes_read = 0;

    loop {
        match socket.read(&mut received[bytes_read..]) {
            Ok(0) => {
                // Reading 0 bytes means the other side has closed the
                // connection or is done writing, then so are we.
                return Err(BrokenPipe.into());
            }

            Ok(n) => {
                bytes_read += n;
                if bytes_read == received.len() {
                    received.resize(received.len() + 1024, 0);
                }
            }

            // Would block "errors" are the OS's way of saying that the
            // connection is not actually ready to perform this I/O operation.
            Err(ref err) if err.kind() == WouldBlock => break,

            Err(ref err) if err.kind() == Interrupted => continue,

            // Other errors we'll consider fatal.
            Err(err) => return Err(err),
        }
    }

    received.resize(bytes_read, 0);

    Ok(received)
}

fn write(socket: &mut TcpStream, data: Vec<u8>) -> io::Result<usize> {
    match socket.write(&data) {
        // We want to write the entire `DATA` buffer in a single go. If we
        // write less we'll return a short write error (same as
        // `io::Write::write_all` does).
        Ok(n) if n < data.len() => Err(WriteZero.into()),

        Ok(n) => {
            // After we've written something we'll reregister the connection to
            // only respond to readable events.
            Ok(n)
        }

        // Would block "errors" are the OS's way of saying that the connection
        // is not actually ready to perform this I/O operation.
        Err(ref err) if err.kind() == WouldBlock => Err(WouldBlock.into()),

        // Got interrupted (how rude!), we'll try again.
        Err(ref err) if err.kind() == Interrupted => write(socket, data),

        // Other errors we'll consider fatal.
        Err(err) => Err(err),
    }
}
