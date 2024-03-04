use anyhow::Result;
use fastwebsockets::{
    upgrade::{self, upgrade},
    FragmentCollector, OpCode, WebSocketError,
};
use http_body_util::Full;
use hyper::{
    body::{Bytes, Incoming},
    server::conn::http1,
    service::service_fn,
    Request, Response,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{tcp::OwnedReadHalf, TcpListener, TcpStream},
    sync::{mpsc, RwLock},
};

use std::{
    collections::{HashMap, HashSet},
    env, io,
    net::{SocketAddr, ToSocketAddrs},
    sync::Arc,
};

mod data;
use data::{Message, SharedState, State};

mod filemap;
use filemap::{FileData, FileMap};

async fn request_handler(
    mut request: Request<Incoming>,
    address: SocketAddr,
    proxy: SocketAddr,
    state: SharedState,
    static_files: Arc<HashMap<String, FileMap>>,
) -> Result<Response<Full<Bytes>>, WebSocketError> {
    let mut uri = request.uri().path();

    if uri == "/" {
        uri = "/index.html";
    }

    match uri {
        "/ws" => {
            let (fut_response, fut) = upgrade(&mut request)?;

            tokio::spawn(async move {
                if let Err(err) = handle_ws(fut, address, proxy, &state).await {
                    eprintln!("{} WebSocket Error: {}", address, err);
                } else {
                    let mut state = state.write().await;
                    state.connected.remove(&address);
                    state.disconnected.insert(address);
                }
            });

            let mut response = Response::builder()
                .status(fut_response.status())
                .body(Full::default())
                .unwrap();

            response.headers_mut().clone_from(fut_response.headers());

            Ok(response)
        }

        _ => {
            if let Some(map) = static_files.get(uri) {
                let response = serve_file(&map.data, map.mime_type).await.unwrap();

                Ok(response)
            } else {
                let response = Response::builder()
                    .status(404)
                    .body(Full::from("Not found (404)"))
                    .unwrap();

                Ok(response)
            }
        }
    }
}

async fn handle_ws(
    fut: upgrade::UpgradeFut,
    address: SocketAddr,
    proxy: SocketAddr,
    state: &SharedState,
) -> Result<(), WebSocketError> {
    let mut ws = FragmentCollector::new(fut.await.unwrap());

    {
        let mut state = state.write().await;
        state.connected.insert(address);
        state.disconnected.remove(&address);
    }

    println!("{} New", address);

    // BITE
    let bite_stream = TcpStream::connect(proxy).await?;
    let (mut bite_read, mut bite_write) = bite_stream.into_split();
    let (bite_tx, mut bite_rx) = mpsc::channel::<Vec<u8>>(128);

    loop {
        tokio::select! {
            frame = ws.read_frame() => {
                let frame = frame?;
                match frame.opcode {
                    OpCode::Close => {
                        println!("{} Closed", address);
                        break;
                    }

                    OpCode::Text => {
                        bite_write.write_all(frame.payload.as_ref()).await?;
                        bite_write.flush().await?;
                    }

                    OpCode::Binary => {
                        bite_write.write_all(frame.payload.as_ref()).await?;
                        bite_write.flush().await?;
                    }

                    _ => {}
                }
            },

            bite_result = bite_reader(&mut bite_read, &bite_tx) => {
                if let Err(err) = bite_result {
                    eprintln!("{} BITE Read Error: {}", address, err);
                    return Err(ws_error(&err.to_string()));
                }
            },

            from_bite_reader = bite_rx.recv() => {
                if let Some(message) = from_bite_reader {
                    let message = Message::Binary(message);
                    ws.write_frame(message.as_frame()).await?;
                } else {
                    let err = "BITE Receiver Error";
                    eprintln!("{} {}.", address, err);
                    return Err(ws_error(err));
                }
            },

        }
    }

    Ok(())
}

async fn bite_reader(reader: &mut OwnedReadHalf, tx: &mpsc::Sender<Vec<u8>>) -> io::Result<()> {
    let mut buffer = [0u8; 4096];

    loop {
        let n = reader.read(&mut buffer).await?;

        if n == 0 {
            return Err(io_error("End of file"));
        }

        if tx.send(buffer[..n].to_vec()).await.is_err() {
            return Err(io_error("Send error"));
        }
    }
}

async fn serve_file(data: &FileData, mime_type: &'static str) -> Result<Response<Full<Bytes>>> {
    let body = match data {
        FileData::Bytes(bytes) => bytes.clone(),
    };

    let response = Response::builder()
        .status(200)
        .header("Content-Type", mime_type)
        .body(Full::from(body))?;

    Ok(response)
}

fn ws_error(err: &str) -> WebSocketError {
    WebSocketError::IoError(io::Error::other(err))
}

fn io_error(err: &str) -> io::Error {
    io::Error::new(io::ErrorKind::Other, err)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let server_addr = env::var("SERVER").unwrap_or_else(|_| "0.0.0.0:1984".to_string());
    let server_addr = server_addr.to_socket_addrs()?.next().unwrap();
    let proxy_addr = env::var("PROXY").unwrap_or_else(|_| "0.0.0.0:1983".to_string());
    let proxy_addr = proxy_addr.to_socket_addrs()?.next().unwrap();

    let address = SocketAddr::from(server_addr);
    let proxy = SocketAddr::from(proxy_addr);

    let listener = TcpListener::bind(address).await?;
    println!("{} Listening", address);

    let state = Arc::new(RwLock::new(State {
        connected: HashSet::new(),
        disconnected: HashSet::new(),
    }));

    let static_files = FileMap::static_files();

    loop {
        let (stream, address) = listener.accept().await?;
        let state = state.clone();
        let static_files = static_files.clone();

        tokio::task::spawn(async move {
            let io = hyper_util::rt::TokioIo::new(stream);
            let connection = http1::Builder::new()
                .serve_connection(
                    io,
                    service_fn(move |request| {
                        request_handler(
                            request,
                            address,
                            proxy,
                            state.clone(),
                            static_files.clone(),
                        )
                    }),
                )
                .with_upgrades();

            if let Err(err) = connection.await {
                eprintln!("Connection Error: {:?}", err);
            }
        });
    }
}
