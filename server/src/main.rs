use ascii_mmorpg_server::{run_loopback_demo, Simulation};
use ascii_mmorpg_shared::codec::{read_frame, write_frame};
use ascii_mmorpg_shared::{ClientCommand, ServerEvent};
use std::io;
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("--tcp") {
        let address = args.get(2).map(String::as_str).unwrap_or("127.0.0.1:4000");
        run_tcp(address)
    } else {
        for event in run_loopback_demo()? {
            println!("{event:?}");
        }
        Ok(())
    }
}

fn run_tcp(address: &str) -> Result<(), Box<dyn std::error::Error>> {
    let listener = TcpListener::bind(address)?;
    println!("server listening on {address}");
    let (commands, command_rx) = mpsc::channel();
    let clients = Arc::new(Mutex::new(Vec::new()));
    let simulation_clients = Arc::clone(&clients);
    thread::spawn(move || simulation_loop(command_rx, simulation_clients));
    let next_client_id = AtomicU64::new(1);

    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                let client_id = next_client_id.fetch_add(1, Ordering::Relaxed);
                let client_commands = commands.clone();
                thread::spawn(move || {
                    if let Err(error) = serve_client(client_id, stream, client_commands) {
                        eprintln!("client {client_id} disconnected: {error}");
                    }
                });
            }
            Err(error) => eprintln!("accept failed: {error}"),
        }
    }
    Ok(())
}

type ClientSender = mpsc::Sender<ServerEvent>;
type ClientRegistry = Arc<Mutex<Vec<(u64, ClientSender)>>>;

enum ServerCommand {
    Connect {
        client_id: u64,
        sender: ClientSender,
    },
    Input {
        client_id: u64,
        command: ClientCommand,
    },
    Disconnect {
        client_id: u64,
    },
}

fn simulation_loop(command_rx: mpsc::Receiver<ServerCommand>, clients: ClientRegistry) {
    let mut simulation = Simulation::demo();
    while let Ok(message) = command_rx.recv() {
        match message {
            ServerCommand::Connect { client_id, sender } => {
                let snapshot = ServerEvent::Snapshot(simulation.snapshot_for(client_id));
                let _ = sender.send(snapshot);
                clients.lock().unwrap().push((client_id, sender));
            }
            ServerCommand::Input { client_id, command } => {
                simulation.fixed_tick_for(client_id, Some(command));
                let mut registry = clients.lock().unwrap();
                let mut disconnected = Vec::new();
                for (recipient_id, sender) in registry.iter() {
                    if sender
                        .send(ServerEvent::Snapshot(
                            simulation.snapshot_for(*recipient_id),
                        ))
                        .is_err()
                    {
                        disconnected.push(*recipient_id);
                    }
                }
                registry.retain(|(id, _)| !disconnected.contains(id));
            }
            ServerCommand::Disconnect { client_id } => {
                clients.lock().unwrap().retain(|(id, _)| *id != client_id);
            }
        }
    }
}

fn serve_client(
    client_id: u64,
    stream: TcpStream,
    commands: mpsc::Sender<ServerCommand>,
) -> Result<(), Box<dyn std::error::Error>> {
    let (events, event_rx) = mpsc::channel();
    let error_events = events.clone();
    events.send(ServerEvent::Welcome {
        protocol_version: ascii_mmorpg_shared::PROTOCOL_VERSION,
        player_id: client_id,
    })?;
    commands.send(ServerCommand::Connect {
        client_id,
        sender: events,
    })?;

    let mut writer = stream.try_clone()?;
    let writer_thread = thread::spawn(move || {
        for event in event_rx {
            if write_frame(&mut writer, &event).is_err() {
                break;
            }
        }
    });

    let mut reader = stream;
    loop {
        match read_frame::<_, ClientCommand>(&mut reader) {
            Ok(command) => commands.send(ServerCommand::Input { client_id, command })?,
            Err(ascii_mmorpg_shared::codec::CodecError::Io(error))
                if matches!(
                    error.kind(),
                    io::ErrorKind::UnexpectedEof
                        | io::ErrorKind::ConnectionReset
                        | io::ErrorKind::BrokenPipe
                ) =>
            {
                break
            }
            Err(error) => {
                let _ = error_events.send(ServerEvent::Error(error.to_string()));
                break;
            }
        }
    }
    commands.send(ServerCommand::Disconnect { client_id })?;
    let _ = writer_thread.join();
    Ok(())
}
