use crate::protocol::Stats;

pub const MELEE_RANGE: i16 = 6;

pub fn in_melee_range(a: crate::protocol::Point, b: crate::protocol::Point) -> bool {
    (a.x - b.x).abs() + (a.y - b.y).abs() <= MELEE_RANGE
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CombatResult {
    pub damage: i32,
    pub defeated: bool,
    pub critical: bool,
}

/// Resolve an attack without global state. The seed is supplied by the server tick.
pub fn resolve_attack(attacker: &Stats, defender: &mut Stats, seed: u64) -> CombatResult {
    if !attacker.is_alive() || !defender.is_alive() {
        return CombatResult {
            damage: 0,
            defeated: !defender.is_alive(),
            critical: false,
        };
    }
    let roll = (seed ^ (seed >> 17)).wrapping_mul(0x9E37_79B9);
    let critical = attacker.luck > 0 && roll % 100 < u64::from(attacker.luck.min(25));
    let variation = (roll % 3) as i32 - 1;
    let base = (attacker.attack - defender.defense + variation).max(1);
    let damage = if critical {
        base.saturating_mul(2)
    } else {
        base
    };
    defender.hp = defender.hp.saturating_sub(damage);
    CombatResult {
        damage,
        defeated: !defender.is_alive(),
        critical,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn damage_is_at_least_one_and_cannot_underflow() {
        let attacker = Stats::healthy(10, 2, 0);
        let mut defender = Stats::healthy(10, 999, 0);
        let result = resolve_attack(&attacker, &mut defender, 2);
        assert_eq!(result.damage, 1);
        assert_eq!(defender.hp, 9);
    }

    #[test]
    fn dead_defender_is_reported_without_more_damage() {
        let attacker = Stats::healthy(10, 8, 0);
        let mut defender = Stats {
            hp: 0,
            ..Stats::healthy(2, 1, 0)
        };
        let result = resolve_attack(&attacker, &mut defender, 0);
        assert_eq!(result.damage, 0);
        assert!(result.defeated);
    }

    #[test]
    fn melee_range_is_shared_and_has_a_clear_boundary() {
        use crate::protocol::Point;

        assert!(in_melee_range(Point { x: 0, y: 0 }, Point { x: 6, y: 0 }));
        assert!(!in_melee_range(Point { x: 0, y: 0 }, Point { x: 7, y: 0 }));
    }
}
