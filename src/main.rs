use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::response::{Html, Response};
use axum::routing::get;
use axum::Router;
use futures::sink::SinkExt;
use futures::stream::StreamExt;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::mpsc;

use std::env;
use std::include_str;
use std::io;
use std::net::{SocketAddr, ToSocketAddrs};
use std::sync::{Arc, Mutex};

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
    println!("\nBIT:E WebSocket Proxy\n");

    let server = match env::var("SERVER") {
        Ok(var) => var,
        Err(_) => {
            println!("Error: The required environmental variable SERVER is missing.");
            println!("The SERVER variable must contain the address of the server.");
            println!("BASH i.e: export SERVER=0.0.0.0:1983");
            return;
        }
    };

    let proxy = match env::var("PROXY") {
        Ok(var) => var,
        Err(_) => {
            println!("Error: The required environmental variable PROXY is missing.");
            println!("The PROXY variable must contain the URI of the BITE server to be proxied.");
            println!("BASH i.e: export PROXY=0.0.0.0:1984");
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
    println!("\n{ws:?}\n");
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
                        println!("ws -> tcp: {text}");
                        tcp_tx.send(Command::Text(text)).unwrap();
                    }

                    Message::Binary(binary) => {
                        println!("ws -> tcp: Binary");
                        tcp_tx.send(Command::Binary(binary)).unwrap();
                    }

                    Message::Ping(ping) => {
                        println!("ws -> tcp: Ping");
                        tcp_tx.send(Command::Binary(ping)).unwrap();
                    }

                    Message::Pong(pong) => {
                        println!("ws -> tcp: Pong");
                        tcp_tx.send(Command::Binary(pong)).unwrap();
                    }

                    Message::Close(_) => {
                        println!("ws closed");
                        break;
                    }
                },

                Err(err) => {
                    println!("ws closed with error: {err}");
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
                    println!("ws write (text): {text}");
                    ws_writer.send(Message::Text(text)).await.unwrap();
                }

                Command::Binary(binary) => {
                    println!("ws write (binary): {binary:?}");
                    ws_writer.send(Message::Binary(binary)).await.unwrap();
                }
            }
        }
    });

    // BITE reader

    let addr = state.lock().unwrap().proxy;
    let (mut tcp_read, mut tcp_write) = match TcpStream::connect(addr).await {
        Ok(tcp) => tcp.into_split(),
        Err(_) => return,
    };

    let mut tcp_reader = tokio::spawn(async move {
        loop {
            tcp_read.readable().await.unwrap();

            let mut received = vec![0; 8192];
            let mut bytes_read = 0;

            loop {
                match tcp_read.read(&mut received).await {
                    Ok(0) => {
                        // Reading 0 bytes means the other side has closed the
                        // connection or is done writing, then so are we.
                        return;
                    }

                    Ok(n) => {
                        bytes_read += n;
                        if bytes_read == received.len() {
                            received.resize(received.len() + 1024, 0);
                        }
                    }

                    // Would block "errors" are the OS's way of saying that the
                    // connection is not actually ready to perform this I/O operation.
                    Err(ref err) if err.kind() == io::ErrorKind::WouldBlock => break,

                    // Got interrupted (how rude!), we'll try again.
                    Err(ref err) if err.kind() == io::ErrorKind::Interrupted => continue,

                    // Other errors we'll consider fatal.
                    Err(_) => return,
                }
            }

            let received = &received[..bytes_read];
            println!("tcp -> ws: {received:?}");

            ws_tx.send(Command::Binary(received.to_vec())).unwrap();
        }
    });

    // BITE writer

    let mut tcp_writer = tokio::spawn(async move {
        while let Some(cmd) = tcp_rx.recv().await {
            match cmd {
                Command::Text(text) => {
                    println!("tcp try_write (text): {text}");
                    tcp_write.write_all(text.as_bytes()).await.unwrap();
                }

                Command::Binary(binary) => {
                    println!("tcp try_write (binary): {binary:?}");
                    tcp_write.write_all(&binary).await.unwrap();
                }
            }
        }
    });

    // Everyone fails together.

    tokio::select! {
        _ = (&mut ws_reader_handler) => { println!("ws_reader end"); ws_writer_handler.abort(); tcp_reader.abort(); tcp_writer.abort(); },
        _ = (&mut ws_writer_handler) => { println!("ws_writer end"); ws_reader_handler.abort(); tcp_reader.abort(); tcp_writer.abort(); },
        _ = (&mut tcp_reader) => { println!("tcp_reader end"); ws_reader_handler.abort(); ws_writer_handler.abort(); tcp_writer.abort(); },
        _ = (&mut tcp_writer) => { println!("tcp_writer end"); ws_reader_handler.abort(); ws_writer_handler.abort(); tcp_reader.abort(); },
    };

    // One less.

    state.lock().unwrap().count -= 1;
}
