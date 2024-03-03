use fastwebsockets::Frame;
use std::{collections::HashSet, net::SocketAddr, sync::Arc};
use tokio::sync::RwLock;

pub type SharedState = Arc<RwLock<State>>;

pub struct State {
    pub connected: HashSet<SocketAddr>,
    pub disconnected: HashSet<SocketAddr>,
}

#[allow(dead_code)]
#[derive(Clone, Debug)]
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
