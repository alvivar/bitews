use async_openai::{
    config::OpenAIConfig,
    types::{ChatCompletionRequestUserMessageArgs, CreateChatCompletionRequestArgs, Role},
    Client,
};
use fastwebsockets::{
    upgrade::{self, upgrade},
    FragmentCollector, OpCode, WebSocketError,
};
use hyper::{
    body::{Bytes, Incoming},
    server::conn::http1,
    service::service_fn,
    Request, Response,
};
use tokio::{
    net::TcpListener,
    sync::{mpsc, RwLock},
};

use anyhow::Result;
use futures::StreamExt;
use http_body_util::Full;

use std::{
    collections::HashMap,
    env, fs,
    io::{self},
    net::SocketAddr,
    sync::Arc,
};

mod data;
use data::{Message, SharedState, State};

mod filemap; // Static files are served from here.
use filemap::{FileData, FileMap};

const SERVER_ID: &str = "//server"; // This is used to identify server messages.

async fn request_handler(
    mut request: Request<Incoming>,
    address: SocketAddr,
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
                handle_ws(fut, address, &state).await.unwrap();

                {
                    let mut state = state.write().await;
                    state.clients.remove(&address);
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
    state: &SharedState,
) -> Result<(), WebSocketError> {
    let mut ws = FragmentCollector::new(fut.await.unwrap());

    let (tx, mut rx) = mpsc::channel(128);

    {
        let mut state = state.write().await;
        state.clients.insert(address, tx);
    }

    println!("{} New", address);

    let (openai_ws_tx, mut openai_ws_rx) = mpsc::channel::<String>(128);
    let client = Client::new();

    // Session data.

    let prompts: HashMap<SocketAddr, Vec<String>> = HashMap::new();
    let prompts = Arc::new(RwLock::new(prompts));
    let current_model: Arc<RwLock<String>> = Arc::new(RwLock::new("gpt-3.5-turbo-1106".into()));

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
                        let prompt = String::from_utf8(frame.payload.to_vec()).unwrap();
                        store_prompt(address, prompt.clone(), &prompts).await;

                        ws.write_frame(Message::Text(prompt.clone()).as_frame()).await?;
                        ws.write_frame(Message::Text("\0".into()).as_frame()).await?;

                        // Commands.

                        let commands = extract_commands(&prompt);

                        let mut has_model = false;
                        let model = match commands.get("model") {
                            Some(to_model) =>
                            {
                                {
                                    let mut model = current_model.write().await;
                                    *model = to_model.clone();
                                }

                                ws.write_frame(Message::Text(format!("{} Alert: Model set to {}.", SERVER_ID, to_model)).as_frame()).await?;
                                ws.write_frame(Message::Text("\0".into()).as_frame()).await?;

                                has_model = true;
                                to_model.clone()
                            },

                            None => {
                                current_model.read().await.clone()
                            },
                        };

                        if commands.get("info").is_some() {
                            ws.write_frame(Message::Text(format!("{} Info: Using {} model.", SERVER_ID, model)).as_frame()).await?;
                            ws.write_frame(Message::Text("\0".into()).as_frame()).await?;
                            continue;
                        }
                        else if !has_model
                        {
                            ws.write_frame(Message::Text(format!("{} Info: {}", SERVER_ID, model)).as_frame()).await?;
                            ws.write_frame(Message::Text("\0".into()).as_frame()).await?;
                        };

                        tokio::spawn(process_openai_request(
                            prompt,
                            openai_ws_tx.clone(),
                            client.clone(),
                            model,
                        ));
                    }

                    _ => {}
                }
            },

            message = openai_ws_rx.recv() => {
                if let Some(message) = message {
                    let message = Message::Text(message);
                    ws.write_frame(message.as_frame()).await?;
                } else {
                    break;
                }
            },

            frame = rx.recv() => {
                if let Some(frame) = frame {
                    ws.write_frame(frame.as_frame()).await?;
                } else {
                    break;
                }
            }
        }
    }

    Ok(())
}

async fn process_openai_request(
    prompt: String,
    openai_to_ws_tx: mpsc::Sender<String>,
    client: Client<OpenAIConfig>,
    model: String,
) -> anyhow::Result<()> {
    let request = CreateChatCompletionRequestArgs::default()
        .model(model)
        .max_tokens(1024u16)
        .messages([ChatCompletionRequestUserMessageArgs::default()
            .content(prompt)
            .role(Role::User)
            .build()?
            .into()])
        .build()?;

    let mut stream = client.chat().create_stream(request).await?;
    while let Some(result) = stream.next().await {
        match result {
            Ok(response) => {
                for chat_choice in response.choices.iter() {
                    if let Some(ref content) = chat_choice.delta.content {
                        openai_to_ws_tx.send(content.clone()).await.unwrap();
                    }
                }
            }

            Err(err) => {
                eprintln!("OpenAI Error: {}", err);

                openai_to_ws_tx
                    .send(format!("{} OpenAI Error: {}", SERVER_ID, err))
                    .await
                    .unwrap();
            }
        }
    }

    // This means that the transmission is over.
    openai_to_ws_tx.send("\0".into()).await.unwrap();

    Ok(())
}

async fn serve_file(data: &FileData, mime_type: &'static str) -> Result<Response<Full<Bytes>>> {
    let body = match data {
        FileData::Bytes(bytes) => Bytes::from(bytes.to_vec()),
    };

    let response = Response::builder()
        .status(200)
        .header("Content-Type", mime_type)
        .body(Full::from(body))?;

    Ok(response)
}

async fn store_prompt(
    address: SocketAddr,
    prompt: String,
    prompts: &Arc<RwLock<HashMap<SocketAddr, Vec<String>>>>,
) {
    let mut prompts = prompts.write().await;
    let prompts = prompts.entry(address).or_insert(Vec::new());
    prompts.push(prompt);
}

async fn get_prompts(
    address: SocketAddr,
    prompts: Arc<RwLock<HashMap<SocketAddr, Vec<String>>>>,
    num_prompts: usize,
) -> Vec<String> {
    let prompts = prompts.read().await;

    if let Some(prompt_list) = prompts.get(&address) {
        let len = prompt_list.len();

        // If there are fewer prompts than requested, return all of them.
        if len <= num_prompts {
            return prompt_list.clone();
        }

        // Otherwise, return the last `num_prompts`.
        return prompt_list[len - num_prompts..].to_vec();
    }

    // Return an empty Vec if no prompts are found for the address.
    Vec::new()
}

// All "!command value" from a string are extracted and returned as a HashMap.
fn extract_commands(input: &str) -> HashMap<String, String> {
    let mut commands = HashMap::new();
    let mut iter = input.split_whitespace().peekable();

    while let Some(word) = iter.next() {
        if word.starts_with('!') && word.len() > 1 {
            let command = &word[1..];
            let value = iter.next().unwrap_or("").to_string();
            commands.insert(command.to_string(), value);
        }
    }

    commands
}

fn set_environment_from_file(file_path: &str) -> io::Result<()> {
    let contents = fs::read_to_string(file_path)?;

    for line in contents.lines() {
        let parts: Vec<&str> = line.splitn(2, '=').collect();
        if parts.len() == 2 {
            let key = parts[0].trim();
            let value = parts[1].trim();
            env::set_var(key, value);
        } else {
            eprintln!("set_environment_from_file Warning: Skipping line: {}", line);
        }
    }

    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    set_environment_from_file(".env")?;

    let address = SocketAddr::from(([127, 0, 0, 1], 8080));
    let listener = TcpListener::bind(address).await?;

    println!("{} Listening", address);

    let state = Arc::new(RwLock::new(State {
        clients: HashMap::new(),
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
                        request_handler(request, address, state.clone(), static_files.clone())
                    }),
                )
                .with_upgrades();

            if let Err(err) = connection.await {
                eprintln!("Connection Error: {:?}", err);
            }
        });
    }
}
