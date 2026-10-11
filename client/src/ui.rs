//! Ratatui-powered Game HUD and UI presentation layer.
//!
//! Provides rich widgets, TrueColor gradient health gauges, action & combat logs,
//! zone navigation banners, and responsive layout management adapted from Ratatui.

use ascii_mmorpg_shared::{EntityKind, WorldSnapshot};
use ratatui::buffer::Buffer;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Gauge, Paragraph, Widget};

use crate::renderer::WeatherMode;

/// Interactive overlay modal window modes (CmdMMO-inspired windowed TUI).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ActiveModal {
    #[default]
    None,
    /// Carried gear, equipped weapons/armor, satchel items, potions, and gold.
    Inventory,
    /// Attributes (STR, DEX, CON, INT, LUCK), level, experience, and active buffs.
    CharacterSheet,
    /// 2D Overworld radar map of the Whispering Forest and landmark positions.
    WorldMap,
}

/// Comprehensive dark fantasy MMORPG HUD powered by Ratatui.
pub struct RatatuiHud {
    /// Recent event and combat log messages
    pub log_history: Vec<String>,
}

impl Default for RatatuiHud {
    fn default() -> Self {
        Self::new()
    }
}

impl RatatuiHud {
    pub fn new() -> Self {
        Self {
            log_history: Vec::new(),
        }
    }

    /// Appends new snapshot messages to persistent log history.
    pub fn update_messages(&mut self, messages: &[String]) {
        for msg in messages {
            if !self.log_history.contains(msg) {
                self.log_history.push(msg.clone());
            }
        }
        if self.log_history.len() > 50 {
            let drain_count = self.log_history.len() - 50;
            self.log_history.drain(0..drain_count);
        }
    }

    /// Renders the complete HUD overlay around or below the scene using Ratatui layout and widgets.
    pub fn render_hud_overlay(
        &mut self,
        world: &WorldSnapshot,
        width: u16,
        height: u16,
        weather: WeatherMode,
        backend_name: &str,
    ) -> String {
        self.render_hud_overlay_with_modal(
            world,
            width,
            height,
            weather,
            backend_name,
            ActiveModal::None,
        )
    }

    /// Renders the complete HUD overlay with an optional active interactive modal window.
    pub fn render_hud_overlay_with_modal(
        &mut self,
        world: &WorldSnapshot,
        width: u16,
        height: u16,
        weather: WeatherMode,
        backend_name: &str,
        modal: ActiveModal,
    ) -> String {
        self.update_messages(&world.messages);

        let area = Rect::new(0, 0, width.max(40), height.max(8));
        let mut buf = Buffer::empty(area);

        // Layout: Top Status & Stats (3 rows) -> Middle Combat Log or Modal -> Bottom Hotkey Legend (1 row)
        let main_layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3), // Top Navigation & Vitality Bar
                Constraint::Min(3),    // Event / Combat Log or Active Modal
                Constraint::Length(1), // Hotkey / Keybindings Legend
            ])
            .split(area);

        self.render_top_bar(&mut buf, main_layout[0], world, weather, backend_name);
        match modal {
            ActiveModal::None => self.render_combat_log(&mut buf, main_layout[1], world),
            ActiveModal::Inventory => self.render_inventory_modal(&mut buf, main_layout[1], world),
            ActiveModal::CharacterSheet => {
                self.render_character_sheet_modal(&mut buf, main_layout[1], world)
            }
            ActiveModal::WorldMap => self.render_world_map_modal(&mut buf, main_layout[1], world),
        }
        self.render_controls_legend(&mut buf, main_layout[2], modal != ActiveModal::None);

        buffer_to_ansi(&buf)
    }

    /// Renders the top navigation header and health gauge.
    fn render_top_bar(
        &self,
        buf: &mut Buffer,
        area: Rect,
        world: &WorldSnapshot,
        weather: WeatherMode,
        backend_name: &str,
    ) {
        let (weather_str, weather_color) = match weather {
            WeatherMode::Rain => ("🌧 Rain", Color::Rgb(100, 180, 240)),
            WeatherMode::Snow => ("❄ Snow", Color::Rgb(220, 240, 255)),
            WeatherMode::Storm => ("⚡ Storm", Color::Rgb(255, 220, 80)),
        };

        let p = &world.player;
        let hp_ratio = if p.stats.max_hp > 0 {
            (p.stats.hp as f64 / p.stats.max_hp as f64).clamp(0.0, 1.0)
        } else {
            0.0
        };

        let hp_color = if hp_ratio > 0.5 {
            Color::Rgb(40, 200, 100) // Emerald Green
        } else if hp_ratio > 0.25 {
            Color::Rgb(230, 170, 40) // Amber Gold
        } else {
            Color::Rgb(220, 40, 40) // Crimson Alert
        };

        let title_line = Line::from(vec![
            Span::styled(
                "🌲 Whispering Forest ",
                Style::default()
                    .fg(Color::Rgb(220, 190, 120))
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("📍 ({}, {}) ", p.position.x, p.position.y),
                Style::default().fg(Color::Rgb(140, 180, 210)),
            ),
            Span::styled(
                format!("[{weather_str}] "),
                Style::default().fg(weather_color),
            ),
            Span::styled(
                format!("Tick {} ", world.tick),
                Style::default().fg(Color::Rgb(160, 160, 160)),
            ),
            Span::styled(
                format!("• {backend_name}"),
                Style::default().fg(Color::Rgb(100, 210, 255)),
            ),
        ]);

        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(Color::Rgb(90, 80, 70)))
            .title(title_line);

        let inner_area = block.inner(area);
        block.render(area, buf);

        // Split inner into: Health Gauge (Left) and Combat Attributes (Right)
        let inner_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(45), // Health Gauge
                Constraint::Percentage(55), // Combat Attributes
            ])
            .split(inner_area);

        // Health Gauge with fractional unicode blocks
        let gauge = Gauge::default()
            .gauge_style(Style::default().fg(hp_color).bg(Color::Rgb(30, 20, 25)))
            .ratio(hp_ratio)
            .label(format!("HP: {}/{}", p.stats.hp, p.stats.max_hp));
        gauge.render(inner_chunks[0], buf);

        // Combat Stats Card
        let stats_line = Line::from(vec![
            Span::styled(
                " [⚔ WARRIOR] ",
                Style::default()
                    .fg(Color::Rgb(230, 235, 245))
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("HP {}/{} ", p.stats.hp, p.stats.max_hp),
                Style::default()
                    .fg(Color::Rgb(255, 255, 255))
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("ATK: ", Style::default().fg(Color::Rgb(160, 160, 160))),
            Span::styled(
                format!("{:<2} ", p.stats.attack),
                Style::default()
                    .fg(Color::Rgb(255, 120, 120))
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("DEF: ", Style::default().fg(Color::Rgb(160, 160, 160))),
            Span::styled(
                format!("{:<2} ", p.stats.defense),
                Style::default()
                    .fg(Color::Rgb(120, 180, 255))
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("LUCK: ", Style::default().fg(Color::Rgb(160, 160, 160))),
            Span::styled(
                format!("{:<2} ", p.stats.luck),
                Style::default()
                    .fg(Color::Rgb(255, 220, 80))
                    .add_modifier(Modifier::BOLD),
            ),
        ]);
        Paragraph::new(stats_line).render(inner_chunks[1], buf);
    }

    /// Renders the action and combat log widget with semantic event coloring and nearby target scanning.
    fn render_combat_log(&self, buf: &mut Buffer, area: Rect, world: &WorldSnapshot) {
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(70), // Action & Combat Log
                Constraint::Percentage(30), // Nearby Threats / Target Card
            ])
            .split(area);

        // 1. Action & Combat Log
        let log_block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(Color::Rgb(80, 75, 70)))
            .title(Span::styled(
                " 📜 Action & Combat Log ",
                Style::default()
                    .fg(Color::Rgb(210, 175, 110))
                    .add_modifier(Modifier::BOLD),
            ));

        let inner_log = log_block.inner(chunks[0]);
        log_block.render(chunks[0], buf);

        let visible_lines = inner_log.height as usize;
        let mut styled_lines = Vec::new();

        let msgs: Vec<&String> = if !self.log_history.is_empty() {
            self.log_history.iter().collect()
        } else {
            world.messages.iter().collect()
        };

        let start_idx = msgs.len().saturating_sub(visible_lines);
        for &msg in &msgs[start_idx..] {
            let (prefix, color) = if msg.contains("hit") || msg.contains("damage") {
                ("⚔ ", Color::Rgb(255, 130, 80))
            } else if msg.contains("slain") || msg.contains("died") {
                ("💀 ", Color::Rgb(240, 80, 120))
            } else if msg.contains("Blocked") {
                ("⚠ ", Color::Rgb(240, 190, 70))
            } else if msg.contains("Connected") || msg.contains("ready") {
                ("✨ ", Color::Rgb(100, 210, 255))
            } else {
                ("• ", Color::Rgb(180, 190, 205))
            };

            styled_lines.push(Line::from(vec![
                Span::styled(
                    prefix,
                    Style::default().fg(color).add_modifier(Modifier::BOLD),
                ),
                Span::styled(msg.as_str(), Style::default().fg(color)),
            ]));
        }

        Paragraph::new(styled_lines).render(inner_log, buf);

        // 2. Target Radar / Nearby Threat Card
        let threat_block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(Color::Rgb(80, 75, 70)))
            .title(Span::styled(
                " 🎯 Nearby Threats ",
                Style::default()
                    .fg(Color::Rgb(220, 140, 120))
                    .add_modifier(Modifier::BOLD),
            ));

        let inner_threat = threat_block.inner(chunks[1]);
        threat_block.render(chunks[1], buf);

        let mut threat_spans = Vec::new();
        let mut found_monster = false;

        for e in &world.entities {
            if e.kind == EntityKind::Monster && e.stats.is_alive() {
                let dx = e.position.x - world.player.position.x;
                let dy = e.position.y - world.player.position.y;
                let dist = (dx.abs() + dy.abs()).max(1);

                threat_spans.push(Line::from(vec![
                    Span::styled("💀 ", Style::default().fg(Color::Rgb(240, 90, 90))),
                    Span::styled(
                        format!("{} ({}m)", e.name, dist),
                        Style::default()
                            .fg(Color::Rgb(230, 230, 230))
                            .add_modifier(Modifier::BOLD),
                    ),
                ]));

                let enemy_ratio = if e.stats.max_hp > 0 {
                    (e.stats.hp as f64 / e.stats.max_hp as f64).clamp(0.0, 1.0)
                } else {
                    0.0
                };

                let hp_bar_len = inner_threat.width.saturating_sub(12) as usize;
                let filled = (enemy_ratio * hp_bar_len as f64).round() as usize;
                let empty = hp_bar_len.saturating_sub(filled);

                threat_spans.push(Line::from(vec![
                    Span::styled("   [", Style::default().fg(Color::Rgb(120, 120, 120))),
                    Span::styled(
                        "█".repeat(filled),
                        Style::default().fg(Color::Rgb(220, 60, 60)),
                    ),
                    Span::styled(
                        "░".repeat(empty),
                        Style::default().fg(Color::Rgb(60, 40, 40)),
                    ),
                    Span::styled(
                        format!("] {}/{}", e.stats.hp, e.stats.max_hp),
                        Style::default().fg(Color::Rgb(180, 180, 180)),
                    ),
                ]));

                found_monster = true;
                if threat_spans.len() >= inner_threat.height as usize {
                    break;
                }
            }
        }

        if !found_monster {
            threat_spans.push(Line::from(vec![Span::styled(
                "✨ Area peaceful.",
                Style::default().fg(Color::Rgb(120, 180, 140)),
            )]));
            threat_spans.push(Line::from(vec![Span::styled(
                "No hostile creatures nearby.",
                Style::default().fg(Color::Rgb(120, 120, 120)),
            )]));
        }

        Paragraph::new(threat_spans).render(inner_threat, buf);
    }

    /// Renders the Inventory modal window.
    fn render_inventory_modal(&self, buf: &mut Buffer, area: Rect, _world: &WorldSnapshot) {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Double)
            .border_style(Style::default().fg(Color::Rgb(215, 175, 95)))
            .title(Span::styled(
                " 🎒 Inventory & Equipment [I to Close] ",
                Style::default()
                    .fg(Color::Rgb(255, 215, 115))
                    .add_modifier(Modifier::BOLD),
            ));
        let inner = block.inner(area);
        block.render(area, buf);

        let inv_layout = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(inner);

        // Column 1: Equipped Gear
        let equip_lines = vec![
            Line::from(vec![Span::styled(
                "⚔ Equipped Gear:",
                Style::default()
                    .fg(Color::Rgb(220, 190, 120))
                    .add_modifier(Modifier::BOLD),
            )]),
            Line::from(vec![
                Span::styled(
                    "  Main Hand: ",
                    Style::default().fg(Color::Rgb(160, 160, 160)),
                ),
                Span::styled(
                    "Steel Broadsword (+6 ATK)",
                    Style::default()
                        .fg(Color::Rgb(225, 232, 240))
                        .add_modifier(Modifier::BOLD),
                ),
            ]),
            Line::from(vec![
                Span::styled(
                    "  Off Hand:  ",
                    Style::default().fg(Color::Rgb(160, 160, 160)),
                ),
                Span::styled(
                    "Heater Shield (+4 DEF)",
                    Style::default().fg(Color::Rgb(180, 200, 220)),
                ),
            ]),
            Line::from(vec![
                Span::styled(
                    "  Armor:     ",
                    Style::default().fg(Color::Rgb(160, 160, 160)),
                ),
                Span::styled(
                    "Fluted Plate (+8 DEF)",
                    Style::default().fg(Color::Rgb(180, 200, 220)),
                ),
            ]),
            Line::from(vec![
                Span::styled(
                    "  Helm:      ",
                    Style::default().fg(Color::Rgb(160, 160, 160)),
                ),
                Span::styled(
                    "Knight Greathelm (+3 DEF)",
                    Style::default().fg(Color::Rgb(180, 200, 220)),
                ),
            ]),
        ];
        Paragraph::new(equip_lines).render(inv_layout[0], buf);

        // Column 2: Backpack & Satchel
        let backpack_lines = vec![
            Line::from(vec![Span::styled(
                "📦 Satchel Items:",
                Style::default()
                    .fg(Color::Rgb(220, 190, 120))
                    .add_modifier(Modifier::BOLD),
            )]),
            Line::from(vec![
                Span::styled("  [1] ", Style::default().fg(Color::Rgb(240, 200, 100))),
                Span::styled(
                    "Healing Draught x3",
                    Style::default().fg(Color::Rgb(100, 220, 140)),
                ),
                Span::styled(
                    " (Restores 15 HP)",
                    Style::default().fg(Color::Rgb(120, 120, 120)),
                ),
            ]),
            Line::from(vec![
                Span::styled("  [2] ", Style::default().fg(Color::Rgb(240, 200, 100))),
                Span::styled(
                    "Scroll of Town Portal",
                    Style::default().fg(Color::Rgb(140, 180, 255)),
                ),
            ]),
            Line::from(vec![
                Span::styled("  [3] ", Style::default().fg(Color::Rgb(240, 200, 100))),
                Span::styled(
                    "Ancient Fortress Key",
                    Style::default().fg(Color::Rgb(255, 215, 115)),
                ),
            ]),
            Line::from(vec![
                Span::styled("  Gold: ", Style::default().fg(Color::Rgb(160, 160, 160))),
                Span::styled(
                    "148 coins  ",
                    Style::default()
                        .fg(Color::Rgb(255, 215, 75))
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled("Weight: ", Style::default().fg(Color::Rgb(160, 160, 160))),
                Span::styled(
                    "18.5 / 50.0 st",
                    Style::default().fg(Color::Rgb(200, 200, 200)),
                ),
            ]),
        ];
        Paragraph::new(backpack_lines).render(inv_layout[1], buf);
    }

    /// Renders the Character Sheet & Attributes modal window.
    fn render_character_sheet_modal(&self, buf: &mut Buffer, area: Rect, world: &WorldSnapshot) {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Double)
            .border_style(Style::default().fg(Color::Rgb(140, 180, 230)))
            .title(Span::styled(
                " 📜 Character Sheet & Attributes [C to Close] ",
                Style::default()
                    .fg(Color::Rgb(180, 215, 255))
                    .add_modifier(Modifier::BOLD),
            ));
        let inner = block.inner(area);
        block.render(area, buf);

        let char_layout = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(inner);

        let p = &world.player;
        let stats_col = vec![
            Line::from(vec![
                Span::styled(
                    format!("🛡 {}", p.name),
                    Style::default()
                        .fg(Color::Rgb(240, 210, 140))
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    " - Paladin Champion (Lvl 3)",
                    Style::default().fg(Color::Rgb(160, 160, 160)),
                ),
            ]),
            Line::from(vec![
                Span::styled(
                    "  Experience: ",
                    Style::default().fg(Color::Rgb(160, 160, 160)),
                ),
                Span::styled(
                    "340 / 500 XP [███████░░░]",
                    Style::default().fg(Color::Rgb(120, 190, 255)),
                ),
            ]),
            Line::from(vec![
                Span::styled(
                    format!("  Vitality:   {}/{} HP", p.stats.hp, p.stats.max_hp),
                    Style::default().fg(Color::Rgb(100, 220, 140)),
                ),
                Span::styled(
                    "  Mana: 12/12 MP",
                    Style::default().fg(Color::Rgb(100, 170, 240)),
                ),
            ]),
            Line::from(vec![Span::styled(
                format!(
                    "  Attack:     {}  |  Defense: {}",
                    p.stats.attack, p.stats.defense
                ),
                Style::default().fg(Color::Rgb(225, 232, 240)),
            )]),
        ];
        Paragraph::new(stats_col).render(char_layout[0], buf);

        let attr_col = vec![
            Line::from(vec![Span::styled(
                "⚔ Attributes & Modifiers:",
                Style::default()
                    .fg(Color::Rgb(220, 190, 120))
                    .add_modifier(Modifier::BOLD),
            )]),
            Line::from(vec![Span::styled(
                "  Strength:     14 (+2 DMG)   Agility: 11 (+1 EVA)",
                Style::default().fg(Color::Rgb(180, 180, 180)),
            )]),
            Line::from(vec![Span::styled(
                "  Constitution: 15 (Hardy)    Arcana:  10 (+0 MAG)",
                Style::default().fg(Color::Rgb(180, 180, 180)),
            )]),
            Line::from(vec![Span::styled(
                format!(
                    "  Luck:         {} (+{}% CRIT)",
                    p.stats.luck,
                    p.stats.luck * 2
                ),
                Style::default().fg(Color::Rgb(240, 200, 100)),
            )]),
            Line::from(vec![Span::styled(
                "  Active Buff: Torch Radiance (+7 Light Radius)",
                Style::default().fg(Color::Rgb(255, 185, 75)),
            )]),
        ];
        Paragraph::new(attr_col).render(char_layout[1], buf);
    }

    /// Renders the 2D Overworld Radar Map modal window.
    fn render_world_map_modal(&self, buf: &mut Buffer, area: Rect, world: &WorldSnapshot) {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Double)
            .border_style(Style::default().fg(Color::Rgb(100, 190, 120)))
            .title(Span::styled(
                " 🗺 Overworld Map: Whispering Forest [M to Close] ",
                Style::default()
                    .fg(Color::Rgb(140, 240, 160))
                    .add_modifier(Modifier::BOLD),
            ));
        let inner = block.inner(area);
        block.render(area, buf);

        let map_layout = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Min(36), Constraint::Length(32)])
            .split(inner);

        let px = world.player.position.x;
        let py = world.player.position.y;

        // Visual radar map grid
        let map_lines = vec![
            Line::from(vec![Span::styled(
                "  +---------------------------------+",
                Style::default().fg(Color::Rgb(80, 120, 90)),
            )]),
            Line::from(vec![Span::styled(
                "  | ♠ ♠ ♠  [==GATE==]  ♠ ♠ ♠  ▲RIDGE|",
                Style::default().fg(Color::Rgb(100, 160, 110)),
            )]),
            Line::from(vec![Span::styled(
                "  | ♠ 🏛RUIN        ☼TORCH    ♠ ♠ ♠ |",
                Style::default().fg(Color::Rgb(160, 150, 120)),
            )]),
            Line::from(vec![Span::styled(
                "  | ~~~~~~~[==BRIDGE==]~~~~~~~~~RIV |",
                Style::default().fg(Color::Rgb(70, 150, 220)),
            )]),
            Line::from(vec![Span::styled(
                "  | ♠ ♠ ♠     ☼TORCH      ♠MEADOW♠  |",
                Style::default().fg(Color::Rgb(80, 140, 90)),
            )]),
            Line::from(vec![Span::styled(
                "  +---------------------------------+",
                Style::default().fg(Color::Rgb(80, 120, 90)),
            )]),
        ];
        Paragraph::new(map_lines).render(map_layout[0], buf);

        let legend_lines = vec![
            Line::from(vec![
                Span::styled(
                    "📍 Position: ",
                    Style::default().fg(Color::Rgb(160, 160, 160)),
                ),
                Span::styled(
                    format!("({}, {})", px, py),
                    Style::default()
                        .fg(Color::Rgb(240, 210, 140))
                        .add_modifier(Modifier::BOLD),
                ),
            ]),
            Line::from(vec![
                Span::styled("🧭 Zone: ", Style::default().fg(Color::Rgb(160, 160, 160))),
                Span::styled(
                    "Ancient Fortress Woods",
                    Style::default().fg(Color::Rgb(140, 190, 230)),
                ),
            ]),
            Line::from(vec![Span::styled(
                "🏛 Ruins: (-8, -5)  |  ⛩ Gate: (2, -12)",
                Style::default().fg(Color::Rgb(180, 170, 150)),
            )]),
            Line::from(vec![Span::styled(
                "🌊 River: y ~ 8     |  ☼ Torches: (-6, 2)",
                Style::default().fg(Color::Rgb(100, 180, 240)),
            )]),
            Line::from(vec![Span::styled(
                format!(
                    "👾 Hostiles: {} in radar reach",
                    world
                        .entities
                        .iter()
                        .filter(|e| e.kind == EntityKind::Monster)
                        .count()
                ),
                Style::default().fg(Color::Rgb(255, 120, 120)),
            )]),
        ];
        Paragraph::new(legend_lines).render(map_layout[1], buf);
    }

    /// Renders the keybinding hotkey legend bar.
    fn render_controls_legend(&self, buf: &mut Buffer, area: Rect, modal_open: bool) {
        let legend = if modal_open {
            Line::from(vec![
                Span::styled(
                    " [Esc/Key] ",
                    Style::default()
                        .fg(Color::Rgb(255, 210, 80))
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    "Close Modal  ",
                    Style::default().fg(Color::Rgb(180, 180, 180)),
                ),
                Span::styled(
                    "[I] ",
                    Style::default()
                        .fg(Color::Rgb(240, 200, 100))
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    "Inventory  ",
                    Style::default().fg(Color::Rgb(160, 160, 160)),
                ),
                Span::styled(
                    "[C] ",
                    Style::default()
                        .fg(Color::Rgb(140, 190, 255))
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    "Character  ",
                    Style::default().fg(Color::Rgb(160, 160, 160)),
                ),
                Span::styled(
                    "[M] ",
                    Style::default()
                        .fg(Color::Rgb(120, 220, 140))
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled("Map  ", Style::default().fg(Color::Rgb(160, 160, 160))),
                Span::styled(
                    "[Q] ",
                    Style::default()
                        .fg(Color::Rgb(220, 100, 100))
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled("Quit", Style::default().fg(Color::Rgb(160, 160, 160))),
            ])
        } else {
            Line::from(vec![
                Span::styled(
                    " [WASD] ",
                    Style::default()
                        .fg(Color::Rgb(240, 200, 100))
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled("Move  ", Style::default().fg(Color::Rgb(160, 160, 160))),
                Span::styled(
                    "[F] ",
                    Style::default()
                        .fg(Color::Rgb(255, 110, 110))
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled("Attack  ", Style::default().fg(Color::Rgb(160, 160, 160))),
                Span::styled(
                    "[R] ",
                    Style::default()
                        .fg(Color::Rgb(120, 220, 140))
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled("Rest  ", Style::default().fg(Color::Rgb(160, 160, 160))),
                Span::styled(
                    "[I] ",
                    Style::default()
                        .fg(Color::Rgb(240, 200, 100))
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled("Inv  ", Style::default().fg(Color::Rgb(160, 160, 160))),
                Span::styled(
                    "[C] ",
                    Style::default()
                        .fg(Color::Rgb(140, 190, 255))
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled("Chars  ", Style::default().fg(Color::Rgb(160, 160, 160))),
                Span::styled(
                    "[M] ",
                    Style::default()
                        .fg(Color::Rgb(120, 220, 140))
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled("Map  ", Style::default().fg(Color::Rgb(160, 160, 160))),
                Span::styled(
                    "[V/T] ",
                    Style::default()
                        .fg(Color::Rgb(120, 190, 255))
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled("Weather  ", Style::default().fg(Color::Rgb(160, 160, 160))),
                Span::styled(
                    "[Q/Esc] ",
                    Style::default()
                        .fg(Color::Rgb(220, 100, 100))
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled("Quit", Style::default().fg(Color::Rgb(160, 160, 160))),
            ])
        };

        Paragraph::new(legend)
            .alignment(Alignment::Center)
            .render(area, buf);
    }
}

/// Converts a Ratatui `Buffer` into an optimized TrueColor ANSI string with dirty-state compression.
pub fn buffer_to_ansi(buf: &Buffer) -> String {
    let area = buf.area;
    let mut out = String::with_capacity(area.width as usize * area.height as usize * 12 + 256);

    for y in area.top()..area.bottom() {
        let mut cur_fg = Color::Reset;
        let mut cur_bg = Color::Reset;
        let mut cur_mod = Modifier::empty();

        for x in area.left()..area.right() {
            let cell = &buf[(x, y)];

            if cell.modifier != cur_mod {
                if cell.modifier.contains(Modifier::BOLD) && !cur_mod.contains(Modifier::BOLD) {
                    out.push_str("\x1b[1m");
                }
                if cell.modifier.contains(Modifier::DIM) && !cur_mod.contains(Modifier::DIM) {
                    out.push_str("\x1b[2m");
                }
                cur_mod = cell.modifier;
            }

            if cell.fg != cur_fg {
                push_ratatui_color(&mut out, false, cell.fg);
                cur_fg = cell.fg;
            }
            if cell.bg != cur_bg {
                push_ratatui_color(&mut out, true, cell.bg);
                cur_bg = cell.bg;
            }

            out.push_str(cell.symbol());
        }
        out.push_str("\x1b[0m\r\n");
    }

    out
}

#[inline]
fn push_ratatui_color(out: &mut String, is_bg: bool, color: Color) {
    match color {
        Color::Reset => {
            if is_bg {
                out.push_str("\x1b[49m");
            } else {
                out.push_str("\x1b[39m");
            }
        }
        Color::Rgb(r, g, b) => {
            if is_bg {
                out.push_str("\x1b[48;2;");
            } else {
                out.push_str("\x1b[38;2;");
            }
            use std::fmt::Write;
            let _ = write!(out, "{r};{g};{b}m");
        }
        Color::Indexed(idx) => {
            if is_bg {
                out.push_str("\x1b[48;5;");
            } else {
                out.push_str("\x1b[38;5;");
            }
            use std::fmt::Write;
            let _ = write!(out, "{idx}m");
        }
        Color::Black => out.push_str(if is_bg { "\x1b[40m" } else { "\x1b[30m" }),
        Color::Red => out.push_str(if is_bg { "\x1b[41m" } else { "\x1b[31m" }),
        Color::Green => out.push_str(if is_bg { "\x1b[42m" } else { "\x1b[32m" }),
        Color::Yellow => out.push_str(if is_bg { "\x1b[43m" } else { "\x1b[33m" }),
        Color::Blue => out.push_str(if is_bg { "\x1b[44m" } else { "\x1b[34m" }),
        Color::Magenta => out.push_str(if is_bg { "\x1b[45m" } else { "\x1b[35m" }),
        Color::Cyan => out.push_str(if is_bg { "\x1b[46m" } else { "\x1b[36m" }),
        Color::Gray => out.push_str(if is_bg { "\x1b[47m" } else { "\x1b[37m" }),
        Color::DarkGray => out.push_str(if is_bg { "\x1b[100m" } else { "\x1b[90m" }),
        Color::LightRed => out.push_str(if is_bg { "\x1b[101m" } else { "\x1b[91m" }),
        Color::LightGreen => out.push_str(if is_bg { "\x1b[102m" } else { "\x1b[92m" }),
        Color::LightYellow => out.push_str(if is_bg { "\x1b[103m" } else { "\x1b[93m" }),
        Color::LightBlue => out.push_str(if is_bg { "\x1b[104m" } else { "\x1b[94m" }),
        Color::LightMagenta => out.push_str(if is_bg { "\x1b[105m" } else { "\x1b[95m" }),
        Color::LightCyan => out.push_str(if is_bg { "\x1b[106m" } else { "\x1b[96m" }),
        Color::White => out.push_str(if is_bg { "\x1b[107m" } else { "\x1b[97m" }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ascii_mmorpg_shared::{ActorSnapshot, EntityKind, Point, Stats};

    fn sample_world() -> WorldSnapshot {
        WorldSnapshot {
            tick: 42,
            player: ActorSnapshot {
                id: 1,
                kind: EntityKind::Player,
                position: Point { x: 10, y: 15 },
                stats: Stats {
                    hp: 85,
                    max_hp: 100,
                    attack: 14,
                    defense: 9,
                    luck: 6,
                },
                name: "Player".into(),
                glyph: '@',
            },
            entities: vec![ActorSnapshot {
                id: 2,
                kind: EntityKind::Monster,
                position: Point { x: 12, y: 15 },
                stats: Stats {
                    hp: 25,
                    max_hp: 40,
                    attack: 8,
                    defense: 3,
                    luck: 1,
                },
                name: "Skeleton Warrior".into(),
                glyph: 'm',
            }],
            messages: vec![
                "Connected to Whispering Forest.".into(),
                "You strike Skeleton Warrior for 15 damage!".into(),
            ],
        }
    }

    #[test]
    fn test_ratatui_hud_renders_gauges_and_combat_log() {
        let mut hud = RatatuiHud::new();
        let world = sample_world();
        let rendered = hud.render_hud_overlay(&world, 80, 12, WeatherMode::Rain, "WGPU 3D");

        assert!(!rendered.is_empty());
        assert!(rendered.contains("Whispering Forest"));
        assert!(rendered.contains("WARRIOR"));
        assert!(rendered.contains("HP 85/100"));
        assert!(rendered.contains("Skeleton Warrior"));
        assert!(rendered.contains("Action & Combat Log"));
        assert!(rendered.contains("You strike Skeleton Warrior for 15 damage!"));
    }

    #[test]
    fn test_ratatui_hud_renders_interactive_modals() {
        let mut hud = RatatuiHud::new();
        let world = sample_world();

        // 1. Inventory Modal
        let inv_rendered = hud.render_hud_overlay_with_modal(
            &world,
            80,
            12,
            WeatherMode::Rain,
            "WGPU 3D",
            ActiveModal::Inventory,
        );
        assert!(inv_rendered.contains("Inventory & Equipment"));
        assert!(inv_rendered.contains("Equipped Gear"));
        assert!(inv_rendered.contains("Steel Broadsword"));
        assert!(inv_rendered.contains("Healing Draught"));

        // 2. Character Sheet Modal
        let char_rendered = hud.render_hud_overlay_with_modal(
            &world,
            80,
            12,
            WeatherMode::Rain,
            "WGPU 3D",
            ActiveModal::CharacterSheet,
        );
        assert!(char_rendered.contains("Character Sheet"));
        assert!(char_rendered.contains("Attributes & Modifiers"));
        assert!(char_rendered.contains("Strength"));
        assert!(char_rendered.contains("Vitality"));

        // 3. World Map Modal
        let map_rendered = hud.render_hud_overlay_with_modal(
            &world,
            80,
            12,
            WeatherMode::Rain,
            "WGPU 3D",
            ActiveModal::WorldMap,
        );
        assert!(map_rendered.contains("Overworld Map"));
        assert!(map_rendered.contains("Whispering Forest"));
        assert!(map_rendered.contains("Position:"));
        assert!(map_rendered.contains("GATE"));
    }
}
