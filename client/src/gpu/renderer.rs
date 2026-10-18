//! High-level GPU offscreen renderer for the ASCII MMORPG client
//! with multi-buffered asynchronous staging rings and hardware instanced batching.

use std::collections::{HashMap, HashSet};
use std::time::Instant;

use ascii_mmorpg_shared::collision::terrain_seed;
use ascii_mmorpg_shared::{in_melee_range, EntityId, EntityKind, Point, WorldSnapshot};
use glam::{Mat4, Vec2, Vec3};

use super::context::GpuContext;
use super::mesh::{Frustum, MeshBuffer, SceneBatches};
use super::pipeline::{GpuCell, GpuPipeline};
use super::uniforms::{CameraUniforms, GpuPointLight, LightingUniforms};
use crate::camera::CameraState;
use crate::renderer::WeatherMode;

pub struct GpuRenderer {
    pub ctx: GpuContext,
    pub pipeline: GpuPipeline,
    pub mesh: MeshBuffer,
    pub batches: SceneBatches,
    /// Display (terminal) grid in cells. Used only for the composed HUD frame
    /// dimensions; the GPU renders the scene partition natively so the blit
    /// fills it exactly (see `scene_partition_dims`).
    pub width: i16,
    pub height: i16,
    /// Render grid in cells: the scene partition inside the composed HUD
    /// frame (`scene_partition_dims` of `width` x `height`). Rendering the
    /// full terminal grid instead would be top-left cropped by the HUD blit,
    /// breaking the scene's fill and aspect. SSAA stays 4x at every grid
    /// size (see `adaptive_ssaa` for why the 2x tier had to go).
    pub scene_width: i16,
    pub scene_height: i16,
    pub weather: WeatherMode,
    pub active_modal: crate::ui::ActiveModal,
    pub last_player_pos: Option<Point>,
    actor_walk_states: HashMap<EntityId, ActorWalkState>,
    pub player_walk_phase: f32,
    pub smooth_cam_pos: Option<Vec2>,
    pub smooth_player_pos: Option<Vec2>,
    animation_started: Instant,
    last_render_instant: Option<Instant>,
    pub camera: CameraState,
    /// Combat FX (video-tools REPORT #2 impact bursts + #3 telegraphs),
    /// spawned from combat messages the renderer observes in the snapshot.
    /// Presentation only — positions/hues come from real game state, and
    /// server authority is never touched.
    combat_fx: Vec<CombatFx>,
    seen_combat_events: Vec<String>,
    attack_started_at: HashMap<EntityId, f32>,
}

/// Reserved entity id stamped on FX geometry so compute-pass gates treat
/// the quads as entity cells (survive stage clearance, single-hue pop) and
/// the metadata stays distinct from real actors.
const FX_ENTITY_ID: u64 = 254;

/// Lifetime seconds of the solid-core -> ring -> ember burst phase.
const FX_BURST_SECONDS: f32 = 0.45;

/// Seconds the telegraph ring sweeps before a queued cast resolves into
/// its burst (reference: dashes are drawn pre-hit).
const FX_TELEGRAPH_SECONDS: f32 = 0.9;

/// Shared gait timing for realtime, fixed-tick, and batched presentation.
/// The phase is expressed in radians and wraps once per stride.
const WALK_PHASE_RATE: f32 = 21.0;
const WALK_IDLE_FADE_RATE: f32 = 12.0;
const WALK_MOVEMENT_GRACE_SECONDS: f32 = 0.28;
const FIXED_PRESENTATION_STEP_SECONDS: f32 = 0.05;

/// Short presentation-only melee swing. Server combat remains authoritative;
/// this timer only gives the already-observed event a readable local motion.
const ATTACK_PHASE_SECONDS: f32 = 0.42;

#[derive(Clone, Copy, Debug, PartialEq)]
struct CombatFx {
    /// World position (mesh scale): ground anchor of the burst.
    pos: [f32; 3],
    /// Single hue per damage/event type (clarity study: FX are bright
    /// saturated single-hue ink on a dark carpet).
    color: [f32; 4],
    born: f32,
    /// Telegraph lead-in; 0.0 = instant burst.
    telegraph: f32,
    /// Max ring radius in world units.
    radius: f32,
}

#[derive(Clone, Copy, Debug)]
struct ActorWalkState {
    position: Point,
    last_moved_at: f32,
    phase: f32,
}

impl GpuRenderer {
    pub fn new(width: i16, height: i16, weather: WeatherMode) -> Result<Self, String> {
        let width = width.max(20);
        let height = height.max(10);
        let (scene_width, scene_height) =
            crate::ui::scene_partition_dims(width as u16, height as u16);
        let scene_width = scene_width.max(20) as i16;
        let scene_height = scene_height.max(10) as i16;
        let ctx = GpuContext::init()?;
        let pipeline = GpuPipeline::new(&ctx, scene_width as u32, scene_height as u32)?;
        let mesh = MeshBuffer::new();
        let batches = SceneBatches::new();

        Ok(Self {
            ctx,
            pipeline,
            mesh,
            batches,
            width,
            height,
            scene_width,
            scene_height,
            weather,
            active_modal: crate::ui::ActiveModal::None,
            last_player_pos: None,
            actor_walk_states: HashMap::new(),
            player_walk_phase: 0.0,
            smooth_cam_pos: None,
            smooth_player_pos: None,
            animation_started: Instant::now(),
            last_render_instant: None,
            camera: CameraState::default(),
            combat_fx: Vec::new(),
            seen_combat_events: Vec::new(),
            attack_started_at: HashMap::new(),
        })
    }

    pub fn orbit_camera(&mut self, dx: i16, dy: i16) {
        self.camera.orbit_cells(dx, dy);
    }

    pub fn reset_camera(&mut self) {
        self.camera.reset();
    }

    pub fn resize(&mut self, width: i16, height: i16) {
        let width = width.max(20);
        let height = height.max(10);
        let (scene_width, scene_height) =
            crate::ui::scene_partition_dims(width as u16, height as u16);
        let scene_width = scene_width.max(20) as i16;
        let scene_height = scene_height.max(10) as i16;
        if self.width == width && self.height == height {
            return;
        }
        if self.scene_width != scene_width || self.scene_height != scene_height {
            self.scene_width = scene_width;
            self.scene_height = scene_height;
            self.pipeline
                .resize(&self.ctx, scene_width as u32, scene_height as u32);
        }
        self.width = width;
        self.height = height;
    }

    pub fn weather(&self) -> WeatherMode {
        self.weather
    }

    pub fn set_weather(&mut self, weather: WeatherMode) {
        self.weather = weather;
    }

    pub fn render(&mut self, world: &WorldSnapshot) -> String {
        self.render_frame(world, world.tick)
    }

    /// Renders a single frame using the offscreen GPU pipeline and instanced batching.
    pub fn render_frame(&mut self, world: &WorldSnapshot, frame: u64) -> String {
        self.mesh.clear();
        self.batches.clear();

        let now = Instant::now();
        let delta_seconds = self
            .last_render_instant
            .replace(now)
            .map_or(0.0, |previous| now.duration_since(previous).as_secs_f32());
        let elapsed_seconds = now.duration_since(self.animation_started).as_secs_f32();
        let (camera_uniforms, lighting) =
            self.build_scene_frame_realtime(world, frame, delta_seconds, elapsed_seconds);

        // Character-clarity pass: project one floating label per actor (the
        // player's label tracks the smoothed render position, exactly where
        // the mesh was drawn).
        let labels = Self::compute_scene_labels(
            world,
            self.smooth_player_pos,
            &camera_uniforms,
            self.scene_width as f32,
            self.scene_height as f32,
        );

        let cells = match self.pipeline.render_and_readback_batched(
            &self.ctx,
            &self.mesh,
            &self.batches,
            &camera_uniforms,
            &lighting,
        ) {
            Ok(c) => c,
            Err(e) => {
                return format!("GPU Render Error: {e}");
            }
        };

        self.format_frame_with_hud(&cells, world, &labels)
    }

    /// Evaluates whether the player is currently walking based on position changes
    /// within the shared stride window, returning the active walk cycle phase.
    ///
    /// When the player is stationary, returns 0.0 so feet remain planted on the ground.
    pub fn update_walk_phase(&mut self, world: &WorldSnapshot) -> f32 {
        if !world.player.stats.is_alive() {
            self.player_walk_phase = 0.0;
            return 0.0;
        }
        let current_pos = world.player.position;
        let moved = self.last_player_pos.is_some_and(|last| last != current_pos);
        self.last_player_pos = Some(current_pos);
        if moved {
            self.player_walk_phase = (self.player_walk_phase
                + FIXED_PRESENTATION_STEP_SECONDS * WALK_PHASE_RATE)
                .rem_euclid(std::f32::consts::TAU);
        } else {
            self.player_walk_phase *=
                (-WALK_IDLE_FADE_RATE * FIXED_PRESENTATION_STEP_SECONDS).exp();
            if self.player_walk_phase.abs() < 0.02 {
                self.player_walk_phase = 0.0;
            }
        }
        self.player_walk_phase
    }

    /// Entity gait runs only after an authoritative position change. This
    /// keeps stationary monsters planted instead of walking in place.
    fn update_actor_walk_phases(
        &mut self,
        world: &WorldSnapshot,
        delta_seconds: f32,
        elapsed_seconds: f32,
    ) -> HashMap<EntityId, f32> {
        let mut live_ids = HashSet::with_capacity(world.entities.len() + 1);
        let mut phases = HashMap::with_capacity(world.entities.len() + 1);
        for actor in world.entities.iter().chain(std::iter::once(&world.player)) {
            let id = actor.id;
            live_ids.insert(id);
            let state = self.actor_walk_states.entry(id).or_insert(ActorWalkState {
                position: actor.position,
                // A first observation is not movement; feet should start
                // planted until the authoritative position changes.
                last_moved_at: f32::NEG_INFINITY,
                phase: 0.0,
            });
            if actor.position != state.position {
                state.position = actor.position;
                state.last_moved_at = elapsed_seconds;
            }
            if actor.stats.is_alive()
                && elapsed_seconds - state.last_moved_at < WALK_MOVEMENT_GRACE_SECONDS
            {
                state.phase = (state.phase + delta_seconds.max(0.0) * WALK_PHASE_RATE)
                    .rem_euclid(std::f32::consts::TAU);
            } else {
                state.phase *= (-WALK_IDLE_FADE_RATE * delta_seconds.max(0.0)).exp();
                if state.phase.abs() < 0.02 {
                    state.phase = 0.0;
                }
            }
            phases.insert(id, state.phase);
        }
        self.actor_walk_states.retain(|id, _| live_ids.contains(id));
        phases
    }

    /// Renders a batch of world snapshots continuously across the double-buffered GPU ring,
    /// keeping the GPU hardware at full utilization and eliminating CPU-GPU idle bubbles.
    pub fn render_batch(&mut self, snapshots: &[WorldSnapshot]) -> Vec<String> {
        if snapshots.is_empty() {
            return Vec::new();
        }

        let mut batch_data = Vec::with_capacity(snapshots.len());
        let mut batch_player_focus = Vec::with_capacity(snapshots.len());
        let mut last_pos = self.last_player_pos;
        let mut walk_phase = self.player_walk_phase;
        let mut smooth_cam = self.smooth_cam_pos;
        let mut smooth_player = self.smooth_player_pos;
        for (i, snapshot) in snapshots.iter().enumerate() {
            let moved = match last_pos {
                Some(prev) => prev != snapshot.player.position,
                None => false,
            };
            last_pos = Some(snapshot.player.position);
            if moved && snapshot.player.stats.is_alive() {
                walk_phase = (walk_phase + FIXED_PRESENTATION_STEP_SECONDS * WALK_PHASE_RATE)
                    .rem_euclid(std::f32::consts::TAU);
            } else if walk_phase.abs() > 0.05 {
                walk_phase *= (-WALK_IDLE_FADE_RATE * FIXED_PRESENTATION_STEP_SECONDS).exp();
            } else {
                walk_phase = 0.0;
            }

            let target_pos = Vec2::new(
                snapshot.player.position.x as f32 * 0.8,
                snapshot.player.position.y as f32 * 0.8,
            );
            smooth_cam = Some(smooth_cam.map_or(target_pos, |p| {
                p.lerp(target_pos, damping_factor(19.7, 0.05))
            }));
            smooth_player = Some(smooth_player.map_or(target_pos, |p| {
                p.lerp(target_pos, damping_factor(28.8, 0.05))
            }));
            batch_player_focus.push(smooth_player.unwrap());

            let mut mesh = MeshBuffer::new();
            let mut batches = SceneBatches::new();
            let (cam, light) = Self::populate_scene_smoothed(
                self.scene_width,
                self.scene_height,
                self.weather,
                snapshot,
                snapshot.tick.max(i as u64),
                walk_phase,
                smooth_cam.unwrap(),
                smooth_player.unwrap(),
                &mut mesh,
                &mut batches,
            );
            batch_data.push((mesh, batches, cam, light));
        }
        self.last_player_pos = last_pos;
        self.player_walk_phase = walk_phase;
        self.smooth_cam_pos = smooth_cam;
        self.smooth_player_pos = smooth_player;

        let frame_refs: Vec<(
            &MeshBuffer,
            &SceneBatches,
            &CameraUniforms,
            &LightingUniforms,
        )> = batch_data.iter().map(|(m, b, c, l)| (m, b, c, l)).collect();

        let cell_batches = match self.pipeline.render_batch(&self.ctx, &frame_refs) {
            Ok(res) => res,
            Err(e) => return vec![format!("GPU Batch Render Error: {e}")],
        };

        let mut output = Vec::with_capacity(cell_batches.len());
        for (((cells, snapshot), (_, _, cam, _)), focus) in cell_batches
            .iter()
            .zip(snapshots.iter())
            .zip(batch_data.iter())
            .zip(batch_player_focus.iter())
        {
            let labels = Self::compute_scene_labels(
                snapshot,
                Some(*focus),
                cam,
                self.scene_width as f32,
                self.scene_height as f32,
            );
            output.push(self.format_frame_with_hud(cells, snapshot, &labels));
        }
        output
    }

    /// Builds scene geometry and fills hardware instance batches on `self.mesh` and `self.batches`.
    pub fn build_scene_frame(
        &mut self,
        world: &WorldSnapshot,
        frame: u64,
    ) -> (CameraUniforms, LightingUniforms) {
        self.build_scene_frame_with_timing(world, frame, 0.05, frame as f32 * 0.05, true)
    }

    fn build_scene_frame_realtime(
        &mut self,
        world: &WorldSnapshot,
        frame: u64,
        delta_seconds: f32,
        elapsed_seconds: f32,
    ) -> (CameraUniforms, LightingUniforms) {
        self.build_scene_frame_with_timing(
            world,
            frame,
            delta_seconds.clamp(0.0, 0.1),
            elapsed_seconds,
            false,
        )
    }

    fn build_scene_frame_with_timing(
        &mut self,
        world: &WorldSnapshot,
        _frame: u64,
        delta_seconds: f32,
        animation_time: f32,
        deterministic: bool,
    ) -> (CameraUniforms, LightingUniforms) {
        let presentation_delta = if deterministic {
            FIXED_PRESENTATION_STEP_SECONDS
        } else {
            delta_seconds
        };
        let actor_walk_phases =
            self.update_actor_walk_phases(world, presentation_delta, animation_time);
        // The player is part of the same actor gait table as monsters and NPCs.
        // Using this one source keeps fixed-tick and realtime cadence identical.
        let walk_phase = actor_walk_phases
            .get(&world.player.id)
            .copied()
            .unwrap_or(0.0);
        let target_x = world.player.position.x as f32 * 0.8;
        let target_z = world.player.position.y as f32 * 0.8;
        let target_pos = Vec2::new(target_x, target_z);

        let cam_pos = match self.smooth_cam_pos {
            Some(prev) => {
                let factor = damping_factor(19.7, presentation_delta);
                let smoothed = prev.lerp(target_pos, factor);
                self.smooth_cam_pos = Some(smoothed);
                smoothed
            }
            None => {
                self.smooth_cam_pos = Some(target_pos);
                target_pos
            }
        };

        let player_render_pos = match self.smooth_player_pos {
            Some(prev) => {
                let factor = damping_factor(28.8, presentation_delta);
                let smoothed = prev.lerp(target_pos, factor);
                self.smooth_player_pos = Some(smoothed);
                smoothed
            }
            None => {
                self.smooth_player_pos = Some(target_pos);
                target_pos
            }
        };

        // Observe messages before geometry is built so attack phases affect the
        // same frame as the combat notification. FX stamping remains after the
        // scene and is still presentation-only.
        self.process_combat_events(world, animation_time, player_render_pos, deterministic);
        let actor_attack_phases = self.active_attack_phases(world, animation_time);

        let uniforms = Self::populate_scene_smoothed_at_time(
            self.scene_width,
            self.scene_height,
            self.weather,
            world,
            animation_time,
            walk_phase,
            cam_pos,
            player_render_pos,
            &mut self.mesh,
            &mut self.batches,
            Some(&actor_walk_phases),
            Some(&actor_attack_phases),
            self.camera,
        );

        // Combat FX (video-tools REPORT #2 impact bursts + #3 telegraphs):
        // observation, spawn, and stamping all key off `animation_time`, so
        // both the fixed-tick and realtime paths use the same animation clock.
        self.stamp_combat_fx(animation_time);
        uniforms
    }

    /// Watches the snapshot's combat log for events the renderer can show
    /// presentation-side: melee hits (`You hit {name} for {n} damage...`)
    /// spawn an ember burst at the named target; spell casts (`You cast
    /// {spell}; ...`) play the reference's telegraph-then-impact sequence
    /// at the caster — dashes sweeping for `FX_TELEGRAPH_SECONDS`, then the
    /// burst. Dedup by exact message so replayed/snapshot worlds cannot
    /// double-spawn.
    fn process_combat_events(
        &mut self,
        world: &WorldSnapshot,
        animation_time: f32,
        player_focus: Vec2,
        deterministic: bool,
    ) {
        for message in &world.messages {
            if self.seen_combat_events.iter().any(|seen| seen == message) {
                continue;
            }
            self.observe_attack_message(message, world, animation_time);
            if !deterministic {
                if let Some(fx) =
                    Self::combat_fx_from_message(message, world, player_focus, animation_time)
                {
                    self.combat_fx.push(fx);
                }
            }
            self.seen_combat_events.push(message.clone());
        }
        if self.seen_combat_events.len() > 64 {
            self.seen_combat_events
                .drain(..self.seen_combat_events.len() - 64);
        }
        if self.combat_fx.len() > 16 {
            self.combat_fx.drain(..self.combat_fx.len() - 16);
        }
        // Drop expired bursts.
        self.combat_fx
            .retain(|fx| animation_time - fx.born < fx.telegraph + FX_BURST_SECONDS);
    }

    /// Records a melee swing for an actor named by the existing combat log.
    /// Local server snapshots use `You hit ...`; TCP event notices can also
    /// arrive as `Combat <attacker> -> <defender>: ...`. Both forms identify
    /// the attacker without inventing a protocol field.
    fn observe_attack_message(
        &mut self,
        message: &str,
        world: &WorldSnapshot,
        animation_time: f32,
    ) {
        let Some(attacker_id) = Self::attack_actor_from_message(message, world) else {
            return;
        };
        let attacker_alive = world
            .entities
            .iter()
            .chain(std::iter::once(&world.player))
            .find(|actor| actor.id == attacker_id)
            .is_some_and(|actor| actor.stats.is_alive());
        if attacker_alive {
            self.attack_started_at.insert(attacker_id, animation_time);
        }
    }

    fn attack_actor_from_message(message: &str, world: &WorldSnapshot) -> Option<EntityId> {
        if message.starts_with("You hit ") {
            return world.player.stats.is_alive().then_some(world.player.id);
        }
        let rest = message.strip_prefix("Combat ")?;
        let attacker = rest.split_once(" -> ")?.0.trim().parse::<EntityId>().ok()?;
        world
            .entities
            .iter()
            .chain(std::iter::once(&world.player))
            .any(|actor| actor.id == attacker)
            .then_some(attacker)
    }

    fn active_attack_phases(
        &mut self,
        world: &WorldSnapshot,
        animation_time: f32,
    ) -> HashMap<EntityId, f32> {
        let live_ids: HashSet<EntityId> = world
            .entities
            .iter()
            .chain(std::iter::once(&world.player))
            .filter(|actor| actor.stats.is_alive())
            .map(|actor| actor.id)
            .collect();
        self.attack_started_at.retain(|id, started| {
            live_ids.contains(id) && animation_time - *started < ATTACK_PHASE_SECONDS
        });

        self.attack_started_at
            .iter()
            .map(|(&id, &started)| {
                (
                    id,
                    attack_phase_at(animation_time - started, ATTACK_PHASE_SECONDS),
                )
            })
            .collect()
    }

    /// Message -> FX translation; None for non-combat lines. Melee hits
    /// resolve the target's world position by display name (presentation
    /// ambiguity only: duplicate mob names pick the first).
    fn combat_fx_from_message(
        message: &str,
        world: &WorldSnapshot,
        player_focus: Vec2,
        animation_time: f32,
    ) -> Option<CombatFx> {
        const EMBER: [f32; 4] = [0.98, 0.55, 0.16, 1.0];
        const ARC: [f32; 4] = [0.97, 0.66, 0.18, 1.0];
        const LIGHTNING: [f32; 4] = [0.50, 0.72, 1.0, 1.0];
        const SPIRIT: [f32; 4] = [0.55, 0.95, 1.0, 1.0];
        const NEUTRAL: [f32; 4] = [0.80, 0.80, 0.86, 1.0];

        let world_pos = |x: f32, z: f32| [x, 0.15, z];
        if let Some(rest) = message.strip_prefix("You hit ") {
            let name_end = rest.find(" for ")?;
            let name = rest[..name_end].trim();
            if let Some(target) = world.entities.iter().find(|entity| entity.name == name) {
                return Some(CombatFx {
                    pos: world_pos(
                        target.position.x as f32 * 0.8,
                        target.position.y as f32 * 0.8,
                    ),
                    color: EMBER,
                    born: animation_time,
                    telegraph: 0.0,
                    radius: 0.8,
                });
            }
            return None;
        }
        if let Some(rest) = message.strip_prefix("You cast ") {
            let spell = rest
                .split(';')
                .next()
                .unwrap_or(rest)
                .trim()
                .to_ascii_lowercase();
            let (color, radius, telegraph) = if spell.contains("thunder") {
                (LIGHTNING, 1.9, FX_TELEGRAPH_SECONDS)
            } else if spell.contains("iron") || spell.contains("will") {
                (SPIRIT, 1.1, 0.6)
            } else if spell.contains("cleave") {
                (ARC, 1.0, 0.7)
            } else {
                (NEUTRAL, 0.9, FX_TELEGRAPH_SECONDS)
            };
            return Some(CombatFx {
                pos: world_pos(player_focus.x, player_focus.y),
                color,
                born: animation_time,
                telegraph,
                radius,
            });
        }
        None
    }

    /// Stamps this frame's telegraph dashes and burst geometry.
    fn stamp_combat_fx(&mut self, animation_time: f32) {
        for fx in &self.combat_fx {
            let age = animation_time - fx.born;
            if age < 0.0 {
                continue;
            }
            // Class 0 on purpose: the post pass tints edge cells 60% toward
            // the class color, and FX quads are so small that nearly every
            // cell is an edge — the class tint would swamp the authored
            // per-school hue. Class 0 keeps emissive albedo verbatim.
            self.mesh.set_entity_context(FX_ENTITY_ID, 0);
            if fx.telegraph > 0.0 && age < fx.telegraph {
                let sweep = age / fx.telegraph;
                // Dashes sweep inward + rotate while the wind-up runs.
                let radius = fx.radius * (1.0 - 0.25 * sweep);
                self.mesh
                    .add_ground_ring(fx.pos, radius, fx.color, sweep * 1.5, 10);
            } else {
                let burst_age = if fx.telegraph > 0.0 {
                    age - fx.telegraph
                } else {
                    age
                };
                let progress = burst_age / FX_BURST_SECONDS;
                self.mesh
                    .add_impact_burst(fx.pos, progress, fx.color, fx.radius.max(0.8));
            }
            self.mesh.set_entity_context(0, 0);
        }
    }

    /// Populates scene geometry and fills hardware instance batches for a given target buffer set.
    #[allow(clippy::too_many_arguments)]
    pub fn populate_scene(
        width: i16,
        height: i16,
        weather: WeatherMode,
        world: &WorldSnapshot,
        frame: u64,
        walk_phase: f32,
        mesh: &mut MeshBuffer,
        batches: &mut SceneBatches,
    ) -> (CameraUniforms, LightingUniforms) {
        let p_pos = world.player.position;
        let px = p_pos.x as f32 * 0.8;
        let pz = p_pos.y as f32 * 0.8;
        let pos = Vec2::new(px, pz);
        Self::populate_scene_smoothed(
            width, height, weather, world, frame, walk_phase, pos, pos, mesh, batches,
        )
    }

    /// Populates scene geometry with smoothed camera and player positions,
    /// dynamic lighting, and view-frustum culled instanced props.
    #[allow(clippy::too_many_arguments)]
    pub fn populate_scene_smoothed(
        width: i16,
        height: i16,
        weather: WeatherMode,
        world: &WorldSnapshot,
        frame: u64,
        walk_phase: f32,
        cam_pos: Vec2,
        player_render_pos: Vec2,
        mesh: &mut MeshBuffer,
        batches: &mut SceneBatches,
    ) -> (CameraUniforms, LightingUniforms) {
        Self::populate_scene_smoothed_at_time(
            width,
            height,
            weather,
            world,
            frame as f32 * 0.05,
            walk_phase,
            cam_pos,
            player_render_pos,
            mesh,
            batches,
            None,
            None,
            CameraState::default(),
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn populate_scene_smoothed_at_time(
        width: i16,
        height: i16,
        weather: WeatherMode,
        world: &WorldSnapshot,
        time: f32,
        walk_phase: f32,
        cam_pos: Vec2,
        player_render_pos: Vec2,
        mesh: &mut MeshBuffer,
        batches: &mut SceneBatches,
        actor_walk_phases: Option<&HashMap<EntityId, f32>>,
        actor_attack_phases: Option<&HashMap<EntityId, f32>>,
        camera_state: CameraState,
    ) -> (CameraUniforms, LightingUniforms) {
        let px = player_render_pos.x;
        let pz = player_render_pos.y;

        // 1. Setup the OSRS-style elevated orbit camera. The camera follows the
        // smoothed player focus, while yaw/pitch remain client-local and are
        // changed only by holding right mouse and dragging.
        let horizontal = camera_state.distance * camera_state.pitch.cos();
        let cam_eye = Vec3::new(
            cam_pos.x + camera_state.yaw.sin() * horizontal,
            camera_state.pitch.sin() * camera_state.distance,
            cam_pos.y + camera_state.yaw.cos() * horizontal,
        );
        let cam_target = Vec3::new(cam_pos.x, 0.8, cam_pos.y);
        let cam_up = Vec3::new(0.0, 1.0, 0.0);

        let view = Mat4::look_at_rh(cam_eye, cam_target, cam_up);
        let aspect = (width as f32 / height as f32) * 0.48;
        let proj = Mat4::perspective_rh(34.0f32.to_radians(), aspect, 0.1, 100.0);
        let view_proj = proj * view;
        let inv_view_proj = view_proj.inverse();
        let frustum = Frustum::from_view_proj(&view_proj);

        let weather_id = match weather {
            WeatherMode::Rain => 0,
            WeatherMode::Snow => 1,
            WeatherMode::Storm => 2,
        };

        let camera_uniforms = CameraUniforms {
            view_proj: view_proj.to_cols_array_2d(),
            inv_view_proj: inv_view_proj.to_cols_array_2d(),
            camera_pos: [cam_eye.x, cam_eye.y, cam_eye.z, 1.0],
            time,
            weather_mode: weather_id,
            width: width as u32,
            height: height as u32,
        };

        // 2. Dynamic Point Lights
        let mut lighting = LightingUniforms {
            sun_dir: [
                (time * 0.05).sin() * 0.3 + 0.35,
                0.82,
                (time * 0.05).cos() * 0.3 + 0.45,
                0.0,
            ],
            ..Default::default()
        };

        let mut lights = Vec::new();

        // Player Hip Lantern (Warm flickering light pool). A defeated player
        // keeps a snapshot/model for continuity, but should not emit a live
        // actor light.
        if world.player.stats.is_alive() {
            let player_flicker = (time * 8.0).sin() * 0.12;
            lights.push(GpuPointLight {
                position: [px + 0.4, 1.2, pz + 0.2, 9.5],
                color: [1.0, 0.82, 0.45, 1.2 + player_flicker],
            });
        }

        // Gate Torches at fixed world points (Gate at y = -12 -> z = -9.6)
        let gate_flicker1 = (time * 9.5).sin() * 0.15;
        let gate_flicker2 = (time * 11.2).cos() * 0.15;
        lights.push(GpuPointLight {
            position: [-2.2, 2.6, -9.6 + 0.65, 8.5],
            color: [1.0, 0.62, 0.20, 1.4 + gate_flicker1],
        });
        lights.push(GpuPointLight {
            position: [2.2, 2.6, -9.6 + 0.65, 8.5],
            color: [1.0, 0.62, 0.20, 1.4 + gate_flicker2],
        });

        // Dynamic Entity Lighting (Living actors & loot cast atmospheric lighting pools onto the ground)
        for entity in &world.entities {
            // Dead actors remain in snapshots for labels/combat history, but
            // should not continue to cast an active creature light pool.
            if entity.kind != EntityKind::Item && !entity.stats.is_alive() {
                continue;
            }
            let ex = entity.position.x as f32 * 0.8;
            let ez = entity.position.y as f32 * 0.8;
            match entity.kind {
                EntityKind::Npc => {
                    let herb_flicker = (time * 7.0 + ex).sin() * 0.1;
                    lights.push(GpuPointLight {
                        position: [ex + 0.35, 1.7, ez + 0.15, 8.0],
                        color: [0.95, 0.80, 0.35, 1.3 + herb_flicker], // Warm golden amber lantern glow
                    });
                }
                EntityKind::Monster => {
                    let pulse = (time * 4.5 + ex * 1.7).sin() * 0.15;
                    lights.push(GpuPointLight {
                        position: [ex, 0.85, ez, 6.5],
                        color: [0.88, 0.22, 0.14, 1.05 + pulse], // Sinister crimson-amber ground glow
                    });
                }
                EntityKind::Item => {
                    let shimmer = (time * 5.2 + ez * 2.1).cos() * 0.18;
                    lights.push(GpuPointLight {
                        position: [ex, 0.45, ez, 5.0],
                        color: [0.25, 0.78, 0.95, 1.15 + shimmer], // Ethereal celestial cyan shimmering glow
                    });
                }
                EntityKind::Player if entity.id != world.player.id => {
                    let other_flicker = (time * 7.8 + ex).sin() * 0.12;
                    lights.push(GpuPointLight {
                        position: [ex + 0.4, 1.2, ez + 0.2, 9.0],
                        color: [1.0, 0.82, 0.45, 1.2 + other_flicker], // Adventurer's warm lantern glow
                    });
                }
                _ => {}
            }
        }

        // Prioritize lights closest to the camera/player
        lights.sort_by(|a, b| {
            let da = (a.position[0] - px).hypot(a.position[2] - pz);
            let db = (b.position[0] - px).hypot(b.position[2] - pz);
            da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
        });

        let light_count = lights.len().min(16);
        lighting.num_point_lights = light_count as u32;
        for (i, l) in lights.iter().take(16).enumerate() {
            lighting.point_lights[i] = *l;
        }

        // 3. Dynamic Scene Geometry (Cobblestone Highway, Altar, Gate, Entities)
        mesh.set_entity_context(0, 0);
        let terrain_radius = ((width as f32).max(100.0) * 0.38).clamp(42.0, 68.0);
        mesh.add_terrain(cam_pos.x, cam_pos.y, terrain_radius);
        mesh.add_gate([0.0, 0.0, -9.6]);
        mesh.add_ruin_altar([-6.4, 0.0, -4.0]);

        for entity in &world.entities {
            let ex = entity.position.x as f32 * 0.8;
            let ez = entity.position.y as f32 * 0.8;
            let entity_class = match entity.kind {
                EntityKind::Player => 1,
                EntityKind::Monster => 2,
                EntityKind::Npc => 3,
                EntityKind::Item => 4,
            };
            mesh.set_entity_context(entity.id, entity_class);
            let actor_vertices = mesh.vertices.len();
            let gait_phase = actor_walk_phases
                .and_then(|phases| phases.get(&entity.id).copied())
                .unwrap_or(0.0);
            let attack_phase = actor_attack_phases
                .and_then(|phases| phases.get(&entity.id).copied())
                .unwrap_or(0.0);
            let model_scale = match entity.kind {
                EntityKind::Monster => {
                    let label = entity.name.to_ascii_lowercase();
                    if label.contains("warrior") {
                        mesh.add_skeleton_warrior([ex, 0.0, ez], gait_phase, attack_phase);
                        0.88 // readable OSRS-style player-sized champion
                    } else if label.contains("skeleton") {
                        mesh.add_skeleton([ex, 0.0, ez], gait_phase, attack_phase);
                        0.84 // lean undead frame, still readable at top-down zoom
                    } else {
                        mesh.add_mossling_with_attack([ex, 0.0, ez], gait_phase, attack_phase);
                        0.90 // stocky, low-slung beast
                    }
                }
                EntityKind::Npc => {
                    mesh.add_herbalist([ex, 0.0, ez], time);
                    0.88 // human height; staff and lantern scale with the body
                }
                EntityKind::Player => {
                    mesh.add_humanoid([ex, 0.0, ez], 0.0, gait_phase, attack_phase);
                    0.90
                }
                EntityKind::Item => {
                    let hover = (time * 3.0 + ex).sin() * 0.15;
                    mesh.add_box(
                        [ex, 0.35 + hover, ez],
                        [0.45, 0.35, 0.35],
                        [0.95, 0.85, 0.35, 1.0],
                        4.0,
                        0.1,
                    );
                    1.0
                }
            };
            if entity.kind != EntityKind::Item {
                mesh.scale_vertices_since(actor_vertices, [ex, 0.0, ez], model_scale);
                // Keep every readable actor's authored front toward the orbit
                // camera.  Without this, remote players and mobs showed a
                // side/back silhouette while their labels stayed screen-flat.
                // This is presentation-only; entity facing is not gameplay
                // state and never enters snapshots or protocol messages.
                mesh.rotate_vertices_since(actor_vertices, [ex, 0.0, ez], camera_state.yaw);
            }
        }

        mesh.set_entity_context(world.player.id, 1);
        let player_vertices = mesh.vertices.len();
        // The humanoid part geometry faces +Z (toward the yaw-0 camera eye);
        // passing the camera yaw keeps the player's head and body facing the
        // viewer as the orbit camera swings. Presentation-only: yaw never
        // enters snapshots or the wire protocol.
        let player_attack_phase = actor_attack_phases
            .and_then(|phases| phases.get(&world.player.id).copied())
            .unwrap_or(0.0);
        mesh.add_humanoid(
            [px, 0.0, pz],
            camera_state.yaw,
            walk_phase,
            player_attack_phase,
        );
        mesh.scale_vertices_since(player_vertices, [px, 0.0, pz], 0.90);
        mesh.set_entity_context(0, 0);

        // 4. Instanced Props with View-Frustum Culling (Asciicker plane culling optimization)
        // Scaled to exact 0.8 world tile grid:
        // Landmark Trees: (-7, -2) -> [-5.6, -1.6], (8, -3) -> [6.4, -2.4],
        // (9, -10) -> [7.2, -8.0], (-9, -10) -> [-7.2, -8.0]
        let landmark_trees = [
            ([-5.6, 0.0, -1.6], 1.0, (time * 2.0).sin()),
            ([6.4, 0.0, -2.4], 1.1, (time * 2.3).sin()),
            ([7.2, 0.0, -8.0], 0.95, (time * 1.8).sin()),
            ([-7.2, 0.0, -8.0], 1.05, (time * 2.1).sin()),
        ];
        for (pos, scale, sway) in landmark_trees {
            if frustum.test_sphere(Vec3::from(pos) + Vec3::new(0.0, 3.5, 0.0), scale * 5.2) {
                batches.add_pine_tree(pos, scale, sway);
            }
        }

        // Procedural background trees matching collision grid (off-highway |tree_wx| > 4)
        let cam_tile_x = (cam_pos.x / 0.8).round() as i16;
        let cam_tile_y = (cam_pos.y / 0.8).round() as i16;
        let min_gx = (cam_tile_x - 30).div_euclid(11) - 1;
        let max_gx = (cam_tile_x + 30).div_euclid(11) + 1;
        let min_gy = (cam_tile_y - 25).div_euclid(9) - 1;
        let max_gy = (cam_tile_y + 25).div_euclid(9) + 1;

        for gx in min_gx..=max_gx {
            for gy in min_gy..=max_gy {
                let seed = terrain_seed(gx, gy);
                let tree_wx = gx * 11 + (seed % 5) as i16;
                let tree_wy = gy * 9 + ((seed / 7) % 5) as i16;
                // Trees stop at the shoreline: the beach and the open water
                // south of the walkable bound (see add_terrain's ocean band)
                // must stay clear — a pine standing in the surf rasterizes
                // as letter noise across the sea.
                if tree_wy >= 32 {
                    continue;
                }
                if tree_wx.abs() > 4
                    && seed % 9 == 0
                    && (tree_wx, tree_wy) != (-7, -2)
                    && (tree_wx, tree_wy) != (8, -3)
                    && (tree_wx, tree_wy) != (9, -10)
                    && (tree_wx, tree_wy) != (-9, -10)
                {
                    let wx = tree_wx as f32 * 0.8;
                    let wz = tree_wy as f32 * 0.8;
                    let scale = 0.92 + ((seed % 7) as f32) * 0.04;
                    let sway = (time * 1.5 + (seed as f32 * 0.17)).sin();
                    let pos = [wx, 0.0, wz];
                    if frustum.test_sphere(Vec3::from(pos) + Vec3::new(0.0, 3.5, 0.0), scale * 5.2)
                    {
                        batches.add_pine_tree(pos, scale, sway);
                    }
                }
            }
        }

        let pillars = [([-8.0, 0.0, -4.0], 1.0), ([-4.8, 0.0, -4.0], 0.76)];
        for (pos, height_scale) in pillars {
            if frustum.test_sphere(Vec3::from(pos) + Vec3::new(0.0, 1.5, 0.0), 7.0) {
                batches.add_pillar(pos, height_scale);
            }
        }

        // Meadow grass: sparse crossed-blade tufts (1 in 5 tiles, ~20%
        // coverage). The old per-cell field put stroke glyphs ('|', '/', '\')
        // on most of the screen (video-tools REPORT #1: reference ground is a
        // quiet punctuation carpet with occasional tufts, not a blade forest).
        // Vertex-cheap; reduce modulus or range if batch timing regresses.
        for gx in (cam_tile_x - 18)..=(cam_tile_x + 18) {
            for gy in (cam_tile_y - 14)..=(cam_tile_y + 14) {
                // Grass stops at the beach line: blades over the wet sand or
                // the ocean read as texture soup on the water (user-reported
                // "grass texture everywhere" on the shore view).
                if gy >= 32 {
                    continue;
                }
                let seed = terrain_seed(gx, gy);
                if seed % 5 != 0 {
                    continue;
                }
                let x = gx as f32 * 0.8 + (seed % 7) as f32 * 0.08;
                let z = gy as f32 * 0.8 + ((seed / 7) % 7) as f32 * 0.08;
                if (x - cam_pos.x).abs() > 28.0 || (z - cam_pos.y).abs() > 22.0 {
                    continue;
                }
                let phase = time * 2.4 + seed as f32 * 0.013;
                // Match the terrain generator's shared meadow height field so
                // crossed blades stay planted while the ground gently rolls.
                let ground_y = if x.abs() < 3.2 {
                    0.0
                } else if x.abs() < 3.8 {
                    0.08
                } else {
                    (x * 0.25).sin() * (z * 0.25).cos() * 0.15
                };
                mesh.add_grass_blade_grounded(
                    x,
                    z,
                    ground_y,
                    0.28 + (seed % 5) as f32 * 0.06,
                    phase.sin() * 0.14,
                );
            }
        }

        batches.add_weather_particles(cam_pos.x, cam_pos.y, time, weather_id);

        (camera_uniforms, lighting)
    }

    /// O(1) mathematical screen-to-world depth unprojection (borrowed from Asciicker's depth unprojection).
    /// Given screen cell coordinates (screen_x, screen_y) and the GPU cell buffer, recovers the exact 3D world coordinate.
    pub fn unproject_at(
        &self,
        screen_x: usize,
        screen_y: usize,
        cells: &[GpuCell],
        camera: &CameraUniforms,
    ) -> Option<Vec3> {
        let idx = screen_y * self.scene_width as usize + screen_x;
        let cell = cells.get(idx)?;
        let depth = cell.depth();
        Some(camera.unproject_screen_to_world(screen_x as f32, screen_y as f32, depth))
    }

    /// Character-clarity pass (video-tools REPORT clarity study): one
    /// projected floating label per actor — dark-backed nameplate anchors
    /// over heads, HP digits, and the dashed ring under the melee target.
    /// The target rule mirrors `Command::Attack`'s presentation-side view:
    /// the nearest living monster in melee range is what the next `F` hits.
    /// Uses the same `view_proj` the mesh pass used, so plates land on heads.
    fn compute_scene_labels<'a>(
        world: &'a WorldSnapshot,
        player_render_pos: Option<Vec2>,
        camera_uniforms: &CameraUniforms,
        scene_width: f32,
        scene_height: f32,
    ) -> Vec<crate::ui::SceneLabel<'a>> {
        let view_proj = Mat4::from_cols_array_2d(&camera_uniforms.view_proj);
        let target_id = world
            .entities
            .iter()
            .filter(|entity| {
                entity.kind == EntityKind::Monster
                    && entity.stats.is_alive()
                    && in_melee_range(world.player.position, entity.position)
            })
            .min_by_key(|entity| {
                (entity.position.x - world.player.position.x).abs()
                    + (entity.position.y - world.player.position.y).abs()
            })
            .map(|entity| entity.id);

        let project = |world_pos: Vec3| -> Option<(f32, f32)> {
            let clip = view_proj * world_pos.extend(1.0);
            if clip.w <= 0.0 {
                return None;
            }
            let ndc = clip.truncate() / clip.w;
            // Small margin so actors grazing the edge still get plates.
            if ndc.x < -1.05 || ndc.x > 1.05 || ndc.y < -1.05 || ndc.y > 1.05 {
                return None;
            }
            Some((
                (ndc.x * 0.5 + 0.5) * scene_width,
                (0.5 - ndc.y * 0.5) * scene_height,
            ))
        };

        let player_focus = player_render_pos.unwrap_or_else(|| {
            Vec2::new(
                world.player.position.x as f32 * 0.8,
                world.player.position.y as f32 * 0.8,
            )
        });

        let mut labels = Vec::with_capacity(world.entities.len() + 1);
        for actor in world.entities.iter().chain(std::iter::once(&world.player)) {
            let is_player = actor.id == world.player.id;
            let (x, z) = if is_player {
                (player_focus.x, player_focus.y)
            } else {
                (actor.position.x as f32 * 0.8, actor.position.y as f32 * 0.8)
            };
            let head_y = match actor.kind {
                EntityKind::Item => 0.6,
                EntityKind::Monster => {
                    let label = actor.name.to_ascii_lowercase();
                    if label.contains("warrior") {
                        1.9
                    } else if label.contains("skeleton") {
                        1.65
                    } else {
                        1.35
                    }
                }
                EntityKind::Npc => 2.05,
                EntityKind::Player => 2.1,
            };
            let (Some((col, feet_row)), Some((_, head_row))) = (
                project(Vec3::new(x, 0.1, z)),
                project(Vec3::new(x, head_y, z)),
            ) else {
                continue;
            };
            let hp = match actor.kind {
                EntityKind::Item | EntityKind::Player => None,
                _ => Some((
                    actor.stats.hp.max(0) as u32,
                    actor.stats.max_hp.max(0) as u32,
                )),
            };
            labels.push(crate::ui::SceneLabel {
                col,
                feet_row,
                head_row,
                name: actor.name.as_str(),
                hp,
                is_player,
                is_target: Some(actor.id) == target_id,
            });
        }
        labels
    }

    /// High-performance zero-allocation ANSI string formatter (native 1:1 grid).
    ///
    /// Foreground/background channels are quantized to 32 levels before
    /// run-merging: smooth GPU gradients otherwise emit a fresh truecolor
    /// escape on nearly every cell, bloating frames (~2x bytes) and choking
    /// Windows ConPTY parsing at zoomed-out grids. 5-bit steps are visually
    /// near-identical for glyph art but collapse long runs into one escape.
    fn format_frame_with_hud(
        &self,
        cells: &[GpuCell],
        world: &WorldSnapshot,
        labels: &[crate::ui::SceneLabel<'_>],
    ) -> String {
        // Lower GPU cells to shared scene cells and compose the full frame in
        // one Ratatui buffer (scene + HUD + overlay), exactly like the CPU
        // path. Channel quantization is preserved for ANSI run-merging.
        let scene_width = self.scene_width as usize;
        let scene: Vec<crate::partitions::SceneCell> = cells
            .iter()
            .map(|cell| crate::partitions::SceneCell {
                glyph: std::char::from_u32(cell.glyph).unwrap_or(' '),
                fg: (
                    quantize_channel(cell.fg_r) as u8,
                    quantize_channel(cell.fg_g) as u8,
                    quantize_channel(cell.fg_b) as u8,
                ),
                bg: Some((
                    quantize_channel(cell.bg_r) as u8,
                    quantize_channel(cell.bg_g) as u8,
                    quantize_channel(cell.bg_b) as u8,
                )),
            })
            .collect();

        let mut hud = crate::ui::RatatuiHud::new();
        hud.render_composed_frame(crate::ui::ComposedScene {
            world,
            // The composed HUD frame always spans the full terminal; the
            // scene grid is the smaller partition `blit_scene` fills exactly.
            width: self.width.max(20) as u16,
            height: self.height.max(10) as u16,
            weather: self.weather,
            backend_name: "WGPU 3D BATCHED PIPELINE",
            modal: self.active_modal,
            cells: &scene,
            cells_width: scene_width.max(1),
            labels,
        })
    }
}

fn damping_factor(rate: f32, delta_seconds: f32) -> f32 {
    1.0 - (-rate * delta_seconds.max(0.0)).exp()
}

fn attack_phase_at(age_seconds: f32, duration_seconds: f32) -> f32 {
    if duration_seconds <= 0.0 {
        return 1.0;
    }
    (age_seconds / duration_seconds).clamp(0.0, 1.0)
}

/// Quantizes a 0-255 channel to 32 levels for ANSI run-merging.
#[inline]
fn quantize_channel(value: u32) -> u32 {
    (value.min(255) >> 3) << 3
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sample_world;

    #[test]
    fn test_gpu_renderer_creation_and_render() {
        let Ok(mut renderer) = GpuRenderer::new(60, 24, WeatherMode::Rain) else {
            eprintln!("Skipping test_gpu_renderer_creation_and_render: no GPU adapter on host");
            return;
        };
        let world = sample_world();
        let frame = renderer.render_frame(&world, 0);

        assert!(!frame.is_empty(), "GPU render output should not be empty");
        assert!(
            frame.contains("\x1b["),
            "Should contain ANSI truecolor escape codes"
        );
        assert!(frame.contains("WGPU 3D"), "Should display WGPU HUD");
    }

    #[test]
    fn test_gpu_renderer_resize() {
        let Ok(mut renderer) = GpuRenderer::new(40, 20, WeatherMode::Rain) else {
            eprintln!("Skipping test_gpu_renderer_resize: no GPU adapter on host");
            return;
        };
        renderer.resize(80, 30);
        assert_eq!(renderer.width, 80);
        assert_eq!(renderer.height, 30);
        let world = sample_world();
        let frame = renderer.render_frame(&world, 1);
        assert!(!frame.is_empty());
    }

    #[test]
    fn test_gpu_renderer_batch_processing() {
        let Ok(mut renderer) = GpuRenderer::new(50, 20, WeatherMode::Rain) else {
            eprintln!("Skipping test_gpu_renderer_batch_processing: no GPU adapter on host");
            return;
        };
        let mut world1 = sample_world();
        world1.tick = 1;
        let mut world2 = sample_world();
        world2.tick = 2;
        let mut world3 = sample_world();
        world3.tick = 3;

        let batch_outputs = renderer.render_batch(&[world1, world2, world3]);
        assert_eq!(
            batch_outputs.len(),
            3,
            "Batch rendering should produce 3 frames"
        );
        for (i, frame) in batch_outputs.iter().enumerate() {
            assert!(!frame.is_empty(), "Batch frame {i} should not be empty");
            assert!(
                frame.contains("\x1b["),
                "Batch frame {i} should contain ANSI codes"
            );
        }
    }

    #[test]
    fn test_gpu_renderer_instanced_batches() {
        let world = sample_world();
        let mut mesh = MeshBuffer::new();
        let mut batches = SceneBatches::new();

        let _ = GpuRenderer::populate_scene(
            40,
            20,
            WeatherMode::Rain,
            &world,
            10,
            0.0,
            &mut mesh,
            &mut batches,
        );
        assert!(
            batches.total_instance_count() > 100,
            "Should batch weather particles, trees, and pillars"
        );
        assert!(
            batches.tree_instances.len() >= 4,
            "Should have at least 4 instanced trees"
        );
        assert_eq!(
            batches.pillar_instances.len(),
            2,
            "Should have 2 instanced pillars"
        );
        assert_eq!(
            batches.particle_instances.len(),
            180,
            "Should have 180 instanced weather particles"
        );
    }

    #[test]
    fn test_frustum_culling() {
        let view = glam::Mat4::look_at_rh(
            glam::Vec3::new(0.0, 10.0, -10.0),
            glam::Vec3::new(0.0, 0.0, 0.0),
            glam::Vec3::new(0.0, 1.0, 0.0),
        );
        let proj = glam::Mat4::perspective_rh(45.0f32.to_radians(), 1.0, 0.1, 100.0);
        let vp = proj * view;
        let frustum = Frustum::from_view_proj(&vp);

        // Object at origin should be inside
        assert!(frustum.test_sphere(glam::Vec3::new(0.0, 0.0, 0.0), 1.0));
        assert!(frustum.test_aabb(
            glam::Vec3::new(-1.0, -1.0, -1.0),
            glam::Vec3::new(1.0, 1.0, 1.0),
        ));

        // Object far behind camera should be culled
        assert!(!frustum.test_sphere(glam::Vec3::new(0.0, 10.0, -50.0), 1.0));
        assert!(!frustum.test_aabb(
            glam::Vec3::new(-1.0, 9.0, -51.0),
            glam::Vec3::new(1.0, 11.0, -49.0),
        ));
    }

    #[test]
    fn test_wavefront_obj_loader() {
        let obj_cube = r#"
# Simple 3D OBJ Pyramid
v 0.0 1.0 0.0
v -1.0 0.0 -1.0
v 1.0 0.0 -1.0
v 1.0 0.0 1.0
v -1.0 0.0 1.0
f 1 2 3
f 1 3 4
f 1 4 5
f 1 5 2
f 2 5 4 3
"#;
        let mb = MeshBuffer::load_obj(obj_cube, [1.0, 0.8, 0.2, 1.0], 1.0, 0.3)
            .expect("Should parse valid OBJ string");
        assert!(
            !mb.vertices.is_empty(),
            "MeshBuffer should contain vertices"
        );
        assert!(
            !mb.indices.is_empty(),
            "MeshBuffer should contain triangle indices"
        );
        // 4 triangular sides (4 * 3 = 12 indices) + quad base (2 * 3 = 6 indices) = 18 indices
        assert_eq!(
            mb.indices.len(),
            18,
            "Pyramid should have 6 triangles = 18 indices"
        );
    }

    #[test]
    fn test_depth_unproject() {
        let cam_eye = glam::Vec3::new(0.0, 10.0, -10.0);
        let cam_target = glam::Vec3::new(0.0, 0.0, 0.0);
        let view = glam::Mat4::look_at_rh(cam_eye, cam_target, glam::Vec3::new(0.0, 1.0, 0.0));
        let proj = glam::Mat4::perspective_rh(45.0f32.to_radians(), 1.0, 0.1, 100.0);
        let view_proj = proj * view;
        let inv_view_proj = view_proj.inverse();

        let cam_uniforms = CameraUniforms {
            view_proj: view_proj.to_cols_array_2d(),
            inv_view_proj: inv_view_proj.to_cols_array_2d(),
            camera_pos: [cam_eye.x, cam_eye.y, cam_eye.z, 1.0],
            time: 0.0,
            weather_mode: 0,
            width: 100,
            height: 100,
        };

        // Center of screen (50, 50) projected forward to depth corresponding to target
        let target_ndc = view_proj * cam_target.extend(1.0);
        let target_depth = target_ndc.z / target_ndc.w;

        let reconstructed = cam_uniforms.unproject_screen_to_world(50.0, 50.0, target_depth);
        assert!(
            (reconstructed - cam_target).length() < 0.01,
            "Unprojected point should match original target: {reconstructed:?} vs {cam_target:?}"
        );
    }

    #[test]
    fn test_player_feet_do_not_move_when_idle() {
        let Ok(mut renderer) = GpuRenderer::new(40, 20, WeatherMode::Rain) else {
            eprintln!("Skipping test_player_feet_do_not_move_when_idle: no GPU adapter on host");
            return;
        };
        let mut world = sample_world();

        // 1. Initial idle state: player is stationary, walk_phase must be 0.0
        let idle_phase = renderer.update_walk_phase(&world);
        assert_eq!(
            idle_phase, 0.0,
            "Player feet should not move when standing still"
        );

        // Subsequent idle frame at same position: still 0.0
        let idle_phase_2 = renderer.update_walk_phase(&world);
        assert_eq!(
            idle_phase_2, 0.0,
            "Player feet should remain stationary on subsequent idle frames"
        );

        // 2. Player takes a step: position changes
        world.player.position.x += 1;
        let walk_phase = renderer.update_walk_phase(&world);
        assert!(
            walk_phase > 0.0,
            "Player walk phase should activate when position changes: {walk_phase}"
        );

        // 3. Humanoid mesh at 0.0 walk_phase has both boots planted side-by-side with zero leg swing
        let mut mesh = MeshBuffer::new();
        mesh.add_humanoid([0.0, 0.0, 0.0], 0.0, 0.0, 0.0);
        assert!(!mesh.vertices.is_empty());
    }

    #[test]
    fn test_character_models_and_monster_dispatch() {
        let mut mesh_player = MeshBuffer::new();
        mesh_player.add_humanoid([0.0, 0.0, 0.0], 0.0, 0.0, 0.0);
        assert!(
            !mesh_player.vertices.is_empty(),
            "Humanoid knight model should produce vertices"
        );

        let mut mesh_skel = MeshBuffer::new();
        mesh_skel.add_skeleton([0.0, 0.0, 0.0], 0.0, 0.0);
        assert!(
            !mesh_skel.vertices.is_empty(),
            "Skeleton model should produce vertices"
        );

        let mut mesh_warrior = MeshBuffer::new();
        mesh_warrior.add_skeleton_warrior([0.0, 0.0, 0.0], 0.0, 0.0);
        assert!(
            mesh_warrior.vertices.len() > mesh_skel.vertices.len(),
            "Skeleton warrior should have more geometry than unarmored skeleton"
        );

        let mut mesh_mossling = MeshBuffer::new();
        mesh_mossling.add_mossling([0.0, 0.0, 0.0], 0.0);
        assert!(
            !mesh_mossling.vertices.is_empty(),
            "Mossling model should produce vertices"
        );

        let mut mesh_herbalist = MeshBuffer::new();
        mesh_herbalist.add_herbalist([0.0, 0.0, 0.0], 0.0);
        assert!(
            !mesh_herbalist.vertices.is_empty(),
            "Herbalist model should produce vertices"
        );
    }

    #[test]
    fn test_entity_metadata_is_stamped_on_dynamic_geometry() {
        let mut mesh = MeshBuffer::new();
        mesh.set_entity_context(42, 2);
        mesh.add_box(
            [0.0, 0.5, 0.0],
            [1.0, 1.0, 1.0],
            [1.0, 0.2, 0.1, 1.0],
            1.0,
            0.2,
        );

        assert!(!mesh.vertices.is_empty());
        assert!(mesh
            .vertices
            .iter()
            .all(|vertex| vertex.entity_id == 42.0 && vertex.entity_class == 2.0));
    }

    #[test]
    fn test_populate_scene_with_all_entity_kinds() {
        let mut world = sample_world();
        world.entities.push(ascii_mmorpg_shared::ActorSnapshot {
            id: 4,
            name: "Skeleton Warrior".into(),
            kind: EntityKind::Monster,
            position: ascii_mmorpg_shared::Point { x: 5, y: 5 },
            stats: ascii_mmorpg_shared::Stats::healthy(15, 5, 2),
            glyph: 'W',
        });
        world.entities.push(ascii_mmorpg_shared::ActorSnapshot {
            id: 5,
            name: "Herbalist".into(),
            kind: EntityKind::Npc,
            position: ascii_mmorpg_shared::Point { x: 1, y: 1 },
            stats: ascii_mmorpg_shared::Stats::healthy(20, 1, 1),
            glyph: 'H',
        });

        let mut mesh = MeshBuffer::new();
        let mut batches = SceneBatches::default();
        let (_uniforms, lighting) = GpuRenderer::populate_scene(
            40,
            20,
            WeatherMode::Rain,
            &world,
            20,
            0.5,
            &mut mesh,
            &mut batches,
        );

        assert!(
            !mesh.vertices.is_empty(),
            "MeshBuffer must contain vertices for scene"
        );
        assert!(
            !mesh.indices.is_empty(),
            "MeshBuffer must contain indices for scene"
        );
        assert!(
            lighting.num_point_lights >= 2,
            "Scene should include player and NPC lights"
        );

        // Dead actors remain renderable for snapshot continuity, but stop
        // contributing creature light pools.
        world.entities[0].stats.hp = 0;
        let mut dead_mesh = MeshBuffer::new();
        let mut dead_batches = SceneBatches::default();
        let (_, dead_lighting) = GpuRenderer::populate_scene(
            40,
            20,
            WeatherMode::Rain,
            &world,
            20,
            0.5,
            &mut dead_mesh,
            &mut dead_batches,
        );
        assert!(
            dead_lighting.num_point_lights < lighting.num_point_lights,
            "dead monster should not cast a dynamic point-light pool"
        );
    }

    #[test]
    fn test_pine_tree_trunk_and_root_flares_geometry() {
        let tree_proto = MeshBuffer::create_pine_tree_prototype();
        assert!(
            !tree_proto.vertices.is_empty(),
            "Pine tree prototype must contain vertices"
        );
        assert!(
            !tree_proto.indices.is_empty(),
            "Pine tree prototype must contain indices"
        );

        // Verify trunk ground contact reaches down to y <= 0.0
        let min_y = tree_proto
            .vertices
            .iter()
            .map(|v| v.position[1])
            .fold(f32::INFINITY, f32::min);
        assert!(
            min_y <= 0.05,
            "Pine tree trunk/roots must touch the ground plane: min_y={min_y}"
        );

        // Verify maximum tree apex reaches above 7.0 for tall towering silhouette
        let max_y = tree_proto
            .vertices
            .iter()
            .map(|v| v.position[1])
            .fold(f32::NEG_INFINITY, f32::max);
        assert!(
            max_y >= 7.0,
            "Pine tree apex should be towering: max_y={max_y}"
        );

        // Verify root flare lateral footprint spreads outwards for grounded stability
        let max_x = tree_proto
            .vertices
            .iter()
            .map(|v| v.position[0].abs())
            .fold(0.0f32, f32::max);
        assert!(
            max_x >= 1.4,
            "Pine tree root flares / branches must spread laterally: max_x={max_x}"
        );

        // Verify direct mesh buffer add_pine_tree
        let mut mesh = MeshBuffer::new();
        mesh.add_pine_tree([0.0, 0.0, 0.0], 0.0);
        assert!(!mesh.vertices.is_empty());
    }

    #[test]
    fn test_smooth_camera_and_player_transitions() {
        let Ok(mut renderer) = GpuRenderer::new(40, 20, WeatherMode::Rain) else {
            eprintln!("Skipping test_smooth_camera_and_player_transitions: no GPU adapter on host");
            return;
        };
        let mut world = sample_world();
        world.player.position = Point { x: 0, y: 0 };

        // Frame 1: Initial position at (0, 0)
        let (cam1, _) = renderer.build_scene_frame(&world, 0);
        let cam_pos1 = glam::Vec3::from_slice(&cam1.camera_pos[0..3]);
        assert_eq!(renderer.smooth_cam_pos, Some(glam::Vec2::new(0.0, 0.0)));
        assert_eq!(renderer.smooth_player_pos, Some(glam::Vec2::new(0.0, 0.0)));

        // Player takes a step to (10, 0) -> target x = 8.0
        world.player.position = Point { x: 10, y: 0 };
        let (cam2, _) = renderer.build_scene_frame(&world, 1);
        let cam_pos2 = glam::Vec3::from_slice(&cam2.camera_pos[0..3]);

        // Camera must smoothly interpolate rather than instantaneously jumping all 8 units
        assert!(cam_pos2.x > cam_pos1.x);
        assert!(cam_pos2.x < 8.0); // Smooth dampening prevents instantaneous 1-frame snap
        assert!(renderer.smooth_player_pos.unwrap().x < 8.0);
    }

    #[test]
    fn damping_is_stable_across_fixed_substeps() {
        let rate = 19.7;
        let one_step = damping_factor(rate, 1.0 / 30.0);
        let two_steps = 1.0 - (1.0 - damping_factor(rate, 1.0 / 60.0)).powi(2);
        assert!((one_step - two_steps).abs() < 1e-6);
        assert_eq!(damping_factor(rate, 0.0), 0.0);
    }

    #[test]
    fn attack_messages_identify_player_and_remote_attackers() {
        let world = sample_world();
        assert_eq!(
            GpuRenderer::attack_actor_from_message("You hit Mossling for 5 damage.", &world),
            Some(world.player.id)
        );
        assert_eq!(
            GpuRenderer::attack_actor_from_message("Combat 2 -> 1: 3 damage", &world),
            Some(2)
        );
        assert_eq!(
            GpuRenderer::attack_actor_from_message("Combat 99 -> 1: 3 damage", &world),
            None
        );
        assert_eq!(attack_phase_at(0.0, ATTACK_PHASE_SECONDS), 0.0);
        assert!(
            (attack_phase_at(ATTACK_PHASE_SECONDS * 0.5, ATTACK_PHASE_SECONDS) - 0.5).abs() < 1e-6
        );
        assert_eq!(
            attack_phase_at(ATTACK_PHASE_SECONDS * 2.0, ATTACK_PHASE_SECONDS),
            1.0
        );
    }

    #[test]
    fn ssaa_is_fixed_4x_for_every_grid_size() {
        // The former adaptive 2x tier broke the compute pass's 4x-locked tap
        // grid (half-block coverage artifact); SSAA is now 4x everywhere.
        assert_eq!(crate::gpu::pipeline::adaptive_ssaa(80, 30), 4);
        assert_eq!(crate::gpu::pipeline::adaptive_ssaa(128, 36), 4);
        assert_eq!(crate::gpu::pipeline::adaptive_ssaa(220, 70), 4);
    }

    #[test]
    fn display_output_matches_terminal_size_despite_capped_render() {
        let Ok(mut renderer) = GpuRenderer::new(220, 70, WeatherMode::Rain) else {
            eprintln!("Skipping: no GPU adapter on host");
            return;
        };
        // Terminal dims stay visible for the composed HUD frame.
        assert_eq!((renderer.width, renderer.height), (220, 70));
        // The GPU renders the scene partition (220 - 28 side panel cols,
        // 70 - 8 HUD rows) so its blit fills the partition exactly; the HUD
        // bars fill the remainder. Never a downscaled render either.
        assert_eq!(
            (renderer.pipeline.width, renderer.pipeline.height),
            (192, 62),
            "GPU must render the scene partition at native cell resolution"
        );
        assert_eq!(renderer.pipeline.ssaa, 4);
        let world = sample_world();
        let frame = renderer.render_frame(&world, 0);
        // 70 scene rows + HUD block must still fill the display grid.
        assert!(frame.lines().count() >= 70, "output must fill display grid");
    }

    #[test]
    fn scene_partition_dims_match_composed_layout() {
        // Wide terminal: side panel consumes 28 columns, HUD bars 8 rows.
        assert_eq!(crate::ui::scene_partition_dims(120, 30), (92, 22));
        // Narrow terminal: no side panel; the blit keeps the full width.
        assert_eq!(crate::ui::scene_partition_dims(60, 30), (60, 22));
        // Minimums stay safe; the composed frame clamps to the same bounds.
        assert_eq!(crate::ui::scene_partition_dims(40, 10), (40, 4));
    }

    #[test]
    fn paused_gpu_frame_renders_pause_menu() {
        let Ok(mut renderer) = GpuRenderer::new(80, 30, WeatherMode::Rain) else {
            eprintln!("Skipping: no GPU adapter on host");
            return;
        };
        renderer.active_modal = crate::ui::ActiveModal::Paused;
        let world = sample_world();
        let frame = renderer.render_frame(&world, 0);
        assert!(frame.contains("PAUSED"), "paused frame must show the menu");
        assert!(frame.contains("Resume play"));
    }

    #[test]
    fn ocean_renders_when_the_player_reaches_the_south_map_edge() {
        // The southern ocean lives beyond WORLD_BOUNDS.max_y (mesh z = 28.0
        // at the renderer's 0.8 scale). A player standing near the boundary
        // must get open water inside the terrain radius.
        let mut world = sample_world();
        world.player.position = Point { x: 2, y: 33 };
        let mut mesh = MeshBuffer::new();
        let mut batches = SceneBatches::new();
        let _ = GpuRenderer::populate_scene(
            128,
            36,
            WeatherMode::Rain,
            &world,
            0,
            0.0,
            &mut mesh,
            &mut batches,
        );
        let water = mesh
            .vertices
            .iter()
            .filter(|v| v.material_id >= 5.8 && v.material_id <= 6.2)
            .count();
        assert!(water > 0, "ocean geometry must exist south of the boundary");

        let shore = 28.0;
        for v in mesh
            .vertices
            .iter()
            .filter(|v| v.material_id >= 5.8 && v.material_id <= 6.2)
        {
            assert!(
                v.position[2] >= shore - 1e-4,
                "water must stay south of the shoreline: {:?}",
                v.position
            );
        }

        // Interior semantics: the terrain window always extends to its
        // radius, so even the inland sample world gets far-south water
        // vertices — but they must start well beyond the meadow view, while
        // the near-edge build above gets water within a screen-reachable
        // band south of the player (player z = 33 * 0.8 = 26.4).
        let mut inland = MeshBuffer::new();
        let mut inland_batches = SceneBatches::new();
        let _ = GpuRenderer::populate_scene(
            128,
            36,
            WeatherMode::Rain,
            &sample_world(),
            0,
            0.0,
            &mut inland,
            &mut inland_batches,
        );
        let interior_player_z = 2.0 * 0.8;
        let water_z = |mesh: &MeshBuffer| {
            mesh.vertices
                .iter()
                .filter(|v| v.material_id >= 5.8 && v.material_id <= 6.2)
                .map(|v| v.position[2])
                .fold(f32::NAN, f32::min)
        };
        let near_min_z = water_z(&mesh);
        assert!(
            !near_min_z.is_nan() && near_min_z <= 26.4 + 4.0,
            "near-edge player must get water within a few cells south"
        );
        let inland_min_z = water_z(&inland);
        assert!(
            !inland_min_z.is_nan() && inland_min_z > 28.0 && inland_min_z < 30.0,
            "inland terrain window must contain water only at the boundary rows, got min z {inland_min_z}"
        );
        assert!(
            inland_min_z > interior_player_z + 24.0,
            "inland ocean water should sit far south of the meadow view: {inland_min_z}"
        );
    }

    #[test]
    fn shore_view_outputs_rolling_wave_glyph_language() {
        // End-to-end ASCII proof: standing on the beach, the south half of
        // the scene rasterizes into the material-6 wave language ('~' '='
        // ramp lines and '*' foam caps driven by the rolling swell + foam
        // shading), far more often than the meadow-interior view, which
        // cannot see the ocean at all. Two frames built in one process keep
        // the known run-to-run GPU tie-flip noise irrelevant at the margin.
        let Ok(mut edge) = GpuRenderer::new(120, 30, WeatherMode::Rain) else {
            eprintln!("Skipping shore_view_outputs_rolling_wave_glyph_language: no GPU adapter");
            return;
        };
        let mut shore_world = sample_world();
        shore_world.player.position = Point { x: 2, y: 33 }; // ~2 cells north of the shoreline
        let shore_frame = edge.render_frame(&shore_world, 0);
        drop(edge);

        let Ok(mut inland) = GpuRenderer::new(120, 30, WeatherMode::Rain) else {
            eprintln!("Skipping shore_view_outputs_rolling_wave_glyph_language: no GPU adapter");
            return;
        };
        let inland_frame = inland.render_frame(&sample_world(), 0);
        drop(inland);

        // Strip ANSI payloads (both color "…m" and cursor "…H" sequences)
        // crudely: everything after the first letter of the sequence body is
        // frame payload text; digits that survive malformed splits contain
        // no wave-language glyphs.
        let wave_count = |frame: &str| {
            frame
                .split("\x1b[")
                .map(|chunk| chunk.split_once('m').map(|(_, rest)| rest).unwrap_or(chunk))
                .flat_map(|text| text.chars())
                .filter(|c| matches!(c, '~' | '=' | '*'))
                .count()
        };
        let shore_waves = wave_count(&shore_frame);
        let inland_waves = wave_count(&inland_frame);
        eprintln!("wave glyph counts: shore {shore_waves} vs inland {inland_waves}");
        assert!(
            shore_waves > inland_waves + 200,
            "rolling wave/foam language must dominate the shoreline view: shore {shore_waves} vs inland {inland_waves}"
        );
    }

    #[test]
    fn scene_labels_project_onto_actor_cells() {
        // Character-clarity pass: labels use the same view_proj the mesh
        // pass used, so plates land on heads. The melee-range Mossling is
        // the marked target (the rule mirrors Command::Attack), the
        // out-of-range Skeleton is not, and the player's plate skips the
        // HP row (the top HUD bar already shows vitals).
        let eye = Vec3::new(0.9, 16.1, 13.4);
        let view = Mat4::look_at_rh(eye, Vec3::new(3.2, 1.0, 1.6), Vec3::Y);
        let proj = Mat4::perspective_rh(34.0f32.to_radians(), 0.48, 0.1, 100.0);
        let camera_uniforms = CameraUniforms {
            view_proj: (proj * view).to_cols_array_2d(),
            inv_view_proj: [[0.0; 4]; 4],
            camera_pos: [eye.x, eye.y, eye.z, 0.0],
            time: 0.0,
            weather_mode: 0,
            width: 100,
            height: 40,
        };
        let mut world = sample_world();
        // The shipped 34°/0.48 camera frame is narrow; pull the second mob
        // near-frame for the projection assertions while keeping it outside
        // melee range (manhattan 4 vs the Mossling's 1).
        world.entities[1].position = Point { x: 6, y: 2 };
        let labels = GpuRenderer::compute_scene_labels(&world, None, &camera_uniforms, 100.0, 40.0);

        let player = labels.iter().find(|l| l.is_player).expect("player label");
        assert!(
            (15.0..=30.0).contains(&player.col),
            "player feet project near the camera focus column: {player:?}"
        );
        assert!(player.head_row < player.feet_row, "head above feet");
        assert!(player.hp.is_none(), "player plates skip the HP row");

        let mossling = labels
            .iter()
            .find(|l| l.name == "Mossling")
            .expect("melee monster label");
        assert!(
            mossling.is_target,
            "melee-range monster is the marked target"
        );
        assert_eq!(mossling.hp, Some((8, 8)), "monster plates carry HP digits");

        let skeleton = labels
            .iter()
            .find(|l| l.name == "Skeleton")
            .expect("far monster label");
        assert!(
            !skeleton.is_target,
            "out-of-range monster is not the target"
        );
        assert!(
            (65.0..=90.0).contains(&skeleton.col),
            "second mob projects on the opposite flank: {skeleton:?}"
        );

        assert!(
            labels
                .iter()
                .all(|l| (0.0..=100.0).contains(&l.col) && (0.0..=40.0).contains(&l.feet_row)),
            "labels stay on the scene grid"
        );
    }

    #[test]
    fn combat_messages_translate_into_fx_specs() {
        let world = sample_world();
        let focus = Vec2::new(1.6, 1.6);

        let hit = GpuRenderer::combat_fx_from_message(
            "You hit Mossling for 5 damage.",
            &world,
            focus,
            3.0,
        )
        .expect("hit message spawns impact FX");
        assert_eq!(hit.pos, [2.4, 0.15, 1.6], "burst anchors at the target");
        assert_eq!(hit.telegraph, 0.0, "melee hits burst instantly");
        assert_eq!(hit.born, 3.0, "FX timelines key off animation time");

        let cast = GpuRenderer::combat_fx_from_message(
            "You cast Cleave; spell effects are queued for the next combat slice.",
            &world,
            focus,
            4.0,
        )
        .expect("cast message spawns telegraph FX");
        assert!(cast.telegraph > 0.0, "casts sweep a pre-hit ring first");
        assert_eq!(cast.pos, [focus.x, 0.15, focus.y], "caster is the anchor");

        let thunder = GpuRenderer::combat_fx_from_message(
            "You cast Thunder Mark; spell effects are queued for the next combat slice.",
            &world,
            focus,
            5.0,
        )
        .expect("thunder message spawns FX");
        assert_ne!(thunder.color, cast.color, "single hue per spell school");
        assert!(
            thunder.radius > cast.radius,
            "AoE radius (2 cells) out-scales the melee arc"
        );
        assert_ne!(cast.color, hit.color, "spell arcs differ from melee embers");

        assert_eq!(
            GpuRenderer::combat_fx_from_message("You hit Nobody for 5 damage.", &world, focus, 6.0),
            None,
            "hits against unknown names stay quiet"
        );
        assert_eq!(
            GpuRenderer::combat_fx_from_message("Blocked by a pine.", &world, focus, 7.0),
            None,
            "non-combat lines stay quiet"
        );
    }

    #[test]
    fn combat_fx_spawn_once_and_expire_from_snapshot_messages() {
        let Ok(mut renderer) = GpuRenderer::new(60, 24, WeatherMode::Snow) else {
            eprintln!("Skipping combat_fx_spawn_once_and_expire: no GPU adapter on host");
            return;
        };
        let mut world = sample_world();
        world.messages.push("You hit Mossling for 5 damage.".into());

        let _ = renderer.build_scene_frame_with_timing(&world, 0, 0.05, 1.0, false);
        assert_eq!(
            renderer.combat_fx.len(),
            1,
            "hit message spawns exactly one FX"
        );
        assert_eq!(renderer.combat_fx[0].pos, [2.4, 0.15, 1.6]);
        assert!(
            renderer.mesh.vertices.iter().any(|v| {
                v.entity_id == FX_ENTITY_ID as f32
                    && v.material_id == 4.0
                    && (v.position[0] - 2.4).abs() < 1.0
                    && (v.position[2] - 1.6).abs() < 1.0
            }),
            "burst geometry is emissive FX-ink around the target"
        );

        // Replaying the same snapshot must not double-spawn...
        let _ = renderer.build_scene_frame_with_timing(&world, 1, 0.05, 1.05, false);
        assert_eq!(renderer.combat_fx.len(), 1, "deduplicated by message");

        // ...and expired bursts drop off the stack.
        let _ = renderer.build_scene_frame_with_timing(&world, 2, 0.05, 2.0, false);
        assert!(
            renderer.combat_fx.is_empty(),
            "FX lifetime is telegraph + burst seconds"
        );
    }

    #[test]
    fn impact_burst_follows_core_then_ring_then_ember_timeline() {
        let mut mesh = MeshBuffer::new();
        mesh.set_entity_context(FX_ENTITY_ID, 3);
        let center = [5.0, 0.15, 7.0];
        let color = [0.98, 0.55, 0.16, 1.0];

        mesh.add_impact_burst(center, 0.0, color, 0.8);
        let early_len = mesh.vertices.len();
        assert!(early_len > 0, "progress 0 already shows the solid core");
        assert!(
            mesh.vertices.iter().all(|v| {
                v.material_id == 4.0 && v.entity_id == FX_ENTITY_ID as f32 && v.position[1] < 0.6
            }),
            "early burst is a compact low emissive mass"
        );

        mesh.add_impact_burst(center, 0.95, color, 0.8);
        let late_count = mesh.vertices.len() - early_len;
        assert!(late_count > 0, "late burst adds race + ember ink");
        let mut early_top = f32::MIN;
        for vertex in &mesh.vertices[..early_len] {
            early_top = early_top.max(vertex.position[1]);
        }
        let mut late_top = f32::MIN;
        let mut late_bottom = f32::MAX;
        for vertex in &mesh.vertices[early_len..] {
            late_top = late_top.max(vertex.position[1]);
            late_bottom = late_bottom.min(vertex.position[1]);
        }
        assert!(
            late_top > early_top + 0.4,
            "embers rise well above the initial core ({late_top} vs {early_top})"
        );
        assert!(late_bottom > -0.01, "FX never sinks under the ground plane");

        mesh.add_ground_ring(center, 1.4, color, 0.5, 10);
        let ring: Vec<_> = mesh.vertices[early_len + late_count..]
            .iter()
            .map(|v| {
                let dx = v.position[0] - center[0];
                let dz = v.position[2] - center[2];
                ((dx * dx + dz * dz).sqrt(), v.position[1])
            })
            .collect();
        assert!(!ring.is_empty(), "telegraph ring stamps dashes");
        for (dist, y) in ring {
            assert!(
                (1.0..=1.75).contains(&dist),
                "dash sits on the ring band: {dist}"
            );
            assert!(y < 0.4, "dashes hug the ground");
        }
    }
}
