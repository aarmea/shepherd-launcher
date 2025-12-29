//! IPC client implementation

use shepherd_api::{Command, Event, Request, Response, ResponseResult};
use std::path::Path;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;

use crate::{IpcError, IpcResult};

/// IPC Client for connecting to shepherdd
pub struct IpcClient {
    reader: BufReader<tokio::net::unix::OwnedReadHalf>,
    writer: tokio::net::unix::OwnedWriteHalf,
    next_request_id: u64,
}

impl IpcClient {
    /// Connect to shepherdd
    pub async fn connect(socket_path: impl AsRef<Path>) -> IpcResult<Self> {
        let stream = UnixStream::connect(socket_path).await?;
        let (read_half, write_half) = stream.into_split();

        Ok(Self {
            reader: BufReader::new(read_half),
            writer: write_half,
            next_request_id: 1,
        })
    }

    /// Send a command and wait for response
    pub async fn send(&mut self, command: Command) -> IpcResult<Response> {
        let request_id = self.next_request_id;
        self.next_request_id += 1;

        let request = Request::new(request_id, command);
        let mut json = serde_json::to_string(&request)?;
        json.push('\n');

        self.writer.write_all(json.as_bytes()).await?;

        // Read response
        let mut line = String::new();
        let n = self.reader.read_line(&mut line).await?;
        if n == 0 {
            return Err(IpcError::ConnectionClosed);
        }

        let response: Response = serde_json::from_str(line.trim())?;

        Ok(response)
    }

    /// Subscribe to events and consume this client to return an event stream
    pub async fn subscribe(mut self) -> IpcResult<EventStream> {
        let response = self.send(Command::SubscribeEvents).await?;

        match response.result {
            ResponseResult::Ok(_) => {}
            ResponseResult::Err(e) => {
                return Err(IpcError::ServerError(e.message));
            }
        }

        Ok(EventStream {
            reader: self.reader,
        })
    }
}

/// Stream of events from shepherdd
pub struct EventStream {
    reader: BufReader<tokio::net::unix::OwnedReadHalf>,
}

impl EventStream {
    /// Wait for the next event
    pub async fn next(&mut self) -> IpcResult<Event> {
        let mut line = String::new();
        let n = self.reader.read_line(&mut line).await?;
        if n == 0 {
            return Err(IpcError::ConnectionClosed);
        }

        let event: Event = serde_json::from_str(line.trim())?;
        Ok(event)
    }
}

#[cfg(test)]
mod tests {
    // Client tests would require a running server
    // See integration tests
}
