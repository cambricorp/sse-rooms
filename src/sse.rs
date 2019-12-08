//! An SSE Responder.
//!
//! This module might be suitable for inclusion in rocket_contrib.

use std::future::Future;

use rocket::request::Request;
use rocket::response::{Responder, Response, ResultFuture};
use tokio::io::{BufWriter, AsyncWrite, AsyncWriteExt};

use super::io_channel::{io_channel, IoChannelReader, IoChannelWriter};

#[derive(Clone, Debug)]
pub struct Event {
    event: Option<String>,
    data: Option<String>,
    id: Option<String>,
    // retry field?
}

impl Event {
    /// Create a new Event with event, data, and id all (optionally) specified
    pub fn new(event: Option<String>, data: Option<String>, id: Option<String>) -> Option<Self> {
        if event.as_ref().map_or(false, |e| e.find(|b| b == '\r' || b == '\n').is_some()) {
            return None;
        }

        if id.as_ref().map_or(false, |i| i.find(|b| b == '\r' || b == '\n').is_some()) {
            return None;
        }

        Some(Self { event, id, data })
    }

    /// Writes this event to a `writer` according in the EventStream format
    // TODO: Remove Unpin bound?
    pub async fn write_to<W: AsyncWrite + Unpin>(self, mut writer: W) -> Result<(), std::io::Error> {
        if let Some(event) = self.event {
            writer.write_all(b"event: ").await?;
            writer.write_all(event.as_bytes()).await?;
            writer.write_all(b"\n").await?;
        }
        if let Some(id) = self.id {
            writer.write_all(b"id: ").await?;
            writer.write_all(id.as_bytes()).await?;
            writer.write_all(b"\n").await?;
        }
        if let Some(data) = self.data {
            for line in data.lines() {
                writer.write_all(b"data: ").await?;
                writer.write_all(line.as_bytes()).await?;
                writer.write_all(b"\n").await?;
            }
        }
        writer.write_all(b"\n").await?;
        Ok(())
    }
}

/// The 'read half' of an SSE stream. This type implements `Responder`; see the
/// [`with_writer`] function for a usage example.
pub struct SSE(IoChannelReader);

/// The 'send half' of an SSE stream. You can use the [`SSEWriter::send`] method
/// to send events to the stream
pub struct SSEWriter(BufWriter<IoChannelWriter>);

impl SSEWriter {
    /// Sends the `event` to the connected client
    pub async fn send(&mut self, event: Event) -> Result<(), std::io::Error> {
        event.write_to(&mut self.0).await?;
        self.0.flush().await?;
        Ok(())
    }
}

impl<'r> Responder<'r> for SSE {
    fn respond_to(self, _req: &'r Request<'_>) -> ResultFuture<'r> {
        Box::pin(async move {
            Response::build()
                .raw_header("Content-Type", "text/event-stream")
                .streamed_body(self.0)
                .ok()
        })
    }
}

pub fn with_writer<F, Fut>(func: F) -> SSE
where
    F: FnOnce(SSEWriter) -> Fut,
    Fut: Future<Output=()> + Send + 'static,
{
    let (tx, rx) = io_channel();
    tokio::spawn(func(SSEWriter(BufWriter::new(tx))));
    SSE(rx)
}

// TODO: Consider an SSEStream that wraps an Stream<Item=Event>.
// Users would probably need to use something like async_stream, and the
// AsyncRead impl would probably have to be a pretty complex state machine