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
use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, Mutex};

#[derive(Debug)]
enum Command {
    Text(String),
    Binary(Vec<u8>),
}

struct State {
    count: usize,
    proxy: String,
}

#[tokio::main]
async fn main() {
    println!("\nBIT:E WebSocket Proxy\n");

    let server = match env::var("SERVER") {
        Ok(var) => var,
        Err(_) => {
            println!("Environmental variable SERVER is missing!");
            println!("The URI where the server is gonna receive connections.");
            println!("BASH i.e: export SERVER=0.0.0.0:1983");

            return ();
        }
    };

    let proxy = match env::var("PROXY") {
        Ok(var) => var,
        Err(_) => {
            println!("Environmental variable PROXY is missing!");
            println!("That's the Bite server that we are gonna proxy.");
            println!("BASH i.e: export PROXY=0.0.0.0:1984");

            return ();
        }
    };

    let shared = Arc::new(Mutex::new(State { count: 0, proxy }));

    let app: Router = Router::new()
        .route("/", get(index))
        .route("/ws", get(move |ws| ws_handler(ws, Arc::clone(&shared))));

    let (host_str, port_str) = server.split_at(server.find(':').unwrap());
    let host = host_str.parse::<IpAddr>().unwrap();
    let port = port_str[1..].parse::<u16>().unwrap();

    let addr = SocketAddr::from((host, port));
    axum::Server::bind(&addr)
        .serve(app.into_make_service())
        .await
        .unwrap();
}

async fn index() -> Html<&'static str> {
    Html(include_str!("../web/bite-client.html"))
}

async fn ws_handler(ws: WebSocketUpgrade, state: Arc<Mutex<State>>) -> Response {
    println!("\n{:?}\n", ws);
    ws.on_upgrade(|ws| start_sockets(ws, state))
}

async fn start_sockets(socket: WebSocket, state: Arc<Mutex<State>>) {
    {
        let mut state = state.lock().unwrap();
        state.count += 1;
    }

    let (ws_tx, mut ws_rx) = mpsc::unbounded_channel::<Command>();
    let (tcp_tx, mut tcp_rx) = mpsc::unbounded_channel::<Command>();

    // WebSocket reader

    let (mut ws_writer, mut ws_reader) = socket.split();

    let mut ws_reader_handler = tokio::spawn(async move {
        while let Some(msg) = ws_reader.next().await {
            match msg.unwrap() {
                Message::Text(text) => {
                    println!("ws -> tcp: {}", text);
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
            }
        }
    });

    // WebSocket writer

    let mut ws_writer_handler = tokio::spawn(async move {
        while let Some(cmd) = ws_rx.recv().await {
            match cmd {
                Command::Text(text) => {
                    println!("ws write (text): {}", text);
                    ws_writer.send(Message::Text(text)).await.unwrap();
                }

                Command::Binary(binary) => {
                    println!("ws write (binary): {:?}", binary);
                    ws_writer.send(Message::Binary(binary)).await.unwrap();
                }
            }
        }
    });

    // BITE reader
    let proxy = state.lock().unwrap().proxy.clone();
    let (host_str, port_str) = proxy.split_at(proxy.find(':').unwrap());
    let host = host_str.parse::<IpAddr>().unwrap();
    let port = port_str[1..].parse::<u16>().unwrap();

    let addr = SocketAddr::from((host, port));
    let (tcp_read, tcp_write) = match TcpStream::connect(addr).await {
        Ok(tcp) => tcp.into_split(),
        Err(_) => return,
    };

    let mut tcp_reader = tokio::spawn(async move {
        loop {
            tcp_read.readable().await.unwrap();

            let mut received = vec![0; 4096];
            let mut bytes_read = 0;

            loop {
                match tcp_read.try_read(&mut received) {
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
            println!("tcp -> ws: {:?}", received);

            ws_tx.send(Command::Binary(received.to_vec())).unwrap();
        }
    });

    // BITE writer

    let mut tcp_writer = tokio::spawn(async move {
        while let Some(cmd) = tcp_rx.recv().await {
            match cmd {
                Command::Text(text) => {
                    println!("tcp try_write (text): {}", text);
                    tcp_write.try_write(text.as_bytes()).unwrap();
                }

                Command::Binary(binary) => {
                    println!("tcp try_write (binary): {:?}", binary);
                    tcp_write.try_write(&binary).unwrap();
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
}
