use std::{
    collections::HashMap,
    io::{
        self,
        ErrorKind::{BrokenPipe, Interrupted, WouldBlock},
        Read, Write,
    },
    net::TcpStream,
    str::from_utf8,
    sync::mpsc::{channel, Receiver, Sender},
};

use polling::{Event, Poller};

pub struct Bite {
    id: usize,
    socket: TcpStream,
    tx: Sender<String>,
    rx: Receiver<String>,
    forward: Sender<Response>,
    received: Vec<Vec<u8>>,
    to_write: Vec<Vec<u8>>,
    closed: bool,
}

impl Bite {
    pub fn new(id: usize, ip: &str, forward: Sender<Response>) -> Bite {
        let socket = TcpStream::connect(ip).unwrap(); // "127.0.0.1:1984"
        let (tx, rx) = channel::<String>();
        let received = Vec::<Vec<u8>>::new();
        let to_write = Vec::<Vec<u8>>::new();

        Bite {
            id,
            socket,
            tx,
            rx,
            forward,
            received,
            to_write,
            closed: false,
        }
    }
}

pub enum Command {
    Insert(usize, String, Sender<Response>),
    Write(usize, String),
}

pub enum Response {
    Write(usize, String),
}

pub struct Cluster {
    count: usize,
    poller: Poller,
    bites: HashMap<usize, Bite>,
    pub tx: Sender<Command>,
    rx: Receiver<Command>,
}

impl Cluster {
    pub fn new() -> Self {
        let count = 0;
        let poller = Poller::new().unwrap();
        let bites = HashMap::<usize, Bite>::new();
        let (tx, rx) = channel::<Command>();

        Self {
            count,
            poller,
            bites,
            tx,
            rx,
        }
    }

    pub fn get_tx(&mut self, id: usize) -> Option<Sender<String>> {
        if let Some(bite) = self.bites.get(&id) {
            Some(bite.tx.clone())
        } else {
            None
        }
    }

    pub fn handle(&mut self) {
        let mut events = Vec::new();

        loop {
            // Commands

            match self.rx.try_recv() {
                Ok(Command::Insert(id, ip, forward)) => {
                    self.bites.insert(id, Bite::new(id, &ip, forward));
                    self.count += 1;
                }

                Ok(Command::Write(id, value)) => {
                    if let Some(bite) = self.bites.get_mut(&id) {
                        bite.to_write.push(value.into());

                        self.poller
                            .modify(&bite.socket, Event::writable(id))
                            .unwrap();
                    }
                }

                Err(_) => panic!("Panic: Cluster command failed"),
            }

            // Polling

            events.clear();
            self.poller.wait(&mut events, None).unwrap();

            for event in &events {
                match event.key {
                    id if event.readable => {
                        if let Some(bite) = self.bites.get_mut(&id) {
                            try_read(bite);

                            if !bite.received.is_empty() {
                                let received = bite.received.remove(0);

                                if let Ok(utf8) = from_utf8(&received) {
                                    if !utf8.is_empty() {
                                        println!("{}", utf8);
                                    }

                                    bite.to_write.push(utf8.into());

                                    self.poller
                                        .modify(&bite.socket, Event::writable(id))
                                        .unwrap();
                                }
                            } else {
                                self.poller
                                    .modify(&bite.socket, Event::readable(id))
                                    .unwrap();
                            }

                            if bite.closed {
                                self.poller.delete(&bite.socket).unwrap();
                                self.bites.remove(&id).unwrap();
                            }
                        }
                    }

                    id if event.writable => {
                        if let Some(bite) = self.bites.get_mut(&id) {
                            try_write(bite);

                            if !bite.to_write.is_empty() {
                                self.poller
                                    .modify(&bite.socket, Event::writable(id))
                                    .unwrap();
                            }

                            if bite.closed {
                                self.poller.delete(&bite.socket).unwrap();
                                self.bites.remove(&id).unwrap();
                            }
                        }
                    }

                    _ => (),
                }
            }
        }
    }
}

fn try_read(bite: &mut Bite) {
    let data = match read(bite) {
        Ok(data) => data,
        Err(error) => {
            println!("Bite #{} broken, read failed: {}", bite.id, error);
            bite.closed = true;
            return;
        }
    };

    bite.received.push(data);
}

fn try_write(bite: &mut Bite) {
    let data = bite.to_write.remove(0);

    if let Err(err) = bite.socket.write(&data) {
        println!("Bite #{} broken, write failed: {}", bite.id, err);
        bite.closed = true;
    }
}

fn read(bite: &mut Bite) -> io::Result<Vec<u8>> {
    let mut received = vec![0; 1024 * 4];
    let mut bytes_read = 0;

    loop {
        match bite.socket.read(&mut received[bytes_read..]) {
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
