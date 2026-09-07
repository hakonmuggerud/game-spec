//! `SimRng` — the sim's only source of randomness, so every run is seedable and tests are deterministic.
//! Replaces the prototype's `Math.random()` calls (`hunter.js:pickWander`, `driftPick`, `pickRest`, the
//! false light's rest timers, `endgame.js` extra spawns …). Backed by `rand::rngs::SmallRng`.

use rand::rngs::SmallRng;
use rand::{Rng, SeedableRng};

/// A seedable random source with the handful of draws the JS uses.
#[derive(Debug, Clone)]
pub struct SimRng {
    inner: SmallRng,
}

impl SimRng {
    /// From a 64-bit seed.
    pub fn seed(seed: u64) -> SimRng {
        SimRng {
            inner: SmallRng::seed_from_u64(seed),
        }
    }

    /// From OS entropy (the app's normal path).
    pub fn from_entropy() -> SimRng {
        SimRng {
            inner: SmallRng::from_os_rng(),
        }
    }

    /// `Math.random()` — uniform in `[0, 1)`.
    pub fn unit(&mut self) -> f64 {
        self.inner.random::<f64>()
    }

    /// `Math.random() * n | 0` — an index in `0..n` (`n > 0`).
    pub fn index(&mut self, n: usize) -> usize {
        debug_assert!(n > 0, "index() needs a non-empty range");
        ((self.unit() * n as f64) as usize).min(n.saturating_sub(1))
    }

    /// `lo + Math.random() * (hi - lo)`.
    pub fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (self.unit() as f32) * (hi - lo)
    }

    /// Pick one element of a non-empty slice.
    pub fn pick<'a, T>(&mut self, items: &'a [T]) -> Option<&'a T> {
        if items.is_empty() {
            None
        } else {
            let i = self.index(items.len());
            items.get(i)
        }
    }
}

/// Anything that can hand out `Math.random()`-style draws (lets a test inject a scripted sequence).
pub trait RandomSource {
    /// Uniform in `[0, 1)`.
    fn unit(&mut self) -> f64;
}

impl RandomSource for SimRng {
    fn unit(&mut self) -> f64 {
        SimRng::unit(self)
    }
}

/// A scripted source for tests: replays the given values, then repeats the last one.
#[derive(Debug, Clone)]
pub struct ScriptedRng {
    values: Vec<f64>,
    at: usize,
}

impl ScriptedRng {
    /// Replay these values in order.
    pub fn new(values: Vec<f64>) -> ScriptedRng {
        ScriptedRng { values, at: 0 }
    }
}

impl RandomSource for ScriptedRng {
    fn unit(&mut self) -> f64 {
        let v = self
            .values
            .get(self.at)
            .or_else(|| self.values.last())
            .copied()
            .unwrap_or(0.5);
        self.at += 1;
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seeded_is_deterministic() {
        let mut a = SimRng::seed(7);
        let mut b = SimRng::seed(7);
        for _ in 0..16 {
            assert_eq!(a.unit(), b.unit());
        }
        for _ in 0..1000 {
            let i = a.index(5);
            assert!(i < 5);
            let r = a.range(2.0, 3.0);
            assert!((2.0..3.0).contains(&r));
        }
        assert_eq!(a.pick::<u8>(&[]), None);
        assert_eq!(a.pick(&[9]), Some(&9));
    }

    #[test]
    fn scripted_replays() {
        let mut s = ScriptedRng::new(vec![0.1, 0.9]);
        assert_eq!(s.unit(), 0.1);
        assert_eq!(s.unit(), 0.9);
        assert_eq!(s.unit(), 0.9);
    }
}
