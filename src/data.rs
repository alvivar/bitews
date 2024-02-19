use fastwebsockets::Frame;
use tokio::sync::{mpsc::Sender, RwLock};

use std::{collections::HashMap, net::SocketAddr, sync::Arc};

pub type Tx = Sender<Message>;
pub type SharedState = Arc<RwLock<State>>;

pub struct State {
    pub clients: HashMap<SocketAddr, Tx>,
}

impl State {
    pub async fn broadcast(&self, sender: &SocketAddr, msg: Message) {
        for (addr, tx) in self.clients.iter() {
            if addr != sender {
                tx.send(msg.clone()).await.unwrap();
            }
        }
    }
}

#[derive(Clone, Debug)]
#[allow(dead_code)]
pub enum Message {
    Text(String),
    Binary(Vec<u8>),
    Pong(Vec<u8>),
    Close(u16, String), // Code, Reason
}

impl Message {
    pub fn as_frame(&self) -> Frame {
        match self {
            Message::Text(text) => Frame::text(text.as_bytes().into()),
            Message::Binary(data) => Frame::binary(data.as_slice().into()),
            Message::Pong(data) => Frame::pong(data.as_slice().into()),
            Message::Close(code, reason) => Frame::close(*code, reason.as_bytes()),
        }
    }
}
