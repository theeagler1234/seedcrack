//! Bedrock-floor/roof seed cracking.
//!
//! Unlike structures and decorators, bedrock placement is not gated by chunk
//! population order at all: each cell is an independent function of
//! `(world_seed, x, y, z)` via a named positional random fork
//! (`PositionalFactory::from_hash_of("minecraft:bedrock_floor").at(x, y, z)`).
//! That means a handful of observed cells constrain the *full* 64-bit world
//! seed directly, with no dependency on ever having seen a structure.
//!
//! Vanilla places a solid floor at `min_y` (always bedrock, zero information)
//! then a 1-in-5-shrinking chance for the next four layers:
//! `y = min_y+1 -> 80%`, `+2 -> 60%`, `+3 -> 40%`, `+4 -> 20%`, `+5 and above
//! -> never`. The Nether mirrors this at its ceiling. The always-solid and
//! always-air cells carry no signal and `BedrockObservation::new` rejects
//! them (returns `None`) so callers never accidentally add a zero-bit
//! observation.
use crate::xoroshiro::WorldgenRandom;

pub const FLOOR_NAME: &str = "minecraft:bedrock_floor";
pub const ROOF_NAME: &str = "minecraft:bedrock_roof";

const BAND: i32 = 5;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct BedrockObservation {
    pub x: i32,
    pub y: i32,
    pub z: i32,
    /// The dimension's inclusive world bottom and top at observation time
    /// (e.g. overworld -64/319, nether 0/127), kept on the observation
    /// itself (rather than derived once and discarded) so it round-trips
    /// through serialization without needing the dimension's bounds again.
    pub min_y: i32,
    pub max_y: i32,
    pub present: bool,
}

impl BedrockObservation {
    /// Returns `None` for a position outside either random band (the solid
    /// floor, the solid nether roof cap, or open air in between) since those
    /// carry no information about the seed.
    pub fn new(x: i32, y: i32, z: i32, present: bool, min_y: i32, max_y: i32) -> Option<BedrockObservation> {
        let in_floor = (min_y + 1..min_y + BAND).contains(&y);
        let in_roof = (max_y - BAND + 1..max_y).contains(&y);
        (in_floor || in_roof).then_some(BedrockObservation { x, y, z, min_y, max_y, present })
    }

    fn roof(&self) -> bool {
        self.y >= self.max_y - BAND + 1
    }

    fn chance(&self) -> f64 {
        let d = if self.roof() { self.y - (self.max_y - BAND + 1) + 1 } else { self.min_y + BAND - self.y };
        d as f64 / BAND as f64
    }

    fn name(&self) -> &'static str {
        if self.roof() { ROOF_NAME } else { FLOOR_NAME }
    }

    pub fn test(&self, world_seed: i64, base: &mut WorldgenRandom) -> bool {
        base.set_seed(world_seed);
        let factory = base.fork_positional().from_hash_of(self.name());
        let roll = factory.at(self.x, self.y, self.z).next_float() as f64;
        (roll < self.chance()) == self.present
    }

    pub fn bits(&self) -> f64 {
        let p = if self.present { self.chance() } else { 1.0 - self.chance() };
        -p.max(f64::MIN_POSITIVE).log2()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_uninformative_cells() {
        // Solid floor, solid roof, and open air all carry zero bits.
        assert!(BedrockObservation::new(0, -64, 0, true, -64, 319).is_none());
        assert!(BedrockObservation::new(0, 100, 0, false, -64, 319).is_none());
        assert!(BedrockObservation::new(0, 127, 0, true, 0, 127).is_none());
    }

    #[test]
    fn accepts_the_random_bands() {
        for y in -63..=-60 {
            assert!(BedrockObservation::new(0, y, 0, y % 2 == 0, -64, 319).is_some(), "y {y}");
        }
        for y in 123..=126 {
            assert!(BedrockObservation::new(0, y, 0, true, 0, 127).is_some(), "y {y}");
        }
    }

    #[test]
    fn is_deterministic_and_self_consistent() {
        let mut base = WorldgenRandom::new();
        let o = BedrockObservation::new(5, -63, -7, true, -64, 319).unwrap();
        let a = {
            base.set_seed(1234);
            let f = base.fork_positional().from_hash_of(o.name());
            f.at(o.x, o.y, o.z).next_float()
        };
        let b = {
            base.set_seed(1234);
            let f = base.fork_positional().from_hash_of(o.name());
            f.at(o.x, o.y, o.z).next_float()
        };
        assert_eq!(a, b, "the same seed and position must always fork the same stream");

        // Whichever way the roll actually falls, `test` must agree with it.
        let present = (a as f64) < o.chance();
        let matching = BedrockObservation { present, ..o };
        let opposite = BedrockObservation { present: !present, ..o };
        assert!(matching.test(1234, &mut base));
        assert!(!opposite.test(1234, &mut base));
    }

    #[test]
    fn roof_and_floor_agree_on_shape_symmetrically() {
        // Floor y = min_y+1..+4 should mirror roof y = max_y-4..-1 bit for bit.
        for i in 1..BAND {
            let floor = BedrockObservation::new(0, -64 + i, 0, true, -64, 127).unwrap();
            let roof = BedrockObservation::new(0, 127 - i, 0, true, -64, 127).unwrap();
            assert_eq!(floor.chance(), roof.chance(), "layer {i}");
            assert_ne!(floor.name(), roof.name());
        }
    }

    #[test]
    fn bits_are_never_negative_or_nan() {
        for y in -63..=-60 {
            for present in [true, false] {
                let o = BedrockObservation::new(0, y, 0, present, -64, 319).unwrap();
                assert!(o.bits() >= 0.0 && o.bits().is_finite(), "y {y} present {present}: {}", o.bits());
            }
        }
    }
}
