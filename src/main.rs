use std::{
    collections::HashMap,
    io::{self},
    net::{TcpListener, TcpStream},
    str::from_utf8,
    sync::{Arc, Mutex},
    thread,
};

use polling::{Event, Poller};
use tungstenite::{self, WebSocket};

mod conn;
use conn::Connection;

mod writer;
use writer::Writer;

fn main() -> io::Result<()> {
    println!("\nbit:e Proxy\n");

    // The server and the smol poller
    let server = TcpListener::bind("0.0.0.0:1984")?;
    server.set_nonblocking(true)?;

    let poller = Poller::new()?;
    poller.add(&server, Event::readable(0))?;
    let poller = Arc::new(poller);

    // The connections
    let mut readers = HashMap::<usize, Connection>::new();
    let writers = HashMap::<usize, Connection>::new();
    let writers = Arc::new(Mutex::new(writers));
    let mut websockets = HashMap::<usize, WebSocket<TcpStream>>::new();

    // The writer
    let writer = Writer::new(writers.clone(), poller.clone());
    let writer_tx = writer.tx.clone();
    thread::spawn(move || writer.handle());

    // Connections and events via smol Poller.
    let mut id: usize = 1;
    let mut events = Vec::new();

    loop {
        events.clear();
        poller.wait(&mut events, None)?;

        for ev in &events {
            match ev.key {
                0 => {
                    let (read_socket, addr) = server.accept()?;
                    // read_socket.set_nonblocking(true)?;
                    let write_socket = read_socket.try_clone().unwrap();
                    let ws_socket = read_socket.try_clone().unwrap();

                    // The server continues listening for more clients, always 0.
                    poller.modify(&server, Event::readable(0))?;

                    // Try as websocket.
                    match tungstenite::accept(ws_socket) {
                        Ok(ws) => {
                            websockets.insert(id, ws);
                            println!("Connection #{} from {}", id, addr);
                        }
                        Err(err) => {
                            println!("Connection #{} broken: {}", id, err);
                            continue;
                        }
                    }

                    // Register the reading socket for events.
                    poller.add(&read_socket, Event::readable(id))?;
                    readers.insert(id, Connection::new(id, read_socket, addr));

                    // Register the writing socket for events.
                    poller.add(&write_socket, Event::none(id))?;
                    writers
                        .lock()
                        .unwrap()
                        .insert(id, Connection::new(id, write_socket, addr));

                    // One more.
                    id += 1;
                }

                id if ev.readable => {
                    if let Some(conn) = readers.get_mut(&id) {
                        let ws = websockets.get_mut(&id).unwrap();
                        handle_reading(conn, ws);
                        poller.modify(&conn.socket, Event::readable(id))?;

                        // One at the time.
                        if !conn.received.is_empty() {
                            let received = conn.received.remove(0);

                            // Instructions should be string.
                            if let Ok(utf8) = from_utf8(&received) {
                                println!("{}", utf8);

                                writer_tx
                                    .send(writer::Cmd::Write(conn.id, utf8.to_owned()))
                                    .unwrap();
                            }
                        }

                        // Forget it, it died.
                        if conn.closed {
                            poller.delete(&conn.socket)?;
                            readers.remove(&id).unwrap();
                            writers.lock().unwrap().remove(&id);
                            websockets.remove(&id).unwrap();
                        }
                    }
                }

                id if ev.writable => {
                    let mut writers = writers.lock().unwrap();

                    if let Some(conn) = writers.get_mut(&id) {
                        let ws = websockets.get_mut(&id).unwrap();
                        handle_writing(conn, ws);

                        // We need to send more.
                        if !conn.to_write.is_empty() {
                            poller
                                .modify(&conn.socket, Event::writable(conn.id))
                                .unwrap();
                        }

                        // Forget it, it died.
                        if conn.closed {
                            poller.delete(&conn.socket)?;
                            readers.remove(&id).unwrap();
                            writers.remove(&id);
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

fn handle_reading(conn: &mut Connection, ws: &mut WebSocket<TcpStream>) {
    let data = match ws.read_message() {
        Ok(msg) => msg.into_data(),
        Err(err) => {
            println!("Connection #{} broken, read failed: {}", conn.id, err);
            conn.closed = true;
            return;
        }
    };

    conn.received.push(data);
}

fn handle_writing(conn: &mut Connection, ws: &mut WebSocket<TcpStream>) {
    let data = conn.to_write.remove(0);
    let data = from_utf8(&data).unwrap();

    // @todo Text or binary?

    if let Err(err) = ws.write_message(tungstenite::Message::Text(data.to_owned())) {
        println!("Connection #{} broken, write failed: {}", conn.id, err);
        conn.closed = true;
    }
}
