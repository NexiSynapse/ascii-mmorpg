use ascii_mmorpg_bot::{run, BotConfig};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let address = args
        .get(1)
        .cloned()
        .unwrap_or_else(|| "127.0.0.1:4000".into());
    let rounds = args
        .get(2)
        .and_then(|value| value.parse().ok())
        .unwrap_or(10);
    println!("bot connecting to {address} for {rounds} rounds");
    println!("completed {} rounds", run(BotConfig { address, rounds })?);
    Ok(())
}
