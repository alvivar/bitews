use std::{
    collections::HashMap,
    io::{self, Read, Write},
    net::TcpListener,
    str::from_utf8,
    sync::{Arc, Mutex},
    thread,
};

use polling::{Event, Poller};
use tungstenite::accept;

mod conn;
use conn::Connection;

mod writer;
use writer::Writer;

fn main() -> io::Result<()> {
    println!("\nbit:e\n");

    // The server and the smol Poller.
    let server = TcpListener::bind("0.0.0.0:1984")?;
    server.set_nonblocking(true)?;

    let poller = Poller::new()?;
    poller.add(&server, Event::readable(0))?;
    let poller = Arc::new(poller);

    let mut readers = HashMap::<usize, Connection>::new();
    let writers = HashMap::<usize, Connection>::new();
    let writers = Arc::new(Mutex::new(writers));

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
                    read_socket.set_nonblocking(true)?;
                    let write_socket = read_socket.try_clone().unwrap();

                    // let ws_socket = read_socket.try_clone().unwrap();
                    // let ws = accept(ws_socket).unwrap();

                    println!("Connection #{} from {}", id, addr);

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

                    // The server continues listening for more clients, always 0.
                    poller.modify(&server, Event::readable(0))?;
                }

                id if ev.readable => {
                    if let Some(conn) = readers.get_mut(&id) {
                        handle_reading(conn);

                        poller.modify(&conn.socket, Event::readable(id))?;

                        // One at the time.
                        if !conn.received.is_empty() {
                            let received = conn.received.remove(0);

                            // Instructions should be string.
                            if let Ok(utf8) = from_utf8(&received) {
                                println!("{}", utf8);
                            }
                        }

                        // Forget it, it died.
                        if conn.closed {
                            poller.delete(&conn.socket)?;

                            let conn = readers.remove(&id).unwrap();

                            writers.lock().unwrap().remove(&id);
                        }
                    }
                }

                id if ev.writable => {
                    let mut writers = writers.lock().unwrap();

                    if let Some(conn) = writers.get_mut(&id) {
                        handle_writing(conn);

                        // We need to send more.
                        if !conn.to_write.is_empty() {
                            poller
                                .modify(&conn.socket, Event::writable(conn.id))
                                .unwrap();
                        }

                        // Forget it, it died.
                        if conn.closed {
                            poller.delete(&conn.socket)?;

                            let conn = readers.remove(&id).unwrap();

                            writers.remove(&id);
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
    let data = match read(conn) {
        Ok(data) => data,
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

    if let Err(err) = conn.socket.write(&data) {
        println!("Connection #{} broken, write failed: {}", conn.id, err);
        conn.closed = true;
    }
}

fn read(conn: &mut Connection) -> io::Result<Vec<u8>> {
    let mut received = vec![0; 1024 * 4];
    let mut bytes_read = 0;

    loop {
        match conn.socket.read(&mut received[bytes_read..]) {
            Ok(0) => {
                // Reading 0 bytes means the other side has closed the
                // connection or is done writing, then so are we.
                return Err(io::Error::new(io::ErrorKind::BrokenPipe, "0 bytes read"));
            }
            Ok(n) => {
                bytes_read += n;
                if bytes_read == received.len() {
                    received.resize(received.len() + 1024, 0);
                }
            }
            // Would block "errors" are the OS's way of saying that the
            // connection is not actually ready to perform this I/O operation.
            // @todo Wondering if this should be a panic instead.
            Err(ref err) if would_block(err) => break,
            Err(ref err) if interrupted(err) => continue,
            // Other errors we'll consider fatal.
            Err(err) => return Err(err),
        }
    }

    // let received_data = &received_data[..bytes_read];
    // @doubt Using this slice thing and returning with into() versus using the
    // resize? Hm.

    received.resize(bytes_read, 0);

    Ok(received)
}

fn would_block(err: &io::Error) -> bool {
    err.kind() == io::ErrorKind::WouldBlock
}

fn interrupted(err: &io::Error) -> bool {
    err.kind() == io::ErrorKind::Interrupted
}
