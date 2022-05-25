use crossbeam_channel::{unbounded, Receiver, Sender};
use polling::{Event, Poller};
use tungstenite::WebSocket;

use std::collections::HashMap;
use std::net::{SocketAddr, TcpStream};
use std::str::from_utf8;
use std::sync::Arc;

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

pub struct Connections {
    connections: HashMap<usize, Connection>,
    poller: Arc<Poller>,
    pub tx: Sender<Cmd>,
    rx: Receiver<Cmd>,
}

pub enum Cmd {
    New(usize, WebSocket<TcpStream>, SocketAddr),
}

impl Connections {
    pub fn new(poller: Arc<Poller>) -> Self {
        let all = HashMap::<usize, Connection>::new();
        let (tx, rx) = unbounded::<Cmd>();

        Connections {
            connections: all,
            poller,
            tx,
            rx,
        }
    }

    pub fn handle(&mut self) {
        match self.rx.recv().unwrap() {
            Cmd::New(id, ws, addr) => {
                self.poller.add(ws.get_ref(), Event::readable(id)).unwrap();
                let conn = Connection::new(id, id, ws, addr);
                self.connections.insert(id, conn);
                println!("WebSocket #{} from {} ready to poll", id, addr);
            }
        }
    }
}
