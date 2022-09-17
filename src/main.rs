use axum::extract::ws::WebSocketUpgrade;
use axum::extract::ws::{Message, WebSocket};
use axum::response::Response;
use axum::routing::get;
use axum::Router;
use futures::stream::{SplitSink, SplitStream, StreamExt};
use std::net::SocketAddr;

#[tokio::main]
async fn main() {
    let app: Router = Router::new().route("/ws", get(handler));

    let addr = SocketAddr::from(([127, 0, 0, 1], 3000));

    axum::Server::bind(&addr)
        .serve(app.into_make_service())
        .await
        .unwrap();
}

async fn handler(ws: WebSocketUpgrade) -> Response {
    println!("New connection: {:?}", ws);
    ws.on_upgrade(handle_socket)
}

async fn handle_socket(socket: WebSocket) {
    let (sender, receiver) = socket.split();

    tokio::spawn(write(sender));
    tokio::spawn(read(receiver));
}

async fn read(mut receiver: SplitStream<WebSocket>) {
    loop {
        match receiver.next().await {
            Some(Ok(msg)) => match msg {
                Message::Text(t) => {
                    println!("Text: {}", t);
                }

                Message::Binary(_) => {
                    println!("Binary data");
                }

                Message::Ping(_) => {
                    println!("Ping");
                }

                Message::Pong(_) => {
                    println!("Pong");
                }

                Message::Close(_) => {
                    println!("Client disconnected");
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

async fn write(sender: SplitSink<WebSocket, Message>) {
    loop {}
}
