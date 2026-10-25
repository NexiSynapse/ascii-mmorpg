//! Renderer-agnostic semantic data for Veyrath — The Dying Crown.
//!
//! The authoring manifest is the single semantic source for the visual map.
//! Its coordinates describe a provisional composition canvas, not server
//! positions, collision, or any other authoritative game state. The compact
//! printable atlas remains an embedded derived/fallback view for terminals.

use serde::Deserialize;
use std::sync::OnceLock;

pub const WORLD_DESIGN_JSON: &str = include_str!("../../../assets/maps/veyrath/world-design.json");
pub const POLITICAL_CHECKPOINT_JSON: &str =
    include_str!("../../../assets/maps/veyrath/veyrath-04-political-full.json");
pub const ASCII_ATLAS: &str = include_str!("../../../assets/maps/veyrath/veyrath-04-ascii.txt");

pub type Color = [u8; 3];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CoordinateStatus {
    Provisional,
    Accepted,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CheckpointStatus {
    PlacementPending,
    Accepted,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PointPercent {
    pub x: f32,
    pub y: f32,
}

impl PointPercent {
    fn from_array(value: [f32; 2]) -> Self {
        Self {
            x: value[0],
            y: value[1],
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Palette {
    pub background: Color,
    pub water: Color,
    pub forest: Color,
    pub lowland: Color,
    pub mountain: Color,
    pub ruins_and_borders: Color,
    pub labels: Color,
    pub blight: Color,
    pub highlight: Color,
    pub snow_highlight: Color,
    pub road: Color,
    pub river_highlight: Color,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Faction {
    pub id: String,
    pub name: String,
    pub color: Color,
    pub capital: String,
    pub territorial_seed: PointPercent,
    /// The atlas glyph is a presentation key for the derived printable view;
    /// it is not a second copy of the atlas geometry.
    pub atlas_glyph: char,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Region {
    pub id: String,
    pub name: String,
    pub anchor: PointPercent,
    pub extent_hint: [f32; 4],
    pub terrain: Vec<String>,
    pub faction: String,
    pub culture: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RoadKind {
    Royal,
    Regional,
    WildernessTrail,
    Maritime,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Road {
    pub id: String,
    pub name: String,
    pub kind: RoadKind,
    /// Node connectivity from the authoring manifest. These are not straight
    /// line geometry and must not be used as authoritative movement paths.
    pub nodes: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct River {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub path: Vec<PointPercent>,
    pub outlet: Option<String>,
    pub joins: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Settlement {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub region: String,
    pub faction: String,
    pub at: PointPercent,
    pub starter: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Landmark {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub region: String,
    pub at: PointPercent,
    pub principal: bool,
    pub settlement: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct VeyrathVisualData {
    pub title: String,
    pub source_status: String,
    pub coordinate_status: CoordinateStatus,
    pub checkpoint_status: CheckpointStatus,
    pub palette: Palette,
    pub factions: Vec<Faction>,
    pub regions: Vec<Region>,
    pub roads: Vec<Road>,
    pub rivers: Vec<River>,
    pub settlements: Vec<Settlement>,
    pub landmarks: Vec<Landmark>,
    pub starter_settlement: String,
    pub social_hub: String,
    pub atlas: &'static str,
}

impl VeyrathVisualData {
    pub fn faction(&self, id: &str) -> Option<&Faction> {
        self.factions.iter().find(|faction| faction.id == id)
    }

    pub fn region(&self, id: &str) -> Option<&Region> {
        self.regions.iter().find(|region| region.id == id)
    }

    pub fn settlement(&self, id: &str) -> Option<&Settlement> {
        self.settlements
            .iter()
            .find(|settlement| settlement.id == id)
    }

    // Presentation helpers for the derived printable atlas (glyph-colour
    // mapping, line clipping, terminal notes) intentionally live in the
    // CLIENT (client/src/veyrath.rs): the shared crate describes Veyrath
    // semantically; it does not decide how the terminal renders it
    // (review 2026-10-25, ownership split per docs/SOT.md).
}

pub fn visual_data() -> &'static VeyrathVisualData {
    static DATA: OnceLock<VeyrathVisualData> = OnceLock::new();
    DATA.get_or_init(parse_visual_data)
}

#[derive(Deserialize)]
struct RawManifest {
    title: String,
    status: String,
    coordinate_system: RawCoordinateSystem,
    palette: RawPalette,
    factions: Vec<RawFaction>,
    regions: Vec<RawRegion>,
    hydrology_intent: RawHydrology,
    transport_intent: RawTransport,
    settlements: Vec<RawSettlement>,
    landmarks: Vec<RawLandmark>,
    starter_slice: RawStarterSlice,
}

#[derive(Deserialize)]
struct RawCoordinateSystem {
    positions_are_provisional: bool,
}

#[derive(Deserialize)]
struct RawPalette {
    background: String,
    water: String,
    forest: String,
    lowland: String,
    mountain: String,
    ruins_and_borders: String,
    labels: String,
    blight: String,
    highlight: String,
    snow_highlight: String,
    road: String,
    river_highlight: String,
}

#[derive(Deserialize)]
struct RawFaction {
    id: String,
    name: String,
    colour: String,
    capital: String,
    territorial_seed: [f32; 2],
}

#[derive(Deserialize)]
struct RawRegion {
    id: String,
    name: String,
    anchor: [f32; 2],
    extent_hint: [f32; 4],
    terrain: Vec<String>,
    faction: String,
    culture: String,
}

#[derive(Deserialize)]
struct RawHydrology {
    rivers: Vec<RawRiver>,
}

#[derive(Deserialize)]
struct RawRiver {
    id: String,
    name: String,
    kind: String,
    path: Vec<[f32; 2]>,
    #[serde(default)]
    outlet: Option<String>,
    #[serde(default)]
    joins: Option<String>,
}

#[derive(Deserialize)]
struct RawTransport {
    royal_roads: Vec<RawNamedRoute>,
    regional_links: Vec<Vec<String>>,
    wilderness_trails: Vec<RawTrail>,
    maritime_links: Vec<Vec<String>>,
}

#[derive(Deserialize)]
struct RawNamedRoute {
    name: String,
    nodes: Vec<String>,
}

#[derive(Deserialize)]
struct RawTrail {
    from: String,
    to_landmark: String,
}

#[derive(Deserialize)]
struct RawSettlement {
    id: String,
    name: String,
    kind: String,
    region: String,
    faction: String,
    at: [f32; 2],
    #[serde(default)]
    starter: bool,
}

#[derive(Deserialize)]
struct RawLandmark {
    id: String,
    name: String,
    kind: String,
    region: String,
    at: [f32; 2],
    principal: bool,
    #[serde(default)]
    settlement: Option<String>,
}

#[derive(Deserialize)]
struct RawStarterSlice {
    spawn_settlement: String,
    social_hub: String,
}

fn parse_visual_data() -> VeyrathVisualData {
    let raw: RawManifest =
        serde_json::from_str(WORLD_DESIGN_JSON).expect("embedded Veyrath design JSON is valid");
    let palette = Palette {
        background: parse_color(&raw.palette.background),
        water: parse_color(&raw.palette.water),
        forest: parse_color(&raw.palette.forest),
        lowland: parse_color(&raw.palette.lowland),
        mountain: parse_color(&raw.palette.mountain),
        ruins_and_borders: parse_color(&raw.palette.ruins_and_borders),
        labels: parse_color(&raw.palette.labels),
        blight: parse_color(&raw.palette.blight),
        highlight: parse_color(&raw.palette.highlight),
        snow_highlight: parse_color(&raw.palette.snow_highlight),
        road: parse_color(&raw.palette.road),
        river_highlight: parse_color(&raw.palette.river_highlight),
    };
    let factions = raw
        .factions
        .into_iter()
        .map(|faction| Faction {
            atlas_glyph: faction_glyph(&faction.id),
            id: faction.id,
            name: faction.name,
            color: parse_color(&faction.colour),
            capital: faction.capital,
            territorial_seed: PointPercent::from_array(faction.territorial_seed),
        })
        .collect();
    let regions = raw
        .regions
        .into_iter()
        .map(|region| Region {
            id: region.id,
            name: region.name,
            anchor: PointPercent::from_array(region.anchor),
            extent_hint: region.extent_hint,
            terrain: region.terrain,
            faction: region.faction,
            culture: region.culture,
        })
        .collect();
    let rivers = raw
        .hydrology_intent
        .rivers
        .into_iter()
        .map(|river| River {
            id: river.id,
            name: river.name,
            kind: river.kind,
            path: river
                .path
                .into_iter()
                .map(PointPercent::from_array)
                .collect(),
            outlet: river.outlet,
            joins: river.joins,
        })
        .collect();
    let mut roads = Vec::new();
    for (index, route) in raw.transport_intent.royal_roads.into_iter().enumerate() {
        roads.push(Road {
            id: format!("royal-road-{index}"),
            name: route.name,
            kind: RoadKind::Royal,
            nodes: route.nodes,
        });
    }
    for (index, nodes) in raw.transport_intent.regional_links.into_iter().enumerate() {
        roads.push(Road {
            id: format!("regional-link-{index}"),
            name: format!("Regional link {index}"),
            kind: RoadKind::Regional,
            nodes,
        });
    }
    for trail in raw.transport_intent.wilderness_trails {
        roads.push(Road {
            id: format!("trail-{}-{}", trail.from, trail.to_landmark),
            name: format!("Trail to {}", trail.to_landmark),
            kind: RoadKind::WildernessTrail,
            nodes: vec![trail.from, trail.to_landmark],
        });
    }
    for (index, nodes) in raw.transport_intent.maritime_links.into_iter().enumerate() {
        roads.push(Road {
            id: format!("maritime-link-{index}"),
            name: format!("Maritime link {index}"),
            kind: RoadKind::Maritime,
            nodes,
        });
    }
    let settlements = raw
        .settlements
        .into_iter()
        .map(|settlement| Settlement {
            id: settlement.id,
            name: settlement.name,
            kind: settlement.kind,
            region: settlement.region,
            faction: settlement.faction,
            at: PointPercent::from_array(settlement.at),
            starter: settlement.starter,
        })
        .collect();
    let landmarks = raw
        .landmarks
        .into_iter()
        .map(|landmark| Landmark {
            id: landmark.id,
            name: landmark.name,
            kind: landmark.kind,
            region: landmark.region,
            at: PointPercent::from_array(landmark.at),
            principal: landmark.principal,
            settlement: landmark.settlement,
        })
        .collect();
    VeyrathVisualData {
        title: raw.title,
        source_status: raw.status.clone(),
        coordinate_status: if raw.coordinate_system.positions_are_provisional {
            CoordinateStatus::Provisional
        } else {
            CoordinateStatus::Accepted
        },
        checkpoint_status: if raw.status.contains("placement_pending") {
            CheckpointStatus::PlacementPending
        } else {
            CheckpointStatus::Accepted
        },
        palette,
        factions,
        regions,
        roads,
        rivers,
        settlements,
        landmarks,
        starter_settlement: raw.starter_slice.spawn_settlement,
        social_hub: raw.starter_slice.social_hub,
        atlas: ASCII_ATLAS,
    }
}

fn faction_glyph(id: &str) -> char {
    match id {
        "synod" => 'a',
        "gallowmere" => 'g',
        "crown" => 'c',
        "greyfall" => 'v',
        "mire-courts" => 'm',
        "blackwake" => 'b',
        _ => '?',
    }
}

fn parse_color(value: &str) -> Color {
    let value = value
        .strip_prefix('#')
        .expect("Veyrath colour starts with #");
    assert_eq!(value.len(), 6, "Veyrath colours are six-digit hex values");
    [
        u8::from_str_radix(&value[0..2], 16).expect("valid Veyrath red channel"),
        u8::from_str_radix(&value[2..4], 16).expect("valid Veyrath green channel"),
        u8::from_str_radix(&value[4..6], 16).expect("valid Veyrath blue channel"),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_manifest_exposes_the_complete_typed_visual_inventory() {
        let data = visual_data();
        assert_eq!(data.title, "Veyrath — The Dying Crown");
        assert_eq!(data.factions.len(), 6);
        assert_eq!(data.regions.len(), 6);
        assert_eq!(data.rivers.len(), 5);
        assert_eq!(data.settlements.len(), 40);
        assert_eq!(data.landmarks.len(), 45);
        assert_eq!(data.roads.len(), 30);
        assert_eq!(data.coordinate_status, CoordinateStatus::Provisional);
        assert_eq!(data.checkpoint_status, CheckpointStatus::PlacementPending);
        assert_eq!(data.starter_settlement, "millhaven");
        assert_eq!(data.social_hub, "veyrath");
        assert!(data.atlas.is_ascii());
        assert!(!POLITICAL_CHECKPOINT_JSON.is_empty());
    }
}
