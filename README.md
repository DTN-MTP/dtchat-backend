# DTChat Backend

A distributed chat application backend written in Rust that supports peer-to-peer communication over multiple network protocols (UDP, TCP, and Bundle Protocol).

## Dependencies

- `socket-engine`: Custom networking engine for handling UDP and TCP connections

## Features

- **Multi-protocol Support**: TCP, UDP, and Bundle Protocol (BP) communication
- **Peer-to-Peer Architecture**: Direct communication between chat clients
- **Message Acknowledgment**: Built-in message delivery confirmation system
- **Real-time Terminal Interface**: Live chat display with network and application events
- **Protocol Buffers**: Efficient message serialization using protobuf
- **Message Status Tracking**: Track message states (sending, sent, acknowledged, failed)
- **UUID-based Identification**: Unique identification for peers and messages

## Architecture

The application consists of several key components:

- **ChatModel**: Core chat logic and peer management
- **TerminalScreen**: Interactive terminal-based user interface
- **Message System**: Handles chat messages with status tracking
- **Event System**: Network and application event management
- **Protocol Buffer Messages**: Structured message format for network communication

## Usage

### Basic Setup

The application requires two arguments:
1. **Local endpoint**: The address this instance will listen on
2. **Remote endpoint**: The address of the peer to connect to

### Quick Test with UDP

To test the chat functionality using UDP/BP/TCP protocol:

```bash
# Terminal 1 (First peer)
cargo run -- "<PROTOCOL> 127.0.0.1:7777" "<PROTOCOL> 127.0.0.1:6666"

# Terminal 2 (Second peer)
cargo run -- "<PROTOCOL> 127.0.0.1:6666" "<PROTOCOL> 127.0.0.1:7777"
```

### Supported Protocols

- **UDP**: `udp <ip>:<port>`
- **TCP**: `tcp <ip>:<port>`
- **Bundle Protocol**: `bp <address>`


## Message Protocol

Messages use Protocol Buffers with the following structure:

```protobuf
message ProtoMessage {
  string uuid = 1;
  string sender_uuid = 2;
  int64 timestamp = 3;
  string room_uuid = 4;
  
  oneof msg_type {
    TextMessage text = 5;
    AckMessage ack = 6;
  }
}
```

### Message Types

- **TextMessage**: Regular chat messages containing text content
- **AckMessage**: Acknowledgment messages confirming receipt

## Terminal Interface

The application provides a real-time terminal interface displaying:

- **Chat Messages**: Recent messages with timestamps and status indicators
- **Network Events**: Connection status, data transmission events
- **Application Events**: Internal application state changes
- **Message Status**: Visual indicators for message delivery states

### Message Status Indicators

- `SENDING`: Message is being transmitted
- `SENT`: Message has been sent successfully
- `ACKED`: Message has been acknowledged by recipient
- `FAILED`: Message transmission failed
- `RECEIVED`: Message received from peer
