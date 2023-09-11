# BITEWS

Simple proxy between a WebSocket and a TCP server. I made it to make
[Bite](https://github.com/alvivar/bite) compatible with the web.

## Environment variables

The server where the WebSocket server will be served (you will connect here):

    SERVER=127.0.0.1:1983

The BITE server that you will proxy to:

    PROXY=127.0.0.1:1984

## Running locally

Make sure the environment variables are set.

Run the server with **cargo run --release**. You will need a
[Bite](https://github.com/alvivar/bite) server around to proxy it.

Later on, you will be able to use **client.html** to test the WebSocket part
locally, right now it doesn't work because **BITE** has a custom protocol.

## Docker

Just run **docker-compose up -d --build** and it should work, as long you have
configured the PROXY variable (the BITE server you will proxy to).

Check out [**docker-compose.yml**](docker-compose.yml) for more details.
