# shepherd-ipc

IPC layer for Shepherd.

## Overview

This crate provides the local inter-process communication infrastructure between the Shepherd service (`shepherdd`) and its clients (launcher UI, HUD overlay, admin tools). It includes:

- **Unix domain socket server** - Listens for client connections
- **NDJSON protocol** - Newline-delimited JSON message framing
- **Client management** - Connection tracking and cleanup
- **Peer authentication** - UID-based role assignment
- **Event broadcasting** - Push events to subscribed clients

## Architecture

```
┌─────────────────────────────────────────────────────────┐
│                      shepherdd                          │
│  ┌──────────────────────────────────────────────────┐   │
│  │                  IpcServer                       │   │
│  │  ┌──────────┐ ┌─────────┐ ┌─────────┐            │   │
│  │  │Client 1  │ │Client 2 │ │Client 3 │ ...        │   │
│  │  │(Launcher)│ │ (HUD)   │ │ (Admin) │            │   │
│  │  └────┬─────┘ └────┬────┘ └────┬────┘            │   │
│  │       │            │           │                 │   │
│  │       └────────────┴───────────┘                 │   │
│  │              Unix Domain Socket                  │   │
│  └──────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────┘
         │              │              │
    ┌────┴────┐    ┌────┴────┐    ┌────┴────┐
    │Launcher │    │   HUD   │    │  Admin  │
    │   UI    │    │ Overlay │    │  Tool   │
    └─────────┘    └─────────┘    └─────────┘
```

## Server Usage

### Starting the Server

```rust
use shepherd_ipc::IpcServer;

let mut server = IpcServer::new("/run/shepherdd/shepherdd.sock");
server.start().await?;

// Get message receiver for the main loop
let mut messages = server.take_message_receiver().await.unwrap();

// Accept connections in background
tokio::spawn(async move {
    server.run().await
});

// Process messages in main loop
while let Some(msg) = messages.recv().await {
    match msg {
        ServerMessage::Request { client_id, request } => {
            // Handle request, send response
            let response = handle_request(request);
            server.send_response(&client_id, response).await?;
        }
        ServerMessage::ClientConnected { client_id, info } => {
            println!("Client {} connected as {:?}", client_id, info.role);
        }
        ServerMessage::ClientDisconnected { client_id } => {
            println!("Client {} disconnected", client_id);
        }
    }
}
```

### Broadcasting Events

```rust
use shepherd_api::Event;

// Send to all subscribed clients
server.broadcast_event(Event::new(EventPayload::StateChanged(snapshot))).await;
```

### Client Roles

Clients are assigned roles based on their peer UID:

| UID | Role | Permissions |
|-----|------|-------------|
| root (0) | `Admin` | All commands |
| Service user | `Admin` | All commands |
| Other | `Shell` | Read + Launch/Stop |

```rust
// Role-based command filtering
match (request.command, client_info.role) {
    (Command::ReloadConfig, ClientRole::Admin) => { /* allowed */ }
    (Command::ReloadConfig, ClientRole::Shell) => { /* denied */ }
    (Command::Launch { .. }, _) => { /* allowed for all */ }
    // ...
}
```

## Client Usage

### Connecting

```rust
use shepherd_ipc::IpcClient;

let mut client = IpcClient::connect("/run/shepherdd/shepherdd.sock").await?;
```

### Sending Commands

```rust
use shepherd_api::{Command, Response};

// Request current state
client.send(Command::GetState).await?;
let response: Response = client.recv().await?;

// Launch an entry
client.send(Command::Launch { 
    entry_id: "minecraft".into() 
}).await?;
let response = client.recv().await?;
```

### Subscribing to Events

```rust
// Subscribe to event stream
client.send(Command::SubscribeEvents).await?;

// Receive events
loop {
    match client.recv_event().await {
        Ok(event) => {
            match event.payload {
                EventPayload::WarningIssued { remaining, .. } => {
                    println!("Warning: {} seconds remaining", remaining.as_secs());
                }
                EventPayload::SessionEnded { .. } => {
                    println!("Session ended");
                }
                _ => {}
            }
        }
        Err(IpcError::ConnectionClosed) => break,
        Err(e) => eprintln!("Error: {}", e),
    }
}
```

## Protocol

### Message Format

Messages use NDJSON (newline-delimited JSON):

```
{"type":"request","id":1,"command":"get_state"}\n
{"type":"response","id":1,"payload":{"api_version":1,...}}\n
{"type":"event","payload":{"type":"state_changed",...}}\n
```

### Request/Response

Each request has an ID, matched in the response:

```json
// Request
{"type":"request","id":42,"command":{"type":"launch","entry_id":"minecraft"}}

// Response
{"type":"response","id":42,"success":true,"payload":{...}}
```

### Events

Events are pushed without request IDs:

```json
{"type":"event","payload":{"type":"warning_issued","threshold":60,"remaining":{"secs":60}}}
```

## Socket Permissions

The socket is created with mode `0660`:
- Owner can read/write
- Group can read/write
- Others have no access

This allows the service to run as a dedicated user while permitting group members (e.g., `shepherd` group) to connect.

## Rate Limiting

Per-client rate limiting prevents buggy or malicious clients from overwhelming the service:

```rust
// Default: 10 commands per second per client
if rate_limiter.check(&client_id) {
    // Process command
} else {
    // Respond with rate limit error
}
```

## Error Handling

```rust
use shepherd_ipc::IpcError;

match result {
    Err(IpcError::ConnectionClosed) => {
        // Client disconnected
    }
    Err(IpcError::Json(e)) => {
        // Protocol error
    }
    Err(IpcError::Io(e)) => {
        // Socket error
    }
    _ => {}
}
```

## Dependencies

- `tokio` - Async runtime
- `serde` / `serde_json` - JSON serialization
- `nix` - Unix socket peer credentials
- `shepherd-api` - Message types
- `shepherd-util` - Client IDs
