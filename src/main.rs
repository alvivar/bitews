use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::response::Response;
use axum::routing::get;
use axum::Router;

use futures::sink::SinkExt;
use futures::stream::StreamExt;
use tokio::net::TcpStream;
use tokio::sync::mpsc::{self};

use std::io::{self};
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

#[derive(Debug)]
enum Command {
    Text(String),
    Binary(Vec<u8>),
}

struct State {
    count: usize,
}

#[tokio::main]
async fn main() {
    let shared = Arc::new(Mutex::new(State { count: 0 }));

    let app: Router = Router::new().route("/ws", get(move |ws| handler(ws, Arc::clone(&shared))));
    let addr = SocketAddr::from(([0, 0, 0, 0], 3000));

    axum::Server::bind(&addr)
        .serve(app.into_make_service())
        .await
        .unwrap();
}

async fn handler(ws: WebSocketUpgrade, state: Arc<Mutex<State>>) -> Response {
    println!("New connection: {:?}", ws);
    ws.on_upgrade(|ws| start_sockets(ws, state))
}

async fn start_sockets(socket: WebSocket, state: Arc<Mutex<State>>) {
    let (mut ws_write, mut ws_read) = socket.split();
    let (ws_tx, mut ws_rx) = mpsc::unbounded_channel::<Command>();
    let (bite_tx, mut bite_rx) = mpsc::unbounded_channel::<Command>();

    // WebSocket reader

    let mut websocket_reader = tokio::spawn(async move {
        while let Some(msg) = ws_read.next().await {
            match msg.unwrap() {
                Message::Text(text) => {
                    println!("BITE -> Text: {}", text);
                    bite_tx.send(Command::Text(text)).unwrap();
                }

                Message::Binary(binary) => {
                    println!("BITE -> Binary");
                    bite_tx.send(Command::Binary(binary)).unwrap();
                }

                Message::Ping(ping) => {
                    println!("BITE -> Ping");
                    bite_tx.send(Command::Binary(ping)).unwrap();
                }

                Message::Pong(pong) => {
                    println!("BITE -> Pong");
                    bite_tx.send(Command::Binary(pong)).unwrap();
                }

                Message::Close(_) => {
                    println!("BITE -> Close");
                    break;
                }
            }
        }
    });

    // WebSocket writer

    let mut websocket_writer = tokio::spawn(async move {
        while let Some(cmd) = ws_rx.recv().await {
            match cmd {
                Command::Text(text) => {
                    println!("Text -> BITE: {}", text);
                    ws_write.send(Message::Text(text)).await.unwrap();
                }

                Command::Binary(binary) => {
                    println!("Binary -> BITE");
                    ws_write.send(Message::Binary(binary)).await.unwrap();
                }
            }
        }
    });

    // BITE reader

    let bite_addr = SocketAddr::from(([127, 0, 0, 1], 1984));
    let (bite_read, bite_write) = match TcpStream::connect(bite_addr).await {
        Ok(tcp) => tcp.into_split(),
        Err(_) => return,
    };

    let mut bite_reader = tokio::spawn(async move {
        loop {
            bite_read.readable().await.unwrap();

            let mut received = vec![0; 4096];
            let mut bytes_read = 0;

            loop {
                match bite_read.try_read(&mut received) {
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
            ws_tx.send(Command::Binary(received.to_vec())).unwrap();
        }
    });

    // BITE writer

    let mut bite_writer = tokio::spawn(async move {
        while let Some(cmd) = bite_rx.recv().await {
            match cmd {
                Command::Text(text) => {
                    println!("Text: {}", text);
                    bite_write.try_write(text.as_bytes()).unwrap();
                }

                Command::Binary(binary) => {
                    println!("Binary received");
                    bite_write.try_write(&binary).unwrap();
                }
            }
        }
    });

    // Everyone fails together.

    tokio::select! {
        _ = (&mut websocket_reader) => { websocket_writer.abort(); bite_writer.abort(); },
        _ = (&mut websocket_writer) => { websocket_reader.abort(); bite_reader.abort(); },
        _ = (&mut bite_reader) => { bite_writer.abort(); websocket_writer.abort(); },
        _ = (&mut bite_writer) => { bite_reader.abort(); websocket_reader.abort(); },
    };

    let mut state = state.lock().unwrap();
    state.count += 1;
    drop(state);
}
