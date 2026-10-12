//! The sound bank, synthesised at start-up: gunshots per weapon class (near and far), footsteps per surface, the
//! mechanics of reloading and drawing, impacts, the knife, grenades, the bomb, and the interface. Each sound comes
//! in a few variations so repeats don't sound mechanical.
//!
//! Everything is built from a handful of pieces — filtered noise, decaying sines, pitch sweeps, clicks — mixed and
//! shaped with envelopes, plus a small feedback-delay "room" for the tails.

use kira::sound::static_sound::StaticSoundData;
use kira::Frame;
use std::f32::consts::TAU;
use std::sync::Arc;

pub const RATE: u32 = 44_100;

struct Rng(u32);

impl Rng {
    fn f(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        self.0 as f32 / u32::MAX as f32 * 2.0 - 1.0
    }
}

/// A mono buffer being built.
pub struct Buf {
    pub s: Vec<f32>,
}

impl Buf {
    pub fn new(seconds: f32) -> Buf {
        Buf { s: vec![0.0; (seconds * RATE as f32) as usize] }
    }

    fn len_s(&self) -> f32 {
        self.s.len() as f32 / RATE as f32
    }

    /// Adds `f(t)` from `start` seconds for `dur` seconds.
    fn add(&mut self, start: f32, dur: f32, mut f: impl FnMut(f32) -> f32) {
        let a = (start * RATE as f32) as usize;
        let n = (dur * RATE as f32) as usize;
        for i in 0..n {
            if let Some(x) = self.s.get_mut(a + i) {
                *x += f(i as f32 / RATE as f32);
            }
        }
    }

    /// Filtered noise: a one-pole low-pass at `lp` Hz and high-pass at `hp` Hz, shaped by an exponential decay.
    #[allow(clippy::too_many_arguments)]
    fn noise(&mut self, start: f32, dur: f32, gain: f32, attack: f32, decay: f32, lp: f32, hp: f32, seed: u32) {
        let mut r = Rng(seed.max(1));
        let (mut low, mut high_state) = (0.0f32, 0.0f32);
        let a_lp = 1.0 - (-TAU * lp / RATE as f32).exp();
        let a_hp = 1.0 - (-TAU * hp / RATE as f32).exp();
        self.add(start, dur, |t| {
            let w = r.f();
            low += (w - low) * a_lp;
            high_state += (low - high_state) * a_hp;
            let band = low - high_state;
            let env = (t / attack.max(1e-4)).min(1.0) * (-t * decay).exp();
            band * env * gain
        });
    }

    /// A decaying sine, its pitch sliding from `f0` to `f1` over `glide` seconds.
    #[allow(clippy::too_many_arguments)]
    fn tone(&mut self, start: f32, dur: f32, gain: f32, f0: f32, f1: f32, glide: f32, decay: f32, attack: f32) {
        let mut phase = 0.0f32;
        self.add(start, dur, |t| {
            let k = (t / glide.max(1e-4)).min(1.0);
            let f = f0 + (f1 - f0) * k;
            phase += TAU * f / RATE as f32;
            let env = (t / attack.max(1e-4)).min(1.0) * (-t * decay).exp();
            phase.sin() * env * gain
        });
    }

    /// A metallic ring: a few inharmonic partials.
    fn ring(&mut self, start: f32, dur: f32, gain: f32, base: f32, decay: f32) {
        for (k, (ratio, amp)) in [(1.0, 1.0), (2.76, 0.6), (5.4, 0.35), (8.93, 0.2)].iter().enumerate() {
            self.tone(start, dur, gain * amp, base * ratio, base * ratio, 0.0, decay * (1.0 + k as f32 * 0.6), 0.0005);
        }
    }

    fn click(&mut self, start: f32, gain: f32, freq: f32, seed: u32) {
        self.noise(start, 0.03, gain, 0.0003, 180.0, freq * 2.0, freq * 0.4, seed);
        self.tone(start, 0.03, gain * 0.5, freq, freq * 0.8, 0.02, 160.0, 0.0002);
    }

    /// A small room: two feedback delays mixed in.
    fn room(&mut self, size: f32, mix: f32) {
        let d1 = (0.029 * size * RATE as f32) as usize;
        let d2 = (0.041 * size * RATE as f32) as usize;
        let mut out = self.s.clone();
        for i in 0..out.len() {
            let a = if i >= d1 { out[i - d1] * 0.45 } else { 0.0 };
            let b = if i >= d2 { out[i - d2] * 0.35 } else { 0.0 };
            out[i] += (a + b) * mix;
        }
        self.s = out;
    }

    fn lowpass(&mut self, hz: f32) {
        let a = 1.0 - (-TAU * hz / RATE as f32).exp();
        let mut y = 0.0;
        for x in &mut self.s {
            y += (*x - y) * a;
            *x = y;
        }
    }

    /// Normalises to a peak and soft-clips.
    fn finish(mut self, peak: f32) -> StaticSoundData {
        let m = self.s.iter().fold(0.0f32, |m, x| m.max(x.abs())).max(1e-6);
        for x in &mut self.s {
            *x = (*x / m * peak * 1.2).tanh() / 1.2f32.tanh() * peak.min(1.0);
        }
        // Fade the last 5 ms.
        let n = self.s.len();
        let fade = (0.005 * RATE as f32) as usize;
        for i in 0..fade.min(n) {
            self.s[n - 1 - i] *= i as f32 / fade as f32;
        }
        let frames: Vec<Frame> = self.s.iter().map(|&x| Frame { left: x, right: x }).collect();
        StaticSoundData { sample_rate: RATE, frames: Arc::from(frames), settings: Default::default(), slice: None }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Gun {
    Pistol,
    Heavy,
    Smg,
    Rifle,
    Sniper,
    Shotgun,
    Mg,
}

/// A gunshot: near (crack, body, thump, room) or far (muffled, rolling).
pub fn gunshot(kind: Gun, far: bool, seed: u32) -> StaticSoundData {
    let (crack, body_lp, thump, tail, tail_len) = match kind {
        Gun::Pistol => (0.9, 3200.0, 95.0, 0.35, 0.45),
        Gun::Heavy => (1.0, 2600.0, 70.0, 0.55, 0.8),
        Gun::Smg => (0.8, 3600.0, 110.0, 0.3, 0.4),
        Gun::Rifle => (1.0, 2800.0, 75.0, 0.55, 0.9),
        Gun::Sniper => (1.2, 2400.0, 55.0, 0.8, 1.5),
        Gun::Shotgun => (0.9, 1900.0, 60.0, 0.7, 1.0),
        Gun::Mg => (1.0, 2600.0, 70.0, 0.55, 0.85),
    };
    let len = if far { tail_len + 0.8 } else { tail_len + 0.2 };
    let mut b = Buf::new(len);
    if far {
        b.noise(0.0, len, 0.9, 0.004, 5.0, 900.0, 60.0, seed);
        b.tone(0.0, 0.5, 0.8, thump * 0.9, thump * 0.6, 0.3, 7.0, 0.004);
        b.noise(0.05, len, 0.35, 0.05, 2.5, 500.0, 40.0, seed ^ 7);
        b.room(3.0, 0.8);
        return b.finish(0.8);
    }
    b.noise(0.0, 0.02, crack, 0.0002, 260.0, 12_000.0, 2500.0, seed);
    b.noise(0.0, 0.3, 1.0, 0.0008, 22.0, body_lp, 120.0, seed ^ 3);
    b.tone(0.0, 0.35, 1.1, thump * 2.2, thump, 0.06, 14.0, 0.001);
    b.noise(0.02, tail_len, tail, 0.02, 4.5, 1400.0, 80.0, seed ^ 11);
    if kind == Gun::Sniper {
        b.noise(0.0, 0.05, 0.9, 0.0002, 90.0, 9000.0, 1500.0, seed ^ 5);
    }
    b.room(1.6, 0.6);
    b.finish(0.95)
}

/// The mechanism cycling between rounds (pump, bolt).
pub fn cycle(bolt: bool, seed: u32) -> StaticSoundData {
    let mut b = Buf::new(0.6);
    if bolt {
        b.click(0.0, 0.8, 1800.0, seed);
        b.noise(0.05, 0.12, 0.5, 0.01, 25.0, 4000.0, 800.0, seed ^ 1);
        b.click(0.25, 0.9, 2200.0, seed ^ 2);
    } else {
        b.noise(0.0, 0.15, 0.6, 0.01, 18.0, 2500.0, 300.0, seed);
        b.click(0.12, 1.0, 1400.0, seed ^ 3);
        b.noise(0.22, 0.12, 0.5, 0.01, 22.0, 2600.0, 300.0, seed ^ 4);
        b.click(0.33, 0.9, 1700.0, seed ^ 5);
    }
    b.finish(0.6)
}

/// A reload lasting `seconds`: the magazine out, in, and the bolt or slide.
pub fn reload(seconds: f32, pistol: bool, seed: u32) -> StaticSoundData {
    let mut b = Buf::new(seconds + 0.2);
    let t = seconds;
    b.noise(t * 0.1, 0.2, 0.3, 0.02, 15.0, 1800.0, 200.0, seed);
    b.click(t * 0.18, 0.9, 1500.0, seed ^ 1);
    b.noise(t * 0.2, 0.12, 0.4, 0.005, 30.0, 3500.0, 600.0, seed ^ 2);
    b.click(t * 0.55, 1.0, 1200.0, seed ^ 3);
    b.tone(t * 0.55, 0.06, 0.4, 300.0, 220.0, 0.05, 50.0, 0.001);
    if pistol {
        b.click(t * 0.85, 1.0, 2400.0, seed ^ 4);
    } else {
        b.click(t * 0.78, 0.9, 1900.0, seed ^ 5);
        b.noise(t * 0.8, 0.08, 0.5, 0.004, 35.0, 4000.0, 700.0, seed ^ 6);
        b.click(t * 0.86, 1.0, 2300.0, seed ^ 7);
    }
    b.finish(0.55)
}

pub fn shell_in(seed: u32) -> StaticSoundData {
    let mut b = Buf::new(0.25);
    b.noise(0.0, 0.1, 0.5, 0.005, 30.0, 2500.0, 300.0, seed);
    b.click(0.06, 0.9, 1100.0, seed ^ 1);
    b.finish(0.5)
}

pub fn dry_fire(seed: u32) -> StaticSoundData {
    let mut b = Buf::new(0.12);
    b.click(0.0, 1.0, 2600.0, seed);
    b.finish(0.5)
}

pub fn draw(seed: u32) -> StaticSoundData {
    let mut b = Buf::new(0.45);
    b.noise(0.0, 0.25, 0.5, 0.03, 12.0, 2200.0, 400.0, seed);
    b.click(0.2, 0.8, 1700.0, seed ^ 1);
    b.finish(0.45)
}

pub fn zoom(seed: u32) -> StaticSoundData {
    let mut b = Buf::new(0.15);
    b.click(0.0, 0.8, 3000.0, seed);
    b.noise(0.01, 0.08, 0.3, 0.005, 40.0, 5000.0, 1500.0, seed ^ 1);
    b.finish(0.35)
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Surface {
    Concrete,
    Metal,
    Wood,
    Dirt,
    Grass,
    Tile,
    Glass,
    Flesh,
}

pub fn footstep(s: Surface, seed: u32) -> StaticSoundData {
    let mut b = Buf::new(0.3);
    match s {
        Surface::Concrete => {
            b.noise(0.0, 0.12, 0.8, 0.002, 45.0, 2400.0, 250.0, seed);
            b.tone(0.0, 0.08, 0.5, 120.0, 80.0, 0.05, 40.0, 0.002);
        }
        Surface::Tile => {
            b.noise(0.0, 0.08, 0.8, 0.001, 60.0, 5000.0, 600.0, seed);
            b.tone(0.0, 0.06, 0.4, 180.0, 120.0, 0.04, 50.0, 0.001);
        }
        Surface::Metal => {
            b.noise(0.0, 0.06, 0.5, 0.001, 70.0, 4000.0, 500.0, seed);
            b.ring(0.0, 0.3, 0.35, 380.0 + (seed % 7) as f32 * 25.0, 14.0);
        }
        Surface::Wood => {
            b.noise(0.0, 0.1, 0.6, 0.002, 40.0, 1600.0, 150.0, seed);
            b.tone(0.0, 0.15, 0.8, 190.0, 140.0, 0.08, 22.0, 0.002);
        }
        Surface::Dirt => {
            b.noise(0.0, 0.2, 0.9, 0.01, 18.0, 3000.0, 400.0, seed);
            b.noise(0.03, 0.12, 0.5, 0.01, 25.0, 6000.0, 1500.0, seed ^ 9);
        }
        Surface::Grass => {
            b.noise(0.0, 0.22, 0.7, 0.02, 14.0, 4000.0, 900.0, seed);
        }
        Surface::Glass | Surface::Flesh => {
            b.noise(0.0, 0.1, 0.7, 0.002, 40.0, 2400.0, 250.0, seed);
        }
    }
    b.finish(0.5)
}

pub fn land(seed: u32) -> StaticSoundData {
    let mut b = Buf::new(0.35);
    b.noise(0.0, 0.2, 0.8, 0.002, 25.0, 1500.0, 80.0, seed);
    b.tone(0.0, 0.2, 1.0, 110.0, 55.0, 0.1, 18.0, 0.002);
    b.finish(0.6)
}

pub fn impact(s: Surface, seed: u32) -> StaticSoundData {
    let mut b = Buf::new(0.4);
    match s {
        Surface::Metal => {
            b.noise(0.0, 0.03, 0.6, 0.0005, 120.0, 8000.0, 1500.0, seed);
            b.ring(0.0, 0.4, 0.5, 900.0 + (seed % 11) as f32 * 80.0, 11.0);
        }
        Surface::Wood => {
            b.noise(0.0, 0.08, 0.7, 0.001, 50.0, 2000.0, 200.0, seed);
            b.tone(0.0, 0.12, 0.6, 260.0, 180.0, 0.08, 30.0, 0.001);
        }
        Surface::Glass => {
            b.noise(0.0, 0.05, 0.6, 0.0005, 80.0, 9000.0, 3000.0, seed);
            for k in 0..6 {
                b.ring(0.01 * k as f32, 0.25, 0.12, 2400.0 + k as f32 * 700.0, 18.0);
            }
        }
        Surface::Dirt | Surface::Grass => {
            b.noise(0.0, 0.12, 0.8, 0.002, 30.0, 1400.0, 90.0, seed);
        }
        Surface::Flesh => {
            b.noise(0.0, 0.12, 0.9, 0.002, 28.0, 900.0, 60.0, seed);
            b.tone(0.0, 0.1, 0.8, 90.0, 60.0, 0.06, 25.0, 0.002);
        }
        _ => {
            b.noise(0.0, 0.06, 0.8, 0.0005, 70.0, 5000.0, 700.0, seed);
            b.noise(0.0, 0.2, 0.3, 0.01, 18.0, 1200.0, 150.0, seed ^ 3);
        }
    }
    b.finish(0.55)
}

/// A helmet stopping a round: a hard metallic tink.
pub fn helmet(seed: u32) -> StaticSoundData {
    let mut b = Buf::new(0.5);
    b.noise(0.0, 0.02, 0.8, 0.0003, 150.0, 10_000.0, 2000.0, seed);
    b.ring(0.0, 0.5, 0.7, 1500.0, 9.0);
    b.finish(0.7)
}

/// Being hit: a dull body thump.
pub fn hurt(seed: u32) -> StaticSoundData {
    let mut b = Buf::new(0.3);
    b.noise(0.0, 0.18, 0.9, 0.002, 20.0, 700.0, 50.0, seed);
    b.tone(0.0, 0.2, 0.9, 80.0, 50.0, 0.1, 16.0, 0.002);
    b.finish(0.75)
}

pub fn whoosh(stab: bool, seed: u32) -> StaticSoundData {
    let len = if stab { 0.4 } else { 0.25 };
    let mut b = Buf::new(len);
    let mut r = Rng(seed.max(1));
    let mut lp = 0.0f32;
    let mut hp = 0.0f32;
    b.add(0.0, len, |t| {
        let k = t / len;
        let f = 600.0 + 2500.0 * (k * std::f32::consts::PI).sin();
        let a = 1.0 - (-TAU * f / RATE as f32).exp();
        let a2 = 1.0 - (-TAU * f * 0.4 / RATE as f32).exp();
        let w = r.f();
        lp += (w - lp) * a;
        hp += (lp - hp) * a2;
        (lp - hp) * (k * std::f32::consts::PI).sin().powi(2)
    });
    b.finish(0.5)
}

pub fn bounce(seed: u32) -> StaticSoundData {
    let mut b = Buf::new(0.3);
    b.noise(0.0, 0.03, 0.5, 0.0005, 100.0, 6000.0, 800.0, seed);
    b.ring(0.0, 0.3, 0.45, 650.0 + (seed % 5) as f32 * 60.0, 16.0);
    b.finish(0.45)
}

pub fn brass(seed: u32) -> StaticSoundData {
    let mut b = Buf::new(0.25);
    b.ring(0.0, 0.25, 0.5, 2600.0 + (seed % 9) as f32 * 150.0, 20.0);
    b.finish(0.3)
}

pub fn explosion(big: bool, far: bool, seed: u32) -> StaticSoundData {
    let len = if big { 3.5 } else { 2.2 };
    let mut b = Buf::new(len);
    b.noise(0.0, 0.08, 1.0, 0.0005, 30.0, 9000.0, 300.0, seed);
    b.noise(0.0, len, 1.0, 0.003, if big { 1.6 } else { 2.4 }, 700.0, 25.0, seed ^ 5);
    b.tone(0.0, 1.2, 1.2, 70.0, 28.0, 0.8, 3.0, 0.003);
    b.noise(0.1, len - 0.1, 0.4, 0.2, 1.8, 2200.0, 200.0, seed ^ 9);
    if far {
        b.lowpass(600.0);
    }
    b.room(3.0, 0.7);
    b.finish(1.0)
}

pub fn flashbang(seed: u32) -> StaticSoundData {
    let mut b = Buf::new(1.2);
    b.noise(0.0, 0.05, 1.0, 0.0003, 60.0, 12_000.0, 800.0, seed);
    b.noise(0.0, 1.0, 0.6, 0.002, 4.0, 1500.0, 100.0, seed ^ 3);
    b.tone(0.0, 0.3, 0.8, 90.0, 40.0, 0.2, 10.0, 0.002);
    b.room(2.5, 0.6);
    b.finish(0.95)
}

/// The ringing in the ears after a flash.
pub fn tinnitus(seconds: f32) -> StaticSoundData {
    let mut b = Buf::new(seconds);
    b.tone(0.0, seconds, 0.6, 3600.0, 3500.0, seconds, 1.2 / seconds.max(0.5), 0.05);
    b.tone(0.0, seconds, 0.2, 5200.0, 5100.0, seconds, 1.6 / seconds.max(0.5), 0.05);
    b.finish(0.35)
}

pub fn smoke_hiss(seed: u32) -> StaticSoundData {
    let mut b = Buf::new(2.5);
    b.noise(0.0, 2.5, 1.0, 0.1, 1.0, 7000.0, 1800.0, seed);
    b.finish(0.4)
}

pub fn beep() -> StaticSoundData {
    let mut b = Buf::new(0.11);
    b.add(0.0, 0.1, |t| (TAU * 2100.0 * t).sin().signum() * 0.4 * if t < 0.09 { 1.0 } else { 0.0 });
    b.lowpass(6000.0);
    b.finish(0.5)
}

/// Keying in the bomb's code.
pub fn keypad(seed: u32) -> StaticSoundData {
    let mut b = Buf::new(3.0);
    let mut r = Rng(seed.max(1));
    for i in 0..7 {
        let t = 0.25 + i as f32 * 0.36;
        let f = 900.0 + (r.f() * 0.5 + 0.5) * 900.0;
        b.add(t, 0.07, |x| (TAU * f * x).sin() * 0.4 * (1.0 - x / 0.07));
    }
    b.finish(0.45)
}

pub fn defuse_click(seed: u32) -> StaticSoundData {
    let mut b = Buf::new(0.4);
    b.click(0.0, 0.8, 2000.0, seed);
    b.click(0.15, 0.7, 1500.0, seed ^ 3);
    b.finish(0.45)
}

pub fn ui_click() -> StaticSoundData {
    let mut b = Buf::new(0.06);
    b.tone(0.0, 0.05, 0.6, 1400.0, 1100.0, 0.04, 60.0, 0.001);
    b.finish(0.3)
}

pub fn ui_hover() -> StaticSoundData {
    let mut b = Buf::new(0.04);
    b.tone(0.0, 0.035, 0.4, 2200.0, 2000.0, 0.03, 80.0, 0.001);
    b.finish(0.15)
}

pub fn buy() -> StaticSoundData {
    let mut b = Buf::new(0.3);
    b.click(0.0, 0.8, 1800.0, 3);
    b.tone(0.03, 0.2, 0.4, 1318.0, 1318.0, 0.0, 14.0, 0.002);
    b.tone(0.1, 0.2, 0.4, 1760.0, 1760.0, 0.0, 14.0, 0.002);
    b.finish(0.4)
}

pub fn pickup() -> StaticSoundData {
    let mut b = Buf::new(0.3);
    b.noise(0.0, 0.15, 0.5, 0.01, 20.0, 2500.0, 400.0, 5);
    b.click(0.12, 0.9, 1600.0, 7);
    b.finish(0.45)
}

/// A radio squelch before a line.
pub fn radio() -> StaticSoundData {
    let mut b = Buf::new(0.25);
    b.noise(0.0, 0.2, 0.6, 0.005, 12.0, 4500.0, 1200.0, 21);
    b.tone(0.0, 0.08, 0.3, 1200.0, 1200.0, 0.0, 30.0, 0.001);
    b.finish(0.3)
}

/// A short musical sting: rising (won) or falling (lost), and the round start.
pub fn sting(kind: u8) -> StaticSoundData {
    let notes: &[(f32, f32)] = match kind {
        0 => &[(0.0, 392.0), (0.12, 523.25), (0.24, 659.25), (0.36, 783.99)],
        1 => &[(0.0, 440.0), (0.16, 349.23), (0.32, 293.66), (0.48, 220.0)],
        _ => &[(0.0, 523.25), (0.1, 659.25)],
    };
    let len = notes.last().map_or(0.5, |n| n.0) + 1.0;
    let mut b = Buf::new(len);
    for &(t, f) in notes {
        for (h, a) in [(1.0, 0.5), (2.0, 0.25), (3.0, 0.12), (1.005, 0.3)] {
            b.tone(t, len - t, a, f * h, f * h, 0.0, 3.0, 0.01);
        }
    }
    b.lowpass(3000.0);
    b.room(2.0, 0.5);
    let _ = b.len_s();
    b.finish(0.45)
}
