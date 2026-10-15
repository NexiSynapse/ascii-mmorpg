use ascii_mmorpg_tools::validate_alpha_content;
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let root = PathBuf::from(args.get(1).cloned().unwrap_or_else(|| "content".into()));
    let count = validate_alpha_content(&root)?;
    println!("content ok: {count} definitions");
    Ok(())
}
