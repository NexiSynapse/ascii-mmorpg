use ascii_mmorpg_shared::codec::{read_frame, write_frame};
use ascii_mmorpg_shared::{ClientCommand, Command, Direction, ServerEvent};
use std::net::TcpStream;

#[derive(Clone, Debug)]
pub struct BotConfig {
    pub address: String,
    pub rounds: u32,
}

pub fn run(config: BotConfig) -> Result<u32, Box<dyn std::error::Error>> {
    let mut stream = TcpStream::connect(&config.address)?;
    let welcome: ServerEvent = read_frame(&mut stream)?;
    if !matches!(welcome, ServerEvent::Welcome { .. }) {
        return Err("server did not send welcome".into());
    }
    let commands = [
        Command::Move(Direction::East),
        Command::Attack,
        Command::Rest,
    ];
    for tick in 0..config.rounds {
        write_frame(
            &mut stream,
            &ClientCommand::new(
                u64::from(tick),
                commands[(tick as usize) % commands.len()].clone(),
            ),
        )?;
        let _event: ServerEvent = read_frame(&mut stream)?;
    }
    Ok(config.rounds)
}

pub fn run_once_loopback() -> Result<ServerEvent, ascii_mmorpg_shared::codec::CodecError> {
    use std::io::Cursor;
    let command = ClientCommand::new(0, Command::Rest);
    let mut bytes = Vec::new();
    write_frame(&mut bytes, &command)?;
    let _: ClientCommand = read_frame(&mut Cursor::new(bytes))?;
    Ok(ServerEvent::Notice("bot command generated".into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn loopback_bot_generates_a_valid_event() {
        assert!(matches!(
            run_once_loopback().unwrap(),
            ServerEvent::Notice(_)
        ));
    }
}
