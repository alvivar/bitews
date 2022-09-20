use std::{
    collections::HashMap,
    net::{SocketAddr, TcpStream},
};

use axum::headers::Connection;
use polling::{Event, Poller};

enum Command {
    Connect(SocketAddr),
    Disconnect(SocketAddr),
    Send(SocketAddr, Vec<u8>),
}

pub struct Bite {
    poller: Poller,
    readers: HashMap<usize, Connection>,
    writers: HashMap<usize, Connection>,
}

impl Bite {
    pub fn new(id: usize, addr: SocketAddr) -> Bite {
        let poller = Poller::new().unwrap();
        let readers = HashMap::<usize, Connection>::new();
        let writers = HashMap::<usize, Connection>::new();

        // let socket_reader = TcpStream::connect(addr).unwrap();
        // socket_reader.set_nonblocking(true).unwrap();
        // let socket_writer = socket_reader.try_clone().unwrap();

        Bite {
            poller,
            readers,
            writers,
        }
    }

    pub fn handle(&mut self) {
        // Connections and events via smol Poller.
        let mut id_count: usize = 1; // 0 belongs to the main TcpListener.
        let mut events = Vec::new();

        loop {
            events.clear();
            self.poller.wait(&mut events, None)?;

            for ev in &events {
                match ev.key {
                    0 => {}

                    id if ev.readable => {}

                    id if ev.writable => {}

                    _ => unreachable!(),
                }
            }
        }
    }
}
