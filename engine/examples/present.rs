//! Windowed proof of the engine: opens a real OS window and presents
//! [`wave_scene`](ascii_mmorpg_engine::demo::wave_scene) at 60 Hz.
//!
//! Run: `cargo run -p ascii-mmorpg-engine --features present --example present`
//! Flags: `--frames N` exits after N presented frames (automated proof).

#[cfg(not(feature = "present"))]
fn main() {
    eprintln!("this example needs `--features present` (winit + wgpu + fontdue)");
    std::process::exit(2);
}

#[cfg(feature = "present")]
fn main() {
    use ascii_mmorpg_engine::{demo, present::PresentOptions};

    let args: Vec<String> = std::env::args().collect();
    let frames = args
        .iter()
        .position(|arg| arg == "--frames")
        .and_then(|i| args.get(i + 1))
        .and_then(|v| v.parse::<u64>().ok());

    let (scene, camera) = demo::wave_scene();
    match ascii_mmorpg_engine::present::run(
        scene,
        camera,
        PresentOptions {
            title: "ASCII Engine — wave".into(),
            max_frames: frames,
            ..Default::default()
        },
    ) {
        Ok(presented) => {
            // NOTE: exit without running destructors. Teardown of the wgpu
            // GLES backend segfaults inside mesa's eglTerminate on some
            // systems (gdb: wgpu_hal::gles Drop at process exit); the OS
            // reclaims everything, so skipping it is safe here.
            eprintln!("presented {presented} frames");
            std::process::exit(0);
        }
        Err(err) => {
            eprintln!("presenter failed: {err}");
            std::process::exit(1);
        }
    }
}
