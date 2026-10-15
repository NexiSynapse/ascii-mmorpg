use ascii_mmorpg_server::{run_loopback_demo, Simulation};
use ascii_mmorpg_shared::codec::{read_frame, write_frame};
use ascii_mmorpg_shared::{ClientCommand, ServerEvent};
use std::io;
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::Duration;

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
    use std::collections::VecDeque;
    let mut simulation = Simulation::demo();
    // Strict 20 Hz scheduler: inputs queue here and are drained at tick
    // boundaries (one tick per 50 ms). Inputs never advance a tick directly,
    // so burst input cannot exceed 20 Hz. Cap bounds memory under flooding.
    const MAX_PENDING: usize = 512;
    let mut pending: VecDeque<(u64, ClientCommand)> = VecDeque::new();

    // Drains the queue into a single tick and broadcasts the result.
    let flush_tick = |simulation: &mut Simulation,
                      pending: &mut VecDeque<(u64, ClientCommand)>,
                      clients: &ClientRegistry| {
        if pending.is_empty() {
            simulation.advance_tick();
        } else {
            let queued: Vec<(u64, ClientCommand)> = pending.drain(..).collect();
            simulation.advance_tick_with(queued);
        }
        broadcast_snapshots(simulation, clients);
    };

    loop {
        let message = match command_rx.recv_timeout(Duration::from_millis(50)) {
            Ok(message) => message,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                flush_tick(&mut simulation, &mut pending, &clients);
                continue;
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        };
        match message {
            ServerCommand::Connect { client_id, sender } => {
                let snapshot = ServerEvent::Snapshot(simulation.connect_player(client_id));
                let _ = sender.send(snapshot);
                clients.lock().unwrap().push((client_id, sender));
            }
            ServerCommand::Input { client_id, command } => {
                // Protocol errors are reported immediately without ticking or queueing.
                if command.protocol_version != ascii_mmorpg_shared::PROTOCOL_VERSION {
                    if let Some((_, sender)) = clients
                        .lock()
                        .unwrap()
                        .iter()
                        .find(|(id, _)| *id == client_id)
                    {
                        let _ = sender.send(ServerEvent::Error("protocol version mismatch".into()));
                    }
                    continue;
                }
                if pending.len() >= MAX_PENDING {
                    pending.pop_front();
                }
                pending.push_back((client_id, command));
            }
            ServerCommand::Disconnect { client_id } => {
                clients.lock().unwrap().retain(|(id, _)| *id != client_id);
                pending.retain(|(id, _)| *id != client_id);
            }
        }
    }
}

fn broadcast_snapshots(simulation: &Simulation, clients: &ClientRegistry) {
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
    drop(error_events);
    let _ = writer_thread.join();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ascii_mmorpg_shared::{Command, ServerEvent};

    #[test]
    fn tcp_loop_advances_idle_and_input_ticks_separately() {
        let (commands, command_rx) = mpsc::channel();
        let clients = Arc::new(Mutex::new(Vec::new()));
        let (events, event_rx) = mpsc::channel();
        let loop_clients = Arc::clone(&clients);
        let worker = thread::spawn(move || simulation_loop(command_rx, loop_clients));

        commands
            .send(ServerCommand::Connect {
                client_id: 1,
                sender: events,
            })
            .unwrap();
        let initial = event_rx.recv_timeout(Duration::from_millis(200)).unwrap();
        assert!(matches!(initial, ServerEvent::Snapshot(snapshot) if snapshot.tick == 0));

        let idle = event_rx.recv_timeout(Duration::from_millis(200)).unwrap();
        let ServerEvent::Snapshot(idle_snapshot) = idle else {
            panic!("expected idle snapshot")
        };
        assert_eq!(idle_snapshot.tick, 1);

        // Strict scheduler: queued input applies at the next 50 ms tick
        // boundary, not immediately on receipt.
        commands
            .send(ServerCommand::Input {
                client_id: 1,
                command: ClientCommand::new(0, Command::Rest),
            })
            .unwrap();
        let input = event_rx.recv_timeout(Duration::from_millis(500)).unwrap();
        let ServerEvent::Snapshot(input_snapshot) = input else {
            panic!("expected input snapshot")
        };
        assert_eq!(input_snapshot.tick, 2);
        assert!(
            input_snapshot
                .messages
                .iter()
                .any(|message| message.contains("catch your breath")),
            "queued Rest must apply inside tick 2"
        );

        drop(commands);
        worker.join().unwrap();
    }

    #[test]
    fn tcp_loop_coalesces_burst_input_into_one_tick() {
        let (commands, command_rx) = mpsc::channel();
        let clients = Arc::new(Mutex::new(Vec::new()));
        let (events, event_rx) = mpsc::channel();
        let loop_clients = Arc::clone(&clients);
        let worker = thread::spawn(move || simulation_loop(command_rx, loop_clients));

        commands
            .send(ServerCommand::Connect {
                client_id: 7,
                sender: events,
            })
            .unwrap();
        let _ = event_rx.recv_timeout(Duration::from_millis(200)).unwrap();
        let _ = event_rx.recv_timeout(Duration::from_millis(200)).unwrap();

        // Three rapid inputs must not produce three ticks under the strict
        // scheduler; they drain inside the next single tick.
        for _ in 0..3 {
            commands
                .send(ServerCommand::Input {
                    client_id: 7,
                    command: ClientCommand::new(0, Command::Rest),
                })
                .unwrap();
        }
        let burst = event_rx.recv_timeout(Duration::from_millis(500)).unwrap();
        let ServerEvent::Snapshot(burst_snapshot) = burst else {
            panic!("expected burst snapshot")
        };
        assert_eq!(burst_snapshot.tick, 2);

        drop(commands);
        worker.join().unwrap();
    }

    #[test]
    fn tcp_loop_rejects_protocol_mismatch_without_tick_or_queue() {
        let (commands, command_rx) = mpsc::channel();
        let clients = Arc::new(Mutex::new(Vec::new()));
        let (events, event_rx) = mpsc::channel();
        let loop_clients = Arc::clone(&clients);
        let worker = thread::spawn(move || simulation_loop(command_rx, loop_clients));

        commands
            .send(ServerCommand::Connect {
                client_id: 9,
                sender: events,
            })
            .unwrap();
        let _ = event_rx.recv_timeout(Duration::from_millis(200)).unwrap();
        let _ = event_rx.recv_timeout(Duration::from_millis(200)).unwrap();

        commands
            .send(ServerCommand::Input {
                client_id: 9,
                command: ClientCommand {
                    protocol_version: ascii_mmorpg_shared::PROTOCOL_VERSION + 1,
                    client_tick: 0,
                    command: Command::Rest,
                },
            })
            .unwrap();
        let error = event_rx.recv_timeout(Duration::from_millis(200)).unwrap();
        assert!(matches!(error, ServerEvent::Error(_)));
        // Next tick must be idle tick 2 with no queued Rest applied.
        let next = event_rx.recv_timeout(Duration::from_millis(500)).unwrap();
        let ServerEvent::Snapshot(next_snapshot) = next else {
            panic!("expected snapshot after error")
        };
        assert_eq!(next_snapshot.tick, 2);
        assert!(
            !next_snapshot
                .messages
                .iter()
                .any(|message| message.contains("catch your breath")),
            "rejected command must not queue"
        );

        drop(commands);
        worker.join().unwrap();
    }
}
