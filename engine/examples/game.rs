//! Windowed 2.5D port of the terminal game: the same cast as the client's
//! `--demo` (adventurer, mossling, skeleton, herbalist, sword) presented from
//! an engine scene built by [`bridge`](ascii_mmorpg_engine::bridge).
//!
//! Run: `cargo run -p ascii-mmorpg-engine --features present --example game`
//! Flags: `--frames N` exits after N presented frames (automated proof);
//! `--dungeon` delves the sunken-vault preset instead of the meadow.
//!
//! The demo is interactive: movement keys move the player and F/R update the
//! overlay log. Use F11 to toggle fullscreen and Q/Esc to quit.

#[cfg(not(feature = "present"))]
fn main() {
    eprintln!("this example needs `--features present` (winit + wgpu + fontdue)");
    std::process::exit(2);
}

#[cfg(feature = "present")]
fn main() {
    use ascii_mmorpg_engine::bridge::{self, HUD_H, LOG_H, VIEW_H, VIEW_W};
    use ascii_mmorpg_engine::motion::KinematicMotion;
    use ascii_mmorpg_engine::present::{PresentOptions, SceneSource};
    use std::collections::HashSet;
    use winit::keyboard::KeyCode;

    let args: Vec<String> = std::env::args().collect();
    let frames = args
        .iter()
        .position(|arg| arg == "--frames")
        .and_then(|i| args.get(i + 1))
        .and_then(|v| v.parse::<u64>().ok());

    let view = if args.iter().any(|arg| arg == "--dungeon") {
        bridge::dungeon_view()
    } else {
        bridge::demo_view()
    };
    struct Demo {
        view: bridge::SceneView,
        held: HashSet<KeyCode>,
        last_t: f32,
        motion: KinematicMotion,
        cache: bridge::StaticLayers,
        follow: Option<(f32, f32)>,
    }

    impl SceneSource for Demo {
        fn scene_at(
            &mut self,
            t: f32,
        ) -> (ascii_mmorpg_engine::Scene, ascii_mmorpg_engine::Camera) {
            self.scene_at_size(t, VIEW_W as u32, (HUD_H + VIEW_H + LOG_H) as u32)
        }

        fn scene_at_size(
            &mut self,
            t: f32,
            width: u32,
            height: u32,
        ) -> (ascii_mmorpg_engine::Scene, ascii_mmorpg_engine::Camera) {
            let dt = (t - self.last_t).clamp(0.0, 0.1);
            self.last_t = t;
            let dx = (self.held.contains(&KeyCode::KeyD) as i32)
                - (self.held.contains(&KeyCode::KeyA) as i32);
            let dy = (self.held.contains(&KeyCode::KeyS) as i32)
                - (self.held.contains(&KeyCode::KeyW) as i32);
            if let Some((step_x, step_y)) = self.motion.update((dx as f32, dy as f32), dt) {
                self.view.player.x += i32::from(step_x);
                self.view.player.y += i32::from(step_y);
            }
            self.view.player.render_offset = self.motion.remainder();
            self.view.player.motion_speed =
                self.motion.velocity().0.hypot(self.motion.velocity().1);
            if dx != 0 {
                self.view.player.facing_x = dx as i8;
            }
            self.view.tick = (t * 20.0) as u64;
            let (scene, bcam) = bridge::scene_from_view_sized_cached(
                &self.view,
                width as i32,
                height as i32,
                &mut self.cache,
            );
            // Exponential follow toward the bridge camera: the world glides
            // instead of stepping cell to cell.
            let (next, cam) = ascii_mmorpg_engine::present::glide_camera(
                self.follow,
                bcam.x as f32 + self.view.player.render_offset.0,
                bcam.y as f32 + self.view.player.render_offset.1,
                dt,
            );
            self.follow = Some(next);
            (scene, cam)
        }

        fn key(&mut self, code: KeyCode, pressed: bool) {
            match code {
                KeyCode::KeyW | KeyCode::KeyA | KeyCode::KeyS | KeyCode::KeyD => {
                    if pressed {
                        self.held.insert(code);
                    } else {
                        self.held.remove(&code);
                    }
                }
                KeyCode::KeyF if pressed => self
                    .view
                    .messages
                    .push("Attack — the blade flashes with colour.".into()),
                KeyCode::KeyR if pressed => self
                    .view
                    .messages
                    .push("Rest — aura settles around the hero.".into()),
                _ => {}
            }
            self.view.messages.truncate(8);
        }
    }

    match ascii_mmorpg_engine::present::run_dynamic(
        Demo {
            view,
            held: HashSet::new(),
            last_t: 0.0,
            motion: KinematicMotion::default(),
            cache: bridge::StaticLayers::new(),
            follow: None,
        },
        PresentOptions {
            title: "ASCII MMORPG — windowed 2.5D port".into(),
            cell_px: 8.0,
            max_frames: frames,
            ..Default::default()
        },
    ) {
        Ok(presented) => {
            eprintln!("presented {presented} frames");
            std::process::exit(0);
        }
        Err(err) => {
            eprintln!("presenter failed: {err}");
            std::process::exit(1);
        }
    }
}
