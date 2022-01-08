use std::{
    collections::HashMap,
    io::{
        self,
        ErrorKind::{BrokenPipe, Interrupted, WouldBlock},
        Read, Write,
    },
    net::{TcpListener, TcpStream},
    str::from_utf8,
    sync::Arc,
};

use polling::{Event, Poller};
use tungstenite::{self};

mod conn;
use conn::Connection;

mod bite;
use bite::Bite;

fn main() -> io::Result<()> {
    println!("\nbite Proxy\n");

    // The server and the smol poller
    let server = TcpListener::bind("0.0.0.0:1983")?;
    server.set_nonblocking(true)?;

    let poller = Poller::new()?;
    poller.add(&server, Event::readable(0))?;
    let poller = Arc::new(poller);

    // The connections
    let mut connections = HashMap::<usize, Connection>::new();
    let mut bites = HashMap::<usize, Bite>::new();

    // Connections and events via smol Poller.
    let mut id: usize = 1;
    let mut events = Vec::new();

    loop {
        events.clear();
        poller.wait(&mut events, None)?;

        for event in &events {
            println!(
                "+\nPolling #{} (Reading = {}, Writing = {})",
                event.key, event.readable, event.writable
            );

            match event.key {
                0 => {
                    let (socket, addr) = server.accept()?;
                    socket.set_nonblocking(true)?;
                    poller.modify(&server, Event::readable(0))?;

                    // Try as websocket, creating a Bite connection for it.
                    match tungstenite::accept(socket) {
                        Ok(ws) => {
                            let conn_id = id;
                            id += 1;

                            let bite_id = id;
                            id += 1;

                            poller.add(ws.get_ref(), Event::readable(conn_id))?;
                            let conn = Connection::new(conn_id, bite_id, ws, addr);
                            connections.insert(conn_id, conn);
                            println!("WebSocket #{} from {} ready to poll", conn_id, addr);

                            let bite = Bite::new(bite_id, conn_id, "0.0.0.0:1984".into());
                            poller.add(&bite.socket, Event::readable(bite_id))?;
                            bites.insert(bite_id, bite);
                            println!("Bite #{} ready to poll", bite_id);
                        }

                        Err(err) => {
                            println!("WebSocket #{} broken: {}", id, err);
                            continue;
                        }
                    }
                }

                id if event.readable => {
                    // WebSocket Read

                    if let Some(conn) = connections.get_mut(&id) {
                        try_read(conn);

                        if !conn.received.is_empty() {
                            let received = conn.received.remove(0);

                            if let Ok(utf8) = from_utf8(&received) {
                                if !utf8.is_empty() {
                                    println!("WebSocket #{} from {}: {}", conn.id, conn.addr, utf8);
                                }

                                // WebSocket -> Bite
                                if let Some(bite) = bites.get_mut(&conn.belong_id) {
                                    bite.to_write.push(utf8.into());
                                    poller.modify(&bite.socket, Event::writable(bite.id))?;
                                }

                                // Echo
                                // conn.to_write.push(utf8.into());
                                // poller.modify(conn.socket.get_ref(), Event::writable(id))?
                            } else {
                                poller.modify(conn.socket.get_ref(), Event::readable(id))?;
                            }
                        } else {
                            poller.modify(conn.socket.get_ref(), Event::readable(id))?;
                        }

                        if conn.closed {
                            let bite = bites.remove(&conn.belong_id).unwrap();
                            poller.delete(&bite.socket)?;

                            poller.delete(conn.socket.get_ref())?;
                            connections.remove(&id).unwrap();
                        }
                    }

                    // Bite Read

                    if let Some(bite) = bites.get_mut(&id) {
                        read_bite(bite);

                        poller.modify(&bite.socket, Event::readable(id))?;

                        if !bite.received.is_empty() {
                            let received = bite.received.remove(0);

                            if let Ok(utf8) = from_utf8(&received) {
                                if !utf8.is_empty() {
                                    println!("Bite #{}: {}", bite.id, utf8);
                                }

                                // From bite to the websocket
                                if let Some(cnn) = connections.get_mut(&bite.belong_id) {
                                    cnn.to_write.push(utf8.into());
                                    poller.modify(cnn.socket.get_ref(), Event::writable(cnn.id))?;
                                }
                            }
                        }

                        if bite.closed {
                            let conn = connections.remove(&bite.belong_id).unwrap();
                            poller.delete(conn.socket.get_ref())?;

                            poller.delete(&bite.socket)?;
                            bites.remove(&id).unwrap();
                        }
                    }
                }

                id if event.writable => {
                    // WebSocket Write

                    if let Some(conn) = connections.get_mut(&id) {
                        println!("Writing to WebSocket #{}: {:?}", conn.id, conn.to_write);
                        try_write(conn);

                        if !conn.to_write.is_empty() {
                            poller.modify(conn.socket.get_ref(), Event::writable(id))?;
                        } else {
                            poller.modify(conn.socket.get_ref(), Event::readable(id))?;
                        }

                        if conn.closed {
                            let bite = bites.remove(&conn.belong_id).unwrap();
                            poller.delete(&bite.socket)?;

                            poller.delete(conn.socket.get_ref())?;
                            connections.remove(&id).unwrap();
                        }
                    }

                    // Bite Write

                    if let Some(bite) = bites.get_mut(&id) {
                        println!("Writing to Bite #{}: {:?}", bite.id, bite.to_write);
                        write_bite(bite);

                        if !bite.to_write.is_empty() {
                            poller.modify(&bite.socket, Event::writable(id))?;
                        } else {
                            poller.modify(&bite.socket, Event::readable(id))?;
                        }

                        if bite.closed {
                            let ws = connections.remove(&bite.belong_id).unwrap();
                            poller.delete(ws.socket.get_ref())?;

                            poller.delete(&bite.socket)?;
                            bites.remove(&id).unwrap();
                        }
                    }
                }

                // Events that I don't care. @doubt Does it include Event::none?
                _ => (),
            }
        }
    }
}

fn try_read(conn: &mut Connection) {
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

fn try_write(conn: &mut Connection) {
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

fn read_bite(bite: &mut Bite) {
    let data = match read_socket(&mut bite.socket) {
        Ok(data) => data,
        Err(error) => {
            println!("Bite #{} broken, read failed: {}", bite.id, error);
            bite.closed = true;
            return;
        }
    };

    bite.received.push(data);
}

fn write_bite(bite: &mut Bite) {
    let data = bite.to_write.remove(0);

    if let Err(err) = bite.socket.write(&data) {
        println!("Bite #{} broken, write failed: {}", bite.id, err);
        bite.closed = true;
    }
}

fn read_socket(socket: &mut TcpStream) -> io::Result<Vec<u8>> {
    let mut received = vec![0; 1024 * 4];
    let mut bytes_read = 0;

    loop {
        match socket.read(&mut received[bytes_read..]) {
            Ok(0) => {
                // Reading 0 bytes means the other side has closed the
                // connection or is done writing, then so are we.
                return Err(io::Error::new(BrokenPipe, "0 bytes read"));
            }
            Ok(n) => {
                bytes_read += n;
                if bytes_read == received.len() {
                    received.resize(received.len() + 1024, 0);
                }
            }
            // Would block "errors" are the OS's way of saying that the
            // connection is not actually ready to perform this I/O operation.
            Err(ref err) if err.kind() == WouldBlock => break,
            Err(ref err) if err.kind() == Interrupted => continue,
            Err(err) => return Err(err),
        }
    }

    // let received_data = &received_data[..bytes_read];
    // @todo Using the slice and returning with into() versus using the resize?

    received.resize(bytes_read, 0);

    Ok(received)
}
