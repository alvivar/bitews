use std::net::{SocketAddr, TcpStream};

use tungstenite::WebSocket;

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
}
