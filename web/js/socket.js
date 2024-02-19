import { PubSub } from "./pubsub.js";

let socket = null;
let lastAddress = "";
let transmisionComplete = true;

const maxReconnectionAttempts = 30;
const maxReconnectionDelay = 60000; // 1 minute
const baseReconnectionDelay = 3000; // 1 second

let reconnectionAttempts = 0;
let reconnectionDelay = baseReconnectionDelay;

const SocketEvent = {
    connected: "socket.connected",
    partialMessage: "socket.partialMessage",
    completeMessage: "socket.completeMessage",
    reconnection: "socket.reconnection",
    error: "socket.error",
};

const connect = (address) => {
    socket = new WebSocket(address);
    lastAddress = address;

    socket.addEventListener("open", () => {
        PubSub.publish(SocketEvent.connected, true);

        if (reconnectionAttempts > 0) {
            // @todo We may need to make changes to the code and want to reload
            // it. However, this should not be included in the production
            // version. We should find a way to detect an environment variable
            // for development mode instead.
            location.reload();
        }

        reconnectionAttempts = 0;
        reconnectionDelay = baseReconnectionDelay;
    });

    socket.addEventListener("message", (event) => {
        if (event.data === "\0") {
            transmisionComplete = true;
            return;
        }

        if (transmisionComplete) {
            PubSub.publish(SocketEvent.completeMessage, event.data);
            transmisionComplete = false;
        } else {
            PubSub.publish(SocketEvent.partialMessage, event.data);
        }
    });

    socket.addEventListener("close", () => {
        if (reconnectionAttempts < maxReconnectionAttempts) {
            setTimeout(() => {
                connect(lastAddress);
            }, reconnectionDelay);

            PubSub.publish(SocketEvent.reconnection, reconnectionDelay);

            reconnectionAttempts += 1;
            reconnectionDelay = Math.min(
                maxReconnectionDelay,
                reconnectionDelay * 1.5
            );
        }
    });

    socket.addEventListener("error", (event) => {
        PubSub.publish(SocketEvent.error, event);
        console.debug(`Error: ${event}`);
    });
};

const send = (message) => {
    if (socket.readyState === WebSocket.OPEN) {
        socket.send(message);
    }
};

const canReconnect = () => {
    return reconnectionAttempts < maxReconnectionAttempts;
};

export { connect, send, canReconnect, SocketEvent };
