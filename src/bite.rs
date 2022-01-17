use std::{
    io::{
        self,
        ErrorKind::{BrokenPipe, Interrupted, WouldBlock},
        Read, Write,
    },
    net::TcpStream,
};

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

    pub fn read(&mut self) {
        let data = match read_stream(&mut self.socket) {
            Ok(data) => data,
            Err(error) => {
                println!("Bite #{} closed, read failed: {}", self.id, error);
                self.closed = true;
                return;
            }
        };

        self.received.push(data);
    }

    pub fn write(&mut self) {
        let data = self.to_write.remove(0);

        if let Err(err) = self.socket.write(&data) {
            println!("Bite #{} closed, write failed: {}", self.id, err);
            self.closed = true;
        } else {
            self.socket.flush().unwrap();
        }
    }
}

fn read_stream(socket: &mut TcpStream) -> io::Result<Vec<u8>> {
    let mut received = vec![0; 1024 * 4];
    let mut bytes_read = 0;

    loop {
        match socket.read(&mut received[bytes_read..]) {
            Ok(0) => {
                // Reading 0 bytes means the other side has closed the
                // connection or is done writing, then so are we.
                return Err(io::Error::new(BrokenPipe, "0 bytes read"));
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
            Err(err) => return Err(err),
        }
    }

    // let received_data = &received_data[..bytes_read];
    // @todo Using the slice and returning with into() versus using the resize?

    received.resize(bytes_read, 0);

    Ok(received)
}
