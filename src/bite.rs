use std::net::TcpStream;

pub struct Bite {
    pub id: usize,
    pub belong_id: usize,
    pub socket: TcpStream,
    pub received: Vec<Vec<u8>>,
    pub to_write: Vec<Vec<u8>>,
    pub closed: bool,
}

impl Bite {
    pub fn new(id: usize, belong_id: usize, ip: &str) -> Bite {
        let socket = TcpStream::connect(ip).unwrap();
        socket.set_nonblocking(true).unwrap();
        let received = Vec::<Vec<u8>>::new();
        let to_write = Vec::<Vec<u8>>::new();

        Bite {
            id,
            belong_id,
            socket,
            received,
            to_write,
            closed: false,
        }
    }
}
