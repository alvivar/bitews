use std::{
    collections::HashMap,
    io::{self},
    net::TcpListener,
    str::from_utf8,
    sync::{mpsc::channel, Arc},
    thread::spawn,
};

use polling::{Event, Poller};
use tungstenite::{self};

mod conn;
use conn::Connection;

mod bites;
use bites::{Cluster, Command, Response};

fn main() -> io::Result<()> {
    println!("\nbit:e Proxy\n");

    // The server and the smol poller
    let server = TcpListener::bind("0.0.0.0:1983")?;
    server.set_nonblocking(true)?;

    let poller = Poller::new()?;
    poller.add(&server, Event::readable(0))?;
    let poller = Arc::new(poller);

    // The connections
    let mut websockets = HashMap::<usize, Connection>::new();

    // The channel
    let (tx, rx) = channel::<Response>();

    // The Bite cluster
    let mut cluster = Cluster::new();
    let cluster_tx = cluster.tx.clone();
    spawn(move || cluster.handle());

    // Connections and events via smol Poller.
    let mut id: usize = 1;
    let mut events = Vec::new();

    loop {
        // Commands

        match rx.try_recv() {
            Ok(Response::From(id, value)) => {
                if let Some(socket) = websockets.get_mut(&id) {
                    socket.to_write.push(value.into());
                    poller.modify(socket.socket.get_ref(), Event::writable(id))?
                }
            }

            Err(_) => (),
        }

        // Polling

        events.clear();
        poller.wait(&mut events, None)?;

        for event in &events {
            match event.key {
                0 => {
                    let (socket, addr) = server.accept()?;

                    poller.modify(&server, Event::readable(0))?;

                    // Try as websocket.
                    match tungstenite::accept(socket) {
                        Ok(ws) => {
                            poller.add(ws.get_ref(), Event::readable(id))?;
                            websockets.insert(id, Connection::new(id, ws, addr));

                            cluster_tx
                                .send(Command::Insert(id, "0.0.0.0:1984".into(), tx.clone()))
                                .unwrap();

                            println!("Connection #{} from {}", id, addr);
                        }
                        Err(err) => {
                            println!("Connection #{} broken: {}", id, err);
                            continue;
                        }
                    }

                    // Next!
                    id += 1;
                }

                id if event.readable => {
                    if let Some(conn) = websockets.get_mut(&id) {
                        handle_reading(conn);
                        poller.modify(conn.socket.get_ref(), Event::readable(id))?;

                        if !conn.received.is_empty() {
                            let received = conn.received.remove(0);

                            if let Ok(utf8) = from_utf8(&received) {
                                if !utf8.is_empty() {
                                    println!("{}", utf8);
                                }

                                cluster_tx.send(Command::Write(id, utf8.into())).unwrap();
                            }
                        }

                        if conn.closed {
                            poller.delete(conn.socket.get_ref())?;
                            websockets.remove(&id).unwrap();
                        }
                    }
                }

                id if event.writable => {
                    if let Some(conn) = websockets.get_mut(&id) {
                        handle_writing(conn);

                        if !conn.to_write.is_empty() {
                            poller.modify(conn.socket.get_ref(), Event::writable(conn.id))?;
                        }

                        if conn.closed {
                            poller.delete(conn.socket.get_ref())?;
                            websockets.remove(&id).unwrap();
                        }
                    }
                }

                // Events that I don't care. @doubt Does it include Event::none?
                _ => (),
            }
        }
    }
}

fn handle_reading(conn: &mut Connection) {
    let data = match conn.socket.read_message() {
        Ok(msg) => msg.into_data(),
        Err(err) => {
            println!("Connection #{} broken, read failed: {}", conn.id, err);
            conn.closed = true;
            return;
        }
    };

    conn.received.push(data);
}

fn handle_writing(conn: &mut Connection) {
    let data = conn.to_write.remove(0);
    let data = from_utf8(&data).unwrap();

    // @todo Text vs binary?

    if let Err(err) = conn
        .socket
        .write_message(tungstenite::Message::Text(data.to_owned()))
    {
        println!("Connection #{} broken, write failed: {}", conn.id, err);
        conn.closed = true;
    }
}
