//! Object collision mechanics, boundaries, and spatial physics.
//!
//! Provides deterministic 2D/3D collision detection for world structures,
//! gates, ancient ruins, pine trees, and environmental boundaries.

use serde::{Deserialize, Serialize};

use crate::protocol::{Direction, Point};

/// 2D Axis-Aligned Bounding Box (AABB) in world tile space.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct BoundingBox {
    pub min_x: i16,
    pub min_y: i16,
    pub max_x: i16,
    pub max_y: i16,
}

impl BoundingBox {
    /// Constructs a bounding box with explicit minimum and maximum coordinates.
    pub const fn new(min_x: i16, min_y: i16, max_x: i16, max_y: i16) -> Self {
        Self {
            min_x,
            min_y,
            max_x,
            max_y,
        }
    }

    /// Constructs a single-tile bounding box.
    pub const fn point(x: i16, y: i16) -> Self {
        Self {
            min_x: x,
            min_y: y,
            max_x: x,
            max_y: y,
        }
    }

    /// Constructs a centered bounding box around an origin with a given radius.
    pub const fn centered(origin: Point, half_width: i16, half_height: i16) -> Self {
        Self {
            min_x: origin.x.saturating_sub(half_width),
            min_y: origin.y.saturating_sub(half_height),
            max_x: origin.x.saturating_add(half_width),
            max_y: origin.y.saturating_add(half_height),
        }
    }

    /// Returns `true` if this bounding box contains the specified point.
    pub const fn contains(&self, point: Point) -> bool {
        point.x >= self.min_x
            && point.x <= self.max_x
            && point.y >= self.min_y
            && point.y <= self.max_y
    }

    /// Returns `true` if this bounding box intersects another bounding box.
    pub const fn intersects(&self, other: &Self) -> bool {
        self.min_x <= other.max_x
            && self.max_x >= other.min_x
            && self.min_y <= other.max_y
            && self.max_y >= other.min_y
    }

    /// Expands the bounding box uniformly by `margin` units on all sides.
    pub const fn expand(&self, margin: i16) -> Self {
        Self {
            min_x: self.min_x.saturating_sub(margin),
            min_y: self.min_y.saturating_sub(margin),
            max_x: self.max_x.saturating_add(margin),
            max_y: self.max_y.saturating_add(margin),
        }
    }
}

/// Categorization of environmental and structural colliders.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum ObstacleKind {
    /// Ancient fortress gatehouse, watchtowers, and portcullis.
    Gate,
    /// Stone colonnade pillars, broken architraves, and altars.
    Ruins,
    /// Massive pine tree trunks and forest thickets.
    Tree,
    /// Lantern posts and timber fixtures.
    Post,
    /// Impassable natural barriers and world zone boundaries.
    WorldBoundary,
}

/// A physical world obstacle with an identifying label and collision bounds.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Obstacle {
    pub id: &'static str,
    pub name: &'static str,
    pub kind: ObstacleKind,
    pub bounds: BoundingBox,
}

impl Obstacle {
    pub const fn new(
        id: &'static str,
        name: &'static str,
        kind: ObstacleKind,
        bounds: BoundingBox,
    ) -> Self {
        Self {
            id,
            name,
            kind,
            bounds,
        }
    }
}

/// Deterministic terrain pseudorandom seed matching the procedural renderer.
pub fn terrain_seed(x: i16, y: i16) -> u32 {
    let mut value = (i32::from(x) as u32).wrapping_mul(0x45d9f3b);
    value ^= (i32::from(y) as u32).wrapping_mul(0x119de1f3);
    value ^= value >> 16;
    value.wrapping_mul(0x45d9f3b) ^ (value >> 13)
}

/// Standard overworld boundaries for the Whispering Forest zone.
pub const WORLD_BOUNDS: BoundingBox = BoundingBox::new(-35, -35, 35, 35);

/// Returns the complete registry of static obstacles and scenery boundaries in the overworld.
///
/// Depth-Aware Ground Footprint Principle:
/// In 2.5D and isometric perspective with height elevation, characters stand on the ground
/// at their feet, while tree canopies, roofs, and lintels extend upward into the air.
/// Collision boundaries strictly represent the ground-contact footprint (tree trunks,
/// pillar bases, and foundation walls) where an actor's feet make physical contact.
/// Upper visual geometry (foliage, branches, headers) does not impede ground movement,
/// allowing foreground actors to stand in front of trees and overlap them naturally.
pub fn overworld_obstacles() -> Vec<Obstacle> {
    vec![
        // --- Stone Fortress Gatehouse & Towers ---
        // Tower foundation footprints along the northern gateway line at y = -12
        Obstacle::new(
            "gate_west_tower",
            "Stone Gate (West Tower)",
            ObstacleKind::Gate,
            BoundingBox::new(-4, -12, -2, -12),
        ),
        Obstacle::new(
            "gate_east_tower",
            "Stone Gate (East Tower)",
            ObstacleKind::Gate,
            BoundingBox::new(2, -12, 4, -12),
        ),
        Obstacle::new(
            "gate_portcullis",
            "Fortress Portcullis Grate",
            ObstacleKind::Gate,
            BoundingBox::new(-1, -12, 1, -12),
        ),
        // --- Ancient Stone Ruins, Pillars & Altar ---
        // Stone altar pedestal ground base at y = -5
        Obstacle::new(
            "ruin_altar",
            "Overgrown Stone Altar",
            ObstacleKind::Ruins,
            BoundingBox::new(-8, -5, -7, -5),
        ),
        // Colonnade pillar ground bases at y = -5
        Obstacle::new(
            "ruin_pillar_west",
            "Ancient Colonnade Pillar (West)",
            ObstacleKind::Ruins,
            BoundingBox::point(-10, -5),
        ),
        Obstacle::new(
            "ruin_pillar_east",
            "Ancient Colonnade Pillar (East)",
            ObstacleKind::Ruins,
            BoundingBox::point(-6, -5),
        ),
        // --- Landmark Pine Trees (Trunk Ground Contact) ---
        // Ground trunk base is strictly a single tile footprint where the trunk meets the dirt.
        // The canopy extends overhead without blocking character movement on the ground.
        Obstacle::new(
            "pine_tree_sw",
            "Towering Pine Tree (West Woods)",
            ObstacleKind::Tree,
            BoundingBox::point(-7, -2),
        ),
        Obstacle::new(
            "pine_tree_se",
            "Towering Pine Tree (East Meadow)",
            ObstacleKind::Tree,
            BoundingBox::point(8, -3),
        ),
        Obstacle::new(
            "pine_tree_ne",
            "Towering Pine Tree (North-East Ridge)",
            ObstacleKind::Tree,
            BoundingBox::point(9, -10),
        ),
        Obstacle::new(
            "pine_tree_nw",
            "Towering Pine Tree (North-West Ridge)",
            ObstacleKind::Tree,
            BoundingBox::point(-9, -10),
        ),
        // --- Timber Lantern Posts ---
        Obstacle::new(
            "lantern_post_sw",
            "Timber Lantern Post",
            ObstacleKind::Post,
            BoundingBox::point(-6, 2),
        ),
        Obstacle::new(
            "lantern_post_ne",
            "Timber Lantern Post",
            ObstacleKind::Post,
            BoundingBox::point(6, -4),
        ),
        // --- Crimson War Banners ---
        Obstacle::new(
            "banner_post_west",
            "Crimson War Banner Post",
            ObstacleKind::Post,
            BoundingBox::point(-8, 3),
        ),
        Obstacle::new(
            "banner_post_east",
            "Crimson War Banner Post",
            ObstacleKind::Post,
            BoundingBox::point(8, -3),
        ),
    ]
}

/// Checks whether a given world tile collides with any static obstacle,
/// procedural pine tree, or breaches the world boundary.
///
/// Respects depth-aware ground footprints: only the physical base/trunk touching
/// the ground blocks the actor's feet.
pub fn is_point_obstructed(point: Point, obstacles: &[Obstacle]) -> Option<&'static str> {
    // 1. Zone Boundary Check
    if !WORLD_BOUNDS.contains(point) {
        return Some("Whispering Forest Boundary");
    }

    // 2. Static Landmark Obstacles (Ground-contact footprints)
    for obstacle in obstacles {
        if obstacle.bounds.contains(point) {
            return Some(obstacle.name);
        }
    }

    // 3. Procedural Background Trees (seed % 9 == 0 on 11x9 world grid)
    // Only the trunk rooted at (tree_wx, tree_wy) blocks movement.
    // The foliage canopy floats overhead, allowing actors to stand in front of
    // or behind the tree without premature collision.
    let gx = point.x.div_euclid(11);
    let gy = point.y.div_euclid(9);
    let seed = terrain_seed(gx, gy);
    let tree_wx = gx * 11 + (seed % 5) as i16;
    let tree_wy = gy * 9 + ((seed / 7) % 5) as i16;
    if tree_wx.abs() > 4 && seed % 9 == 0 && point.x == tree_wx && point.y == tree_wy {
        return Some("Pine Tree Trunk");
    }

    None
}

/// Result of an actor's movement attempt considering collision boundaries and sliding physics.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MovementOutcome {
    pub original_position: Point,
    pub target_position: Point,
    pub resolved_position: Point,
    pub blocked: bool,
    pub slid: bool,
    pub obstacle: Option<&'static str>,
}

/// Resolves an attempted movement from `current` in `direction`, respecting obstacle boundaries.
///
/// Features:
/// - Exact collision boundary blocking for cardinal movements (`N`, `S`, `E`, `W`).
/// - Smooth sliding along unobstructed axes during diagonal movement (`NE`, `NW`, `SE`, `SW`).
/// - Zone perimeter confinement preventing player escape into coordinate void.
pub fn resolve_movement(
    current: Point,
    direction: Direction,
    obstacles: &[Obstacle],
) -> MovementOutcome {
    let direct_target = current.step(direction);

    // Direct movement check
    let direct_blocker = is_point_obstructed(direct_target, obstacles);
    if direct_blocker.is_none() {
        return MovementOutcome {
            original_position: current,
            target_position: direct_target,
            resolved_position: direct_target,
            blocked: false,
            slid: false,
            obstacle: None,
        };
    }

    let blocker_name = direct_blocker;

    // Handle smooth sliding physics for diagonal movement attempts
    let (dx, dy) = direction.delta();
    if dx != 0 && dy != 0 {
        // Try horizontal slide
        let horizontal_target = Point {
            x: current.x.saturating_add(dx),
            y: current.y,
        };
        if is_point_obstructed(horizontal_target, obstacles).is_none() {
            return MovementOutcome {
                original_position: current,
                target_position: direct_target,
                resolved_position: horizontal_target,
                blocked: false,
                slid: true,
                obstacle: blocker_name,
            };
        }

        // Try vertical slide
        let vertical_target = Point {
            x: current.x,
            y: current.y.saturating_add(dy),
        };
        if is_point_obstructed(vertical_target, obstacles).is_none() {
            return MovementOutcome {
                original_position: current,
                target_position: direct_target,
                resolved_position: vertical_target,
                blocked: false,
                slid: true,
                obstacle: blocker_name,
            };
        }
    }

    // Completely blocked: maintain current position
    MovementOutcome {
        original_position: current,
        target_position: direct_target,
        resolved_position: current,
        blocked: true,
        slid: false,
        obstacle: blocker_name,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounding_box_contains_and_intersects() {
        let bbox = BoundingBox::new(2, 2, 6, 8);
        assert!(bbox.contains(Point { x: 2, y: 2 }));
        assert!(bbox.contains(Point { x: 4, y: 5 }));
        assert!(bbox.contains(Point { x: 6, y: 8 }));
        assert!(!bbox.contains(Point { x: 1, y: 5 }));
        assert!(!bbox.contains(Point { x: 4, y: 9 }));

        let overlapping = BoundingBox::new(5, 7, 10, 12);
        assert!(bbox.intersects(&overlapping));

        let disjoint = BoundingBox::new(7, 9, 10, 12);
        assert!(!bbox.intersects(&disjoint));
    }

    #[test]
    fn cardinal_movement_into_obstacle_is_blocked() {
        let obstacles = overworld_obstacles();
        // Timber Lantern Post is at (-6, 2). Moving East from (-7, 2) hits it.
        let from = Point { x: -7, y: 2 };
        let outcome = resolve_movement(from, Direction::East, &obstacles);

        assert!(outcome.blocked);
        assert_eq!(outcome.resolved_position, from);
        assert_eq!(outcome.obstacle, Some("Timber Lantern Post"));
    }

    #[test]
    fn diagonal_movement_slides_along_open_axis() {
        let obstacles = overworld_obstacles();
        // Moving NorthEast from (-7, 3) towards (-6, 2):
        // Target (-6, 2) is blocked by Timber Lantern Post.
        // Horizontal step (-6, 3) is free -> slides East!
        let from = Point { x: -7, y: 3 };
        let outcome = resolve_movement(from, Direction::NorthEast, &obstacles);

        assert!(!outcome.blocked);
        assert!(outcome.slid);
        assert_eq!(outcome.resolved_position, Point { x: -6, y: 3 });
    }

    #[test]
    fn world_boundary_prevents_escape() {
        let obstacles = overworld_obstacles();
        let border = Point { x: 35, y: 0 };
        let outcome = resolve_movement(border, Direction::East, &obstacles);

        assert!(outcome.blocked);
        assert_eq!(outcome.resolved_position, border);
        assert_eq!(outcome.obstacle, Some("Whispering Forest Boundary"));
    }

    #[test]
    fn unobstructed_movement_succeeds() {
        let obstacles = overworld_obstacles();
        let start = Point { x: 0, y: 0 };
        let outcome = resolve_movement(start, Direction::North, &obstacles);

        assert!(!outcome.blocked);
        assert!(!outcome.slid);
        assert_eq!(outcome.resolved_position, Point { x: 0, y: -1 });
        assert_eq!(outcome.obstacle, None);
    }

    #[test]
    fn standing_in_front_of_tree_is_unobstructed() {
        let obstacles = overworld_obstacles();
        // Towering Pine Tree (West Woods) trunk is at (-7, -2).
        // A player standing directly in front of the tree (South) has their feet at (-7, -1).
        let in_front = Point { x: -7, y: -1 };
        assert_eq!(is_point_obstructed(in_front, &obstacles), None);

        // Moving North from (-7, 0) up to the front of the tree succeeds:
        let outcome = resolve_movement(Point { x: -7, y: 0 }, Direction::North, &obstacles);
        assert!(!outcome.blocked);
        assert_eq!(outcome.resolved_position, in_front);

        // Only moving directly INTO the trunk base from in front is blocked:
        let hit_trunk = resolve_movement(in_front, Direction::North, &obstacles);
        assert!(hit_trunk.blocked);
        assert_eq!(hit_trunk.resolved_position, in_front);
        assert_eq!(hit_trunk.obstacle, Some("Towering Pine Tree (West Woods)"));
    }

    #[test]
    fn walking_around_tree_perimeter_is_unobstructed() {
        let obstacles = overworld_obstacles();
        // Tree trunk is at (-7, -2).
        let behind = Point { x: -7, y: -3 };
        let west_side = Point { x: -8, y: -2 };
        let east_side = Point { x: -6, y: -2 };

        // Feet can walk behind and beside the tree without hitting overhead foliage:
        assert_eq!(is_point_obstructed(behind, &obstacles), None);
        assert_eq!(is_point_obstructed(west_side, &obstacles), None);
        assert_eq!(is_point_obstructed(east_side, &obstacles), None);

        // Stepping into the trunk from behind is blocked:
        let hit_from_behind = resolve_movement(behind, Direction::South, &obstacles);
        assert!(hit_from_behind.blocked);
        assert_eq!(hit_from_behind.resolved_position, behind);
    }

    #[test]
    fn procedural_tree_only_blocks_trunk_ground_tile() {
        let obstacles = overworld_obstacles();
        // Find first procedural tree in region
        let mut found_tree = None;
        for gx in 1..10 {
            for gy in 0..10 {
                let seed = terrain_seed(gx, gy);
                if seed % 9 == 0 {
                    let wx = gx * 11 + (seed % 5) as i16;
                    if wx.abs() > 4 {
                        let wy = gy * 9 + ((seed / 7) % 5) as i16;
                        found_tree = Some(Point { x: wx, y: wy });
                        break;
                    }
                }
            }
            if found_tree.is_some() {
                break;
            }
        }

        let trunk = found_tree.expect("expected at least one procedural tree");
        // Ground trunk tile is blocked:
        assert_eq!(
            is_point_obstructed(trunk, &obstacles),
            Some("Pine Tree Trunk")
        );

        // Surrounding tiles (including directly in front and behind) are clear:
        assert_eq!(
            is_point_obstructed(
                Point {
                    x: trunk.x,
                    y: trunk.y + 1
                },
                &obstacles
            ),
            None
        );
        assert_eq!(
            is_point_obstructed(
                Point {
                    x: trunk.x,
                    y: trunk.y - 1
                },
                &obstacles
            ),
            None
        );
        assert_eq!(
            is_point_obstructed(
                Point {
                    x: trunk.x + 1,
                    y: trunk.y
                },
                &obstacles
            ),
            None
        );
        assert_eq!(
            is_point_obstructed(
                Point {
                    x: trunk.x - 1,
                    y: trunk.y
                },
                &obstacles
            ),
            None
        );
    }
}
