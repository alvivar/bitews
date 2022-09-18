use super::connection::Connection;
use super::message::Messages;

use std::net::{SocketAddr, TcpStream};

pub struct Bitec {
    connection: Connection,
    messages: Messages,
}

impl Bitec {
    pub fn new(id: usize, addr: SocketAddr) -> Bitec {
        let socket = TcpStream::connect(addr).unwrap();
        socket.set_nonblocking(true).unwrap();

        let connection = Connection::new(id, socket, addr);
        let messages = Messages::new();

        Bitec {
            connection,
            messages,
        }
    }

    fn send() {
        // @todo
    }
}
