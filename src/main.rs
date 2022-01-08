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
    println!("\nbit:e Proxy\n");

    // The server and the smol poller
    let server = TcpListener::bind("0.0.0.0:1983")?;
    server.set_nonblocking(true)?;

    let poller = Poller::new()?;
    poller.add(&server, Event::readable(0))?;
    let poller = Arc::new(poller);

    // The connections
    let mut websockets = HashMap::<usize, Connection>::new();
    let mut bites = HashMap::<usize, Bite>::new();

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
                    poller.modify(&server, Event::readable(0))?;

                    // Try as websocket, creating a Bite connection for it.
                    match tungstenite::accept(socket) {
                        Ok(ws) => {
                            println!("Connection #{} from {}", id, addr);

                            let ws_id = id;
                            id += 1;

                            let bite_id = id;
                            id += 1;

                            poller.add(ws.get_ref(), Event::readable(ws_id))?;
                            let conn = Connection::new(id, bite_id, ws, addr);
                            websockets.insert(id, conn);

                            let bite = Bite::new(id, ws_id, "0.0.0.0:1984".into());
                            poller.add(&bite.socket, Event::readable(bite_id))?;
                            bites.insert(id, bite);
                        }
                        Err(err) => {
                            println!("Connection #{} broken: {}", id, err);
                            continue;
                        }
                    }
                }

                id if event.readable => {
                    // Websockets Read

                    if let Some(conn) = websockets.get_mut(&id) {
                        try_read_websocket(conn);

                        if !conn.received.is_empty() {
                            let received = conn.received.remove(0);

                            if let Ok(utf8) = from_utf8(&received) {
                                if !utf8.is_empty() {
                                    println!("{}", utf8);
                                }

                                conn.to_write.push(utf8.into());
                                poller.modify(conn.socket.get_ref(), Event::writable(id))?
                            } else {
                                poller.modify(conn.socket.get_ref(), Event::readable(id))?;
                            }
                        } else {
                            poller.modify(conn.socket.get_ref(), Event::readable(id))?;
                        }

                        if conn.closed {
                            poller.delete(conn.socket.get_ref())?;
                            websockets.remove(&id).unwrap();
                        }
                    }

                    // Bites Read

                    if let Some(bite) = bites.get_mut(&id) {
                        try_read_bite(bite);

                        if !bite.received.is_empty() {
                            let received = bite.received.remove(0);

                            if let Ok(utf8) = from_utf8(&received) {
                                if !utf8.is_empty() {
                                    println!("{}", utf8);
                                }
                            }
                        } else {
                            poller.modify(&bite.socket, Event::readable(id)).unwrap();
                        }

                        if bite.closed {
                            poller.delete(&bite.socket).unwrap();
                            bites.remove(&id).unwrap();
                        }
                    }
                }

                id if event.writable => {
                    // Websockets Write

                    if let Some(conn) = websockets.get_mut(&id) {
                        try_write_websocket(conn);

                        if !conn.to_write.is_empty() {
                            poller.modify(conn.socket.get_ref(), Event::writable(id))?;
                        } else {
                            poller.modify(conn.socket.get_ref(), Event::readable(id))?;
                        }

                        if conn.closed {
                            poller.delete(conn.socket.get_ref())?;
                            websockets.remove(&id).unwrap();
                        }
                    }

                    // Bite Write

                    if let Some(bite) = bites.get_mut(&id) {
                        try_write_bite(bite);

                        if !bite.to_write.is_empty() {
                            poller.modify(&bite.socket, Event::writable(id))?;
                        } else {
                            poller.modify(&bite.socket, Event::readable(id))?;
                        }

                        if bite.closed {
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

fn try_read_websocket(conn: &mut Connection) {
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

fn try_write_websocket(conn: &mut Connection) {
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

fn try_read_bite(bite: &mut Bite) {
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

fn try_write_bite(bite: &mut Bite) {
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
