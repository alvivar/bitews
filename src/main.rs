use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::response::Response;
use axum::routing::get;
use axum::Router;

use futures::sink::SinkExt;
use futures::stream::{SplitSink, SplitStream, StreamExt};
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::net::TcpStream;
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};

use std::io;
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
    let mut state = state.lock().unwrap();
    state.count += 1;
    drop(state);

    let (ws_sender, ws_receiver) = mpsc::unbounded_channel::<Command>();
    let (bite_sender, bite_receiver) = mpsc::unbounded_channel::<Command>();

    let bite_addr = SocketAddr::from(([127, 0, 0, 1], 1984));

    tokio::spawn(handle_websocket(socket, bite_sender, bite_receiver));
    tokio::spawn(handle_bite(bite_addr, ws_sender, ws_receiver));
}

async fn handle_websocket(
    socket: WebSocket,
    bite_sender: UnboundedSender<Command>,
    bite_receiver: UnboundedReceiver<Command>,
) {
    let (sender, receiver) = socket.split();

    tokio::spawn(ws_writer(sender, bite_receiver));
    tokio::spawn(ws_reader(receiver, bite_sender));
}

async fn handle_bite(
    addr: SocketAddr,
    ws_sender: UnboundedSender<Command>,
    ws_receiver: UnboundedReceiver<Command>,
) {
    match TcpStream::connect(addr).await {
        Ok(tcp) => {
            let (receiver, sender) = tcp.into_split();

            tokio::spawn(bite_writer(sender, ws_receiver));
            tokio::spawn(bite_reader(receiver, ws_sender));
        }

        Err(e) => {
            println!("Error: {:?}", e);
        }
    }
}

async fn ws_reader(mut receiver: SplitStream<WebSocket>, bite_sender: UnboundedSender<Command>) {
    loop {
        match receiver.next().await {
            Some(Ok(msg)) => match msg {
                Message::Text(text) => {
                    println!("Text: {}", text);
                    bite_sender.send(Command::Text(text)).unwrap();
                }

                Message::Binary(binary) => {
                    println!("Binary received");
                    bite_sender.send(Command::Binary(binary)).unwrap();
                }

                Message::Ping(binary) => {
                    println!("Ping");
                    bite_sender.send(Command::Binary(binary)).unwrap();
                }

                Message::Pong(binary) => {
                    println!("Pong");
                    bite_sender.send(Command::Binary(binary)).unwrap();
                }

                Message::Close(_) => {
                    // CloseFrame?

                    println!("Client disconnected");

                    // Handling client disconnection, including BITE.

                    return;
                }
            },

            Some(Err(err)) => {
                println!("Error: {:?}", err);
                break;
            }

            None => {
                println!("WebSocket receiver closed");
                break;
            }
        }
    }
}

async fn ws_writer(
    mut sender: SplitSink<WebSocket, Message>,
    mut bite_receiver: UnboundedReceiver<Command>,
) {
    loop {
        match bite_receiver.recv().await {
            Some(Command::Text(text)) => {
                println!("Text -> BITE: {}", text);
                sender.send(Message::Text(text)).await.unwrap();
            }

            Some(Command::Binary(binary)) => {
                println!("Binary -> BITE");
                sender.send(Message::Binary(binary)).await.unwrap();
            }

            None => {
                println!("WebSocket bite_receiver closed");
                break;
            }
        }
    }
}

async fn bite_reader(reader: OwnedReadHalf, ws_sender: UnboundedSender<Command>) {
    loop {
        reader.readable().await.unwrap();

        let mut received = vec![0; 4096];
        let mut bytes_read = 0;

        loop {
            match reader.try_read(&mut received) {
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
        ws_sender.send(Command::Binary(received.to_vec())).unwrap();
    }
}

async fn bite_writer(writer: OwnedWriteHalf, mut ws_receiver: UnboundedReceiver<Command>) {
    loop {
        match ws_receiver.recv().await {
            Some(_) => todo!(),
            None => todo!(),
        }
    }
}
