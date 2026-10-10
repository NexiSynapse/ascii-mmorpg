//! TCP client transport for the shared postcard protocol.
//!
//! The reader runs independently from the keyboard loop so authoritative
//! snapshots can be rendered even while the user is not pressing a key.

use ascii_mmorpg_shared::codec::{read_frame, write_frame, CodecError};
use ascii_mmorpg_shared::{ClientCommand, ServerEvent};
use std::io;
use std::net::{Shutdown, TcpStream};
use std::sync::mpsc::{self, Receiver};
use std::thread::{self, JoinHandle};

#[derive(Debug)]
pub enum ConnectionEvent {
    Server(ServerEvent),
    Eof,
    Error(String),
}

/// A postcard-framed TCP connection to the game server.
pub struct ClientConnection {
    writer: Option<TcpStream>,
    events: Receiver<ConnectionEvent>,
    reader: Option<JoinHandle<()>>,
}

impl ClientConnection {
    pub fn connect(address: &str) -> io::Result<Self> {
        let stream = TcpStream::connect(address)?;
        stream.set_nodelay(true)?;
        let reader_stream = stream.try_clone()?;
        let (sender, events) = mpsc::channel();
        let reader = thread::spawn(move || reader_loop(reader_stream, sender));

        Ok(Self {
            writer: Some(stream),
            events,
            reader: Some(reader),
        })
    }

    /// Send a command as soon as it is produced by the keyboard loop.
    pub fn send_command(&mut self, command: &ClientCommand) -> Result<(), CodecError> {
        let writer = self.writer.as_mut().ok_or_else(|| {
            CodecError::Io(io::Error::new(
                io::ErrorKind::NotConnected,
                "connection is closed",
            ))
        })?;
        write_frame(writer, command)
    }

    pub fn try_recv(&self) -> Option<ConnectionEvent> {
        self.events.try_recv().ok()
    }

    /// Close both halves and wait for the reader thread to observe EOF.
    pub fn close(&mut self) {
        if let Some(writer) = self.writer.take() {
            let _ = writer.shutdown(Shutdown::Both);
        }
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }
}

impl Drop for ClientConnection {
    fn drop(&mut self) {
        self.close();
    }
}

fn reader_loop(mut stream: TcpStream, sender: mpsc::Sender<ConnectionEvent>) {
    loop {
        match read_frame::<_, ServerEvent>(&mut stream) {
            Ok(event) => {
                if sender.send(ConnectionEvent::Server(event)).is_err() {
                    break;
                }
            }
            Err(CodecError::Io(error))
                if matches!(
                    error.kind(),
                    io::ErrorKind::UnexpectedEof
                        | io::ErrorKind::ConnectionReset
                        | io::ErrorKind::BrokenPipe
                ) =>
            {
                let _ = sender.send(ConnectionEvent::Eof);
                break;
            }
            Err(error) => {
                let _ = sender.send(ConnectionEvent::Error(error.to_string()));
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ascii_mmorpg_shared::codec::write_frame;
    use ascii_mmorpg_shared::PROTOCOL_VERSION;
    use std::net::TcpListener;
    use std::thread;

    #[test]
    fn receives_welcome_then_reports_eof() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            write_frame(
                &mut stream,
                &ServerEvent::Welcome {
                    protocol_version: PROTOCOL_VERSION,
                    player_id: 1,
                },
            )
            .unwrap();
        });

        let mut client = ClientConnection::connect(&address.to_string()).unwrap();
        let welcome = loop {
            if let Some(event) = client.try_recv() {
                break event;
            }
            thread::yield_now();
        };
        assert!(matches!(
            welcome,
            ConnectionEvent::Server(ServerEvent::Welcome { .. })
        ));
        let eof = loop {
            if let Some(event) = client.try_recv() {
                break event;
            }
            thread::yield_now();
        };
        assert!(matches!(eof, ConnectionEvent::Eof));
        client.close();
        server.join().unwrap();
    }
}
