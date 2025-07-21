# DTChat Backend

A distributed chat application backend written in Rust that supports peer-to-peer communication over multiple network protocols (UDP, TCP, and Bundle Protocol).

## Dependencies

- `socket-engine`: Custom networking engine for handling UDP and TCP connections
- `clap`: Command-line argument parsing

## Features

- **Multi-protocol Support**: TCP, UDP, and Bundle Protocol (BP) communication
- **Peer-to-Peer Architecture**: Direct communication between chat clients
- **Message Acknowledgment**: Built-in message delivery confirmation system
- **Real-time Terminal Interface**: Live chat display with network and application events
- **Protocol Buffers**: Efficient message serialization using protobuf
- **Message Status Tracking**: Track message states (sending, sent, acknowledged, failed)
- **UUID-based Identification**: Unique identification for peers and messages
- **Command-line Interface**: User-friendly argument parsing with clap

## Building

### Library
To build the library only:
```bash
cargo build
```

### Examples
To build and run the terminal chat example:
```bash
cargo run --example dtchat_tui -- --help
```

## Architecture

The application consists of several key components:

- **ChatModel**: Core chat logic and peer management
- **TerminalScreen**: Interactive terminal-based user interface
- **Message System**: Handles chat messages with status tracking
- **Event System**: Network and application event management
- **Protocol Buffer Messages**: Structured message format for network communication

## Usage

### Running the Terminal Chat Example

The application is available as an example with command-line argument parsing using `clap`. The terminal chat example provides an interactive interface for peer-to-peer communication.

#### Command Line Help

To see all available options:

```bash
cargo run --example dtchat_tui -- --help
```

#### Command Line Options

- `--protocol, -p`: Protocol to use (udp, tcp, bp) - **Required**
- `--local-port, -l`: Local port for listening (for UDP/TCP)
- `--dist-port, -d`: Distant port for sending (for UDP/TCP)
- `--ip, -i`: IP address (default: 127.0.0.1, for UDP/TCP)
- `--local-addr, -L`: Local address (for Bundle Protocol)
- `--dist-addr, -D`: Distant address (for Bundle Protocol)
- `--view-height, -v`: Terminal display height (default: 10)

### Quick Test with Different Protocols

To test the chat functionality using various protocols:

#### UDP Protocol
```bash
# Terminal 1 (listening on port 8080, sending to 8081)
cargo run --example dtchat_tui -- --protocol udp --local-port 8080 --dist-port 8081

# Terminal 2 (listening on port 8081, sending to 8080)
cargo run --example dtchat_tui -- --protocol udp --local-port 8081 --dist-port 8080
```

#### TCP Protocol
```bash
# Terminal 1 (listening on port 8080, sending to 8081)
cargo run --example dtchat_tui -- --protocol tcp --local-port 8080 --dist-port 8081

# Terminal 2 (listening on port 8081, sending to 8080)
cargo run --example dtchat_tui -- --protocol tcp --local-port 8081 --dist-port 8080
```

#### Bundle Protocol
```bash
# Terminal 1
cargo run --example dtchat_tui -- --protocol bp --local-addr node1 --dist-addr node2

# Terminal 2
cargo run --example dtchat_tui -- --protocol bp --local-addr node2 --dist-addr node1
```

#### With Custom View Height and Different IP
```bash
cargo run --example dtchat_tui -- --protocol udp --local-port 8080 --dist-port 8081 --ip 192.168.1.100 --view-height 20
```

### Supported Protocols

- **UDP**: Requires `--local-port` and `--dist-port` options
- **TCP**: Requires `--local-port` and `--dist-port` options  
- **Bundle Protocol**: Requires `--local-addr` and `--dist-addr` options

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

### Usage in the Terminal

- Type your message and press Enter to send
- Type `quit` or `exit` to close the application
- The interface shows real-time updates for all network and application events

## Examples Directory

The `examples/` directory contains:

- **dtchat_tui.rs**: Interactive terminal-based chat client demonstrating the library usage with support for all three protocols (UDP, TCP, BP)
