use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::response::{Html, Response};
use axum::routing::get;
use axum::Router;
use futures::sink::SinkExt;
use futures::stream::StreamExt;
use tokio::net::TcpStream;
use tokio::sync::mpsc;

use std::env;
use std::include_str;
use std::io;
use std::net::{SocketAddr, ToSocketAddrs};
use std::sync::{Arc, Mutex};

#[macro_use]
extern crate log;
extern crate pretty_env_logger;

const BUFFER_SIZE: usize = 1024;

#[derive(Debug)]
enum Command {
    Text(String),
    Binary(Vec<u8>),
}

struct State {
    count: usize,
    proxy: SocketAddr,
}

fn string_to_socketaddr(address: &str) -> SocketAddr {
    address.to_socket_addrs().unwrap().next().unwrap()
}

#[tokio::main]
async fn main() {
    pretty_env_logger::init();

    info!("BIT:E WebSocket Proxy");

    let server = match env::var("SERVER") {
        Ok(var) => var,
        Err(_) => {
            info!("Error: The required environmental variable SERVER is missing.");
            info!("The SERVER variable must contain the address of the server.");
            info!("BASH i.e: export SERVER=0.0.0.0:1983");
            return;
        }
    };

    let proxy = match env::var("PROXY") {
        Ok(var) => var,
        Err(_) => {
            info!("Error: The required environmental variable PROXY is missing.");
            info!("The PROXY variable must contain the URI of the BITE server to be proxied.");
            info!("BASH i.e: export PROXY=0.0.0.0:1984");
            return;
        }
    };

    let server = string_to_socketaddr(&server);
    let proxy = string_to_socketaddr(&proxy);
    let shared = Arc::new(Mutex::new(State { count: 0, proxy }));

    let app: Router = Router::new()
        .route("/", get(index))
        .route("/ws", get(move |ws| ws_handler(ws, Arc::clone(&shared))));

    axum::Server::bind(&server)
        .serve(app.into_make_service())
        .await
        .unwrap();
}

async fn index() -> Html<&'static str> {
    Html(include_str!("../web/bite-client.html"))
}

async fn ws_handler(ws: WebSocketUpgrade, state: Arc<Mutex<State>>) -> Response {
    info!("{ws:?}");
    ws.on_upgrade(|ws| start_sockets(ws, state))
}

async fn start_sockets(socket: WebSocket, state: Arc<Mutex<State>>) {
    {
        state.lock().unwrap().count += 1;
    }

    let (ws_tx, mut ws_rx) = mpsc::unbounded_channel::<Command>();
    let (tcp_tx, mut tcp_rx) = mpsc::unbounded_channel::<Command>();

    // WebSocket reader

    let (mut ws_writer, mut ws_reader) = socket.split();

    let mut ws_reader_handler = tokio::spawn(async move {
        while let Some(msg) = ws_reader.next().await {
            match msg {
                Ok(msg) => match msg {
                    Message::Text(text) => {
                        info!("ws -> tcp: {text}");
                        tcp_tx.send(Command::Text(text)).unwrap();
                    }

                    Message::Binary(binary) => {
                        info!("ws -> tcp: Binary");
                        tcp_tx.send(Command::Binary(binary)).unwrap();
                    }

                    Message::Ping(ping) => {
                        info!("ws -> tcp: Ping");
                        tcp_tx.send(Command::Binary(ping)).unwrap();
                    }

                    Message::Pong(pong) => {
                        info!("ws -> tcp: Pong");
                        tcp_tx.send(Command::Binary(pong)).unwrap();
                    }

                    Message::Close(_) => {
                        info!("ws closed");
                        break;
                    }
                },

                Err(err) => {
                    info!("ws closed with error: {err}");
                    break;
                }
            }
        }
    });

    // WebSocket writer

    let mut ws_writer_handler = tokio::spawn(async move {
        while let Some(cmd) = ws_rx.recv().await {
            match cmd {
                Command::Text(text) => {
                    info!("ws write (text): {text}");
                    ws_writer.send(Message::Text(text)).await.unwrap();
                }

                Command::Binary(binary) => {
                    info!("ws write (binary): {binary:?}");
                    ws_writer.send(Message::Binary(binary)).await.unwrap();
                }
            }
        }
    });

    // BITE reader

    let addr = state.lock().unwrap().proxy;
    let (tcp_read, tcp_write) = match TcpStream::connect(addr).await {
        Ok(tcp) => tcp.into_split(),
        Err(_) => return,
    };

    let mut tcp_reader = tokio::spawn(async move {
        let mut buffer = vec![0; BUFFER_SIZE];

        loop {
            tcp_read.readable().await.unwrap();

            if buffer.capacity() == buffer.len() {
                buffer.reserve(BUFFER_SIZE);
            }

            match tcp_read.try_read(&mut buffer) {
                Ok(0) => {
                    // Reading 0 bytes means the other side has closed the
                    // connection or is done writing, then so are we.
                    return;
                }

                Ok(n) => {
                    let received = &buffer[..n];
                    ws_tx.send(Command::Binary(received.to_vec())).unwrap();
                    info!("tcp -> ws: {received:?}");
                }

                // Would block "errors" are the OS's way of saying that the
                // connection is not actually ready to perform this I/O
                // operation.
                Err(ref err) if err.kind() == io::ErrorKind::WouldBlock => continue,

                // Got interrupted, we'll try again.
                Err(ref err) if err.kind() == io::ErrorKind::Interrupted => continue,

                // Other errors we'll consider fatal.
                Err(_) => return,
            }
        }
    });

    // BITE writer

    let mut tcp_writer = tokio::spawn(async move {
        while let Some(cmd) = tcp_rx.recv().await {
            let data = match cmd {
                Command::Text(text) => {
                    info!("tcp try_write (text): {text}");
                    text.into()
                }

                Command::Binary(binary) => {
                    info!("tcp try_write (binary): {binary:?}");
                    binary
                }
            };

            tcp_write.writable().await.unwrap();

            let mut written = 0;
            while written < data.len() {
                match tcp_write.try_write(&data[written..]) {
                    Ok(0) => {
                        // Writing 0 bytes means the other side has closed the
                        // connection or is done writing, then so are we.
                        return;
                    }

                    Ok(n) => {
                        written += n;
                    }

                    // Would block "errors" are the OS's way of saying that the
                    // connection is not actually ready to perform this I/O
                    // operation.
                    Err(ref err) if err.kind() == io::ErrorKind::WouldBlock => return,

                    // Got interrupted, we'll try again.
                    Err(ref err) if err.kind() == io::ErrorKind::Interrupted => continue,

                    // Other errors we'll consider fatal.
                    Err(_) => return,
                }
            }
        }
    });

    // Everyone fails together.

    tokio::select! {
        _ = (&mut ws_reader_handler) => { info!("ws_reader closed"); ws_writer_handler.abort(); tcp_reader.abort(); tcp_writer.abort(); },
        _ = (&mut ws_writer_handler) => { info!("ws_writer closed"); ws_reader_handler.abort(); tcp_reader.abort(); tcp_writer.abort(); },
        _ = (&mut tcp_reader) => { info!("tcp_reader closed"); ws_reader_handler.abort(); ws_writer_handler.abort(); tcp_writer.abort(); },
        _ = (&mut tcp_writer) => { info!("tcp_writer closed"); ws_reader_handler.abort(); ws_writer_handler.abort(); tcp_reader.abort(); },
    };

    // One less.

    state.lock().unwrap().count -= 1;
}
