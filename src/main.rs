use std::{
    collections::HashMap,
    io::{self},
    net::TcpListener,
    str::from_utf8,
    sync::Arc,
};

use polling::{Event, Poller};
use tungstenite::{self};

mod conn;
use conn::Connection;

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

    // Connections and events via smol Poller.
    let mut id: usize = 1;
    let mut events = Vec::new();

    loop {
        events.clear();
        poller.wait(&mut events, None)?;

        for event in &events {
            match event.key {
                0 => {
                    let (socket, addr) = server.accept()?;

                    // The server continues listening for more clients, always 0.
                    poller.modify(&server, Event::readable(0))?;

                    // Try as websocket.
                    match tungstenite::accept(socket) {
                        Ok(ws) => {
                            // Register the reading socket for events.
                            poller.add(ws.get_ref(), Event::readable(id))?;
                            websockets.insert(id, Connection::new(id, ws, addr));
                            println!("Connection #{} from {}", id, addr);
                        }
                        Err(err) => {
                            println!("Connection #{} broken: {}", id, err);
                            continue;
                        }
                    }

                    // One more.
                    id += 1;
                }

                id if event.readable => {
                    if let Some(conn) = websockets.get_mut(&id) {
                        handle_reading(conn);
                        poller.modify(conn.socket.get_ref(), Event::readable(id))?;

                        // One at the time.
                        if !conn.received.is_empty() {
                            let received = conn.received.remove(0);

                            // Instructions should be string.
                            if let Ok(utf8) = from_utf8(&received) {
                                if !utf8.is_empty() {
                                    println!("{}", utf8);
                                }

                                conn.to_write.push(utf8.into());

                                poller
                                    .modify(conn.socket.get_ref(), Event::writable(conn.id))
                                    .unwrap();
                            }
                        }

                        // Forget it, it died.
                        if conn.closed {
                            poller.delete(conn.socket.get_ref())?;
                            websockets.remove(&id).unwrap();
                        }
                    }
                }

                id if event.writable => {
                    if let Some(conn) = websockets.get_mut(&id) {
                        handle_writing(conn);

                        // We need to send more.
                        if !conn.to_write.is_empty() {
                            poller
                                .modify(conn.socket.get_ref(), Event::writable(conn.id))
                                .unwrap();
                        }

                        // Forget it, it died.
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

    // @todo Text or binary?

    if let Err(err) = conn
        .socket
        .write_message(tungstenite::Message::Text(data.to_owned()))
    {
        println!("Connection #{} broken, write failed: {}", conn.id, err);
        conn.closed = true;
    }
}
