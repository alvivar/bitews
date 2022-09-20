use std::net::{SocketAddr, TcpStream};

use crossbeam_channel::{unbounded, Receiver, Sender};

use super::connection::Connection;

enum Command {
    Write(Vec<u8>),
}

pub struct BiteWriter {
    connection: Connection,
    tx: Sender<Command>,
    rx: Receiver<Command>,
}

impl BiteWriter {
    pub fn new(id: usize, socket: TcpStream, addr: SocketAddr) -> BiteWriter {
        let connection = Connection::new(id, socket, addr);
        let (tx, rx) = unbounded::<Command>();

        BiteWriter { connection, tx, rx }
    }

    pub fn handle(&mut self) {
        loop {
            match self.rx.recv().unwrap() {
                Command::Write(data) => {
                    self.connection.try_write(data);
                }
            }
        }
    }
}
