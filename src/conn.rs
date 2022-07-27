use tungstenite::WebSocket;

use std::net::{SocketAddr, TcpStream};
use std::str::from_utf8;

pub struct Connection {
    pub id: usize,
    pub belong_id: usize,
    pub socket: WebSocket<TcpStream>,
    pub addr: SocketAddr,
    pub received: Vec<Vec<u8>>,
    pub to_write: Vec<Vec<u8>>,
    pub closed: bool,
}

impl Connection {
    pub fn new(
        id: usize,
        belong_id: usize,
        socket: WebSocket<TcpStream>,
        addr: SocketAddr,
    ) -> Connection {
        let received = Vec::<Vec<u8>>::new();
        let to_write = Vec::<Vec<u8>>::new();

        Connection {
            id,
            belong_id,
            socket,
            addr,
            received,
            to_write,
            closed: false,
        }
    }

    pub fn try_read(&mut self) {
        let data = match self.socket.read_message() {
            Ok(msg) => msg.into_data(),
            Err(err) => {
                println!("Connection #{} closed, read failed: {}", self.id, err);
                self.closed = true;
                return;
            }
        };

        self.received.push(data);
    }

    pub fn try_write(&mut self) {
        if self.to_write.is_empty() {
            self.socket.write_pending().unwrap();
            return;
        }

        let data = self.to_write.remove(0);
        let data = from_utf8(&data).unwrap();

        // @todo Text vs binary?

        if let Err(err) = self
            .socket
            .write_message(tungstenite::Message::Text(data.to_owned()))
        {
            println!("Connection #{} closed, write failed: {}", self.id, err);
            self.closed = true;
        }
    }
}
