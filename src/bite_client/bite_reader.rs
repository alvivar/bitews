use super::message::{Message, Messages};
use super::{connection::Connection, message::Received};

use std::net::{SocketAddr, TcpStream};

pub struct BiteReader {
    connection: Connection,
    messages: Messages,
}

impl BiteReader {
    pub fn new(id: usize, socket: TcpStream, addr: SocketAddr) -> BiteReader {
        let connection = Connection::new(id, socket, addr);
        let messages = Messages::new();

        BiteReader {
            connection,
            messages,
        }
    }

    pub fn handle_read(&mut self) {
        loop {
            let data = match self.connection.try_read() {
                Ok(received) => received,

                Err(err) => {
                    // connection.closed = true;
                    // ^ This is already hapenning inside try_read() on errors.

                    println!("Error reading from socket: {}", err);

                    break;
                }
            };

            let received = match self.messages.feed(data) {
                Received::None => continue,

                Received::Complete(received) => received,

                Received::Pending(received) => received,

                Received::Error(err) => {
                    self.connection.closed = true;

                    println!("Error feeding from socket: {}", err);

                    break;
                }
            };

            // Return the received message to the async thing.
        }
    }
}
