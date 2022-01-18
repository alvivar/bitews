use std::{
    collections::HashMap, env, io, net::TcpListener, process::exit, str::from_utf8, sync::Arc,
};

use polling::{Event, Poller};
use tungstenite;

mod conn;
use conn::Connection;

mod bite;
use bite::Bite;

fn main() -> io::Result<()> {
    println!("\nBIT:E WebSocket Proxy\n");

    let server = match env::var("SERVER") {
        Ok(v) => v,
        Err(_) => {
            println!("Environmental variable SERVER is missing!");
            println!("The URI where the server is gonna receive connections.");
            println!("BASH i.e: export SERVER=0.0.0.0:1983");
            exit(1);
        }
    };

    let proxy = match env::var("PROXY") {
        Ok(v) => v,
        Err(_) => {
            println!("Environmental variable PROXY is missing!");
            println!("That's the Bite server that we are gonna proxy.");
            println!("BASH i.e: export PROXY=0.0.0.0:1984");
            exit(1);
        }
    };

    // The server and the smol poller
    let server = TcpListener::bind(server.as_str())?;
    server.set_nonblocking(true)?;

    let poller = Poller::new()?;
    poller.add(&server, Event::readable(0))?;
    let poller = Arc::new(poller);

    // Connections
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

                    // Try as websocket, creating a Bite connection for it.
                    match tungstenite::accept(socket) {
                        Ok(ws) => {
                            let conn_id = id;
                            id += 1;

                            let bite_id = id;
                            id += 1;

                            // Bite is a requirement.
                            let bite = Bite::new(bite_id, conn_id, proxy.as_str());
                            match bite {
                                Some(bite) => {
                                    ws.get_ref().set_nonblocking(true)?;

                                    poller.add(ws.get_ref(), Event::readable(conn_id))?;
                                    let conn = Connection::new(conn_id, bite_id, ws, addr);
                                    connections.insert(conn_id, conn);
                                    println!("WebSocket #{} from {} ready to poll", conn_id, addr);

                                    poller.add(&bite.socket, Event::readable(bite_id))?;
                                    bites.insert(bite_id, bite);
                                    println!("Bite #{} ready to poll", bite_id);
                                }

                                None => {
                                    id -= 2; // Rolling back id calculation.
                                    println!("Ignoring, Bite server {} unavailable", proxy);
                                }
                            }
                        }

                        Err(err) => {
                            println!("WebSocket connection #{} broken: {}", id, err);
                        }
                    }

                    // Continue accepting connections.
                    poller.modify(&server, Event::readable(0))?;
                }

                id if event.readable => {
                    // WebSocket reading
                    if let Some(conn) = connections.get_mut(&id) {
                        conn.read();

                        if !conn.received.is_empty() {
                            let received = conn.received.remove(0);

                            if let Ok(utf8) = from_utf8(&received) {
                                println!("WebSocket #{} from {}: {}", conn.id, conn.addr, utf8);

                                // From WebSocket to Bite
                                if let Some(bite) = bites.get_mut(&conn.belong_id) {
                                    bite.to_write.push(utf8.into());
                                    poller.modify(&bite.socket, Event::writable(bite.id))?;
                                }

                                // WebSocket echo
                                // conn.to_write.push(utf8.into());
                                // poller.modify(conn.socket.get_ref(), Event::writable(id))?
                            }
                        }

                        if conn.closed {
                            let bite = bites.remove(&conn.belong_id).unwrap();
                            poller.delete(&bite.socket)?;
                            println!("Dropping Bite #{}", bite.id);

                            poller.delete(conn.socket.get_ref())?;
                            connections.remove(&id).unwrap();
                            println!("Dropping WebSocket #{}", id);
                        } else {
                            poller.modify(conn.socket.get_ref(), Event::readable(id))?;
                        }
                    }

                    // Bite reading
                    if let Some(bite) = bites.get_mut(&id) {
                        bite.read();

                        if !bite.received.is_empty() {
                            let received = bite.received.remove(0);

                            if let Ok(utf8) = from_utf8(&received) {
                                println!("Bite #{}: {}", bite.id, utf8);

                                // From Bite to the WebSocket
                                if let Some(cnn) = connections.get_mut(&bite.belong_id) {
                                    cnn.to_write.push(utf8.into());
                                    poller.modify(cnn.socket.get_ref(), Event::writable(cnn.id))?;
                                }
                            }
                        }

                        if bite.closed {
                            let conn = connections.remove(&bite.belong_id).unwrap();
                            poller.delete(conn.socket.get_ref())?;
                            println!("Dropping WebSocket #{}", conn.id);

                            poller.delete(&bite.socket)?;
                            bites.remove(&id).unwrap();
                            println!("Dropping Bite #{}", id);
                        } else {
                            poller.modify(&bite.socket, Event::readable(id))?;
                        }
                    }
                }

                id if event.writable => {
                    // WebSocket writing
                    if let Some(conn) = connections.get_mut(&id) {
                        println!("Writing to WebSocket #{}: {:?}", conn.id, conn.to_write);
                        conn.write();

                        if conn.closed {
                            let bite = bites.remove(&conn.belong_id).unwrap();
                            poller.delete(&bite.socket)?;
                            println!("Dropping Bite #{}", bite.id);

                            poller.delete(conn.socket.get_ref())?;
                            connections.remove(&id).unwrap();
                            println!("Dropping WebSocket #{}", id);

                            continue;
                        }

                        // How do we know if we really need how to write?
                        if !conn.to_write.is_empty() {
                            println!("WebSocket #{} to writable", id);
                            conn.socket.write_pending().unwrap();
                            poller.modify(conn.socket.get_ref(), Event::writable(id))?;
                        } else {
                            println!("WebSocket #{} to readable", id);
                            poller.modify(conn.socket.get_ref(), Event::readable(id))?;
                        }
                    }

                    // Bite writing
                    if let Some(bite) = bites.get_mut(&id) {
                        println!("Writing to Bite #{}: {:?}", bite.id, bite.to_write);
                        bite.write();

                        if bite.closed {
                            let conn = connections.remove(&bite.belong_id).unwrap();
                            poller.delete(conn.socket.get_ref())?;
                            println!("Dropping WebSocket #{}", conn.id);

                            poller.delete(&bite.socket)?;
                            bites.remove(&id).unwrap();
                            println!("Dropping Bite #{}", id);

                            continue;
                        }

                        if !bite.to_write.is_empty() {
                            println!("Bite #{} to writable", id);
                            poller.modify(&bite.socket, Event::writable(id))?;
                        } else {
                            println!("Bite #{} to writable", id);
                            poller.modify(&bite.socket, Event::readable(id))?;
                        }
                    }
                }

                _ => unreachable!(),
            }
        }
    }
}
