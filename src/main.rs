use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::response::Response;
use axum::routing::get;
use axum::Router;
use futures::io::WriteHalf;
use futures::stream::{SplitSink, SplitStream, StreamExt};
use tokio::net::TcpStream;
use tokio::sync::mpsc::{self, UnboundedSender};

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::thread;

enum Command {
    Write,
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
    thread::spawn(move || loop {});

    println!("New connection: {:?}", ws);
    ws.on_upgrade(|ws| init_sockets(ws, state))
}

async fn init_sockets(socket: WebSocket, state: Arc<Mutex<State>>) {
    let mut state = state.lock().unwrap();
    state.count += 1;
    drop(state);

    let (bite_sender, bite_receiver) = mpsc::unbounded_channel::<Command>();
    let (ws_sender, ws_receiver) = mpsc::unbounded_channel::<Command>();

    let bite_addr = SocketAddr::from(([127, 0, 0, 1], 1984));
    tokio::spawn(handle_bite(bite_addr, ws_sender));
    tokio::spawn(handle_websocket(socket));

    // tokio::spawn(ws_writer(ws_sender));
    // tokio::spawn(ws_reader(ws_receiver));
}

async fn handle_bite(addr: SocketAddr, ws_sender: UnboundedSender<Command>) {
    match TcpStream::connect(addr).await {
        Ok(mut tcp) => {
            let (tcp_sender, tcp_receiver) = tcp.split();
            // tokio::spawn(bite_reader(tcp_receiver));
            // tokio::spawn(bite_writer(tcp_sender));
        }

        Err(e) => {
            println!("Error: {:?}", e);
        }
    }
}

async fn handle_websocket(socket: WebSocket) {
    let (ws_sender, ws_receiver) = socket.split();
}

async fn bite_reader(bite_receiver: WriteHalf<TcpStream>) {}

async fn bite_writer(mut sender: UnboundedSender<Command>) {}

async fn ws_reader(mut receiver: SplitStream<WebSocket>) {
    loop {
        match receiver.next().await {
            Some(Ok(msg)) => match msg {
                Message::Text(text) => {
                    println!("Text: {}", text);
                }

                Message::Binary(_) => {
                    println!("Binary received");
                }

                Message::Ping(_) => {
                    println!("Ping");
                }

                Message::Pong(_) => {
                    println!("Pong");
                }

                Message::Close(_) => {
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
                println!("Connection closed");
                break;
            }
        }
    }
}

async fn ws_writer(sender: SplitSink<WebSocket, Message>) {
    loop {}
}
