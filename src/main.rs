mod bite_client;
use bite_client::bite_reader::BiteReader;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::response::Response;
use axum::routing::get;
use axum::Router;
use bite_client::connection::Connection;
use bite_client::message::Messages;
use futures::stream::{SplitSink, SplitStream, StreamExt};
use tokio::net::TcpStream;

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::thread;

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
    ws.on_upgrade(|ws| handle_socket(ws, state))
}

async fn handle_socket(socket: WebSocket, state: Arc<Mutex<State>>) {
    let (sender, receiver) = socket.split();

    let mut state = state.lock().unwrap();
    state.count += 1;

    let server_addr = SocketAddr::from(([127, 0, 0, 1], 1984));

    // if let Ok(mut t) = TcpStream::connect(server_addr).await {
    //     let (tcp_sender, tcp_receiver) = t.split();
    // }

    // let reader = TcpStream::connect(server_addr).unwrap();
    // reader.set_nonblocking(true).unwrap();
    // let writer = reader.try_clone().unwrap();

    // let read_conn = Connection::new(state.count, reader, server_addr);
    // let write_conn = Connection::new(state.count, writer, server_addr);
    // let messages = Messages::new();

    // let bite_write = Bitec::new(state.count, server);
    // let bite_read = Bitec::new(state.count, server);

    // @todo

    drop(state);

    // tokio::spawn(ws_write(sender, read_conn));
    // tokio::spawn(ws_read(receiver, write_conn));
}

async fn ws_read(mut receiver: SplitStream<WebSocket>, conn: Connection) {
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

async fn ws_write(sender: SplitSink<WebSocket, Message>, conn: Connection) {
    loop {}
}
