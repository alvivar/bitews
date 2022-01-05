use std::{
    net::TcpStream,
    sync::mpsc::{channel, Receiver, Sender},
};

use polling::Poller;

pub struct Bite {
    poller: Poller,
    read_stream: TcpStream,
    write_stream: TcpStream,
    tx: Sender<String>,
    rx: Receiver<String>,
}

impl Bite {
    pub fn new() -> Bite {
        let poller = Poller::new().unwrap();

        let read_stream = TcpStream::connect("127.0.0.1:1984").unwrap();
        let write_stream = read_stream.try_clone().unwrap();

        let (tx, rx) = channel::<String>();

        Bite {
            poller,
            read_stream,
            write_stream,
            tx,
            rx,
        }
    }
}
