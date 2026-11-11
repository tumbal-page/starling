// Copyright (c) The Create Team / The Creators of Create
// Copyright (c) 2026 create-horizon contributors
// SPDX-License-Identifier: MIT
// PressingBehaviour client read/start/tick/getRenderedHeadOffset/spawnParticles,
// Create ac0c444d9828da3453ae8cc65338e8de063286fb. Platform receipt, item
// decoding, particle admission and actual client BE ticking remain in adapters.
use crate::particles::ParticleRandom;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(i32)]
pub enum PressMode {
    #[default]
    World = 0,
    Belt = 1,
    Basin = 2,
}
impl PressMode {
    pub fn from_index(index: i32) -> Option<Self> {
        match index {
            0 => Some(Self::World),
            1 => Some(Self::Belt),
            2 => Some(Self::Basin),
            _ => None,
        }
    }
    pub fn head_travel(self) -> f32 {
        match self {
            Self::World => 1.0,
            Self::Belt => 19.0 / 16.0,
            Self::Basin => 22.0 / 16.0,
        }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PressPacket {
    pub running: bool,
    pub finished: bool,
    pub ticks: i32,
    pub mode: PressMode,
}
/// The platform adds each particle before the next offset is drawn. Its
/// ordinary limiter may itself consume ClientLevel.random, so batching requests
/// before admission would change the source continuation on Decreased settings.
pub trait PressParticleContext<T>: ParticleRandom {
    fn emit(&mut self, item: &T, position: [f64; 3], velocity: [f64; 3]);
}
#[derive(Clone, Debug, PartialEq)]
pub struct PressClient<T> {
    pub running: bool,
    pub finished: bool,
    pub previous_ticks: i32,
    pub ticks: i32,
    pub mode: PressMode,
    pub particle_items: Vec<T>,
}
impl<T> Default for PressClient<T> {
    fn default() -> Self {
        Self {
            running: false,
            finished: false,
            previous_ticks: 0,
            ticks: 0,
            mode: PressMode::World,
            particle_items: Vec::new(),
        }
    }
}
impl<T> PressClient<T> {
    /// Source read(clientPacket=false) updates scalars but does not replay
    /// ParticleItems from saved/chunk NBT or erase an already queued list.
    pub fn read(&mut self, packet: PressPacket) {
        self.running = packet.running;
        self.mode = packet.mode;
        self.finished = packet.finished;
        self.previous_ticks = packet.ticks;
        self.ticks = packet.ticks;
    }
    /// Called once for EVERY actual client packet, including identical ones.
    pub fn read_packet(
        &mut self,
        packet: PressPacket,
        items: impl IntoIterator<Item = T>,
        origin: [i32; 3],
        context: &mut impl PressParticleContext<T>,
    ) -> usize {
        self.read(packet);
        self.particle_items.extend(items);
        self.spawn_particles(origin, context)
    }
    /// PressingBehaviour.start deliberately does not reset `finished`.
    pub fn start(&mut self, mode: PressMode) {
        self.mode = mode;
        self.running = true;
        self.previous_ticks = 0;
        self.ticks = 0;
        self.particle_items.clear();
    }
    /// Only the original client branch. Source server recipe/completion work
    /// stays in PressCycle and the native server adapter.
    pub fn tick(&mut self, rpm: f32, virtual_block: bool, attached: bool) {
        if !self.running || !attached {
            return;
        }
        if self.ticks == -120 {
            self.previous_ticks = 120;
            return;
        }
        if self.ticks == 120 && rpm != 0.0 && self.mode == PressMode::World {
            // applyInWorld clears its list before the client-side early return.
            // applyOnBasin returns before clearing it on the client.
            self.particle_items.clear();
        }
        self.previous_ticks = self.ticks;
        self.ticks = self.ticks.wrapping_add(tick_speed(rpm));
        if self.previous_ticks < 120 && self.ticks >= 120 {
            self.ticks = if virtual_block { 120 } else { -120 };
        }
    }
    pub fn rendered_offset(&self, partial: f32) -> f32 {
        if !self.running {
            return 0.0;
        }
        let absolute = self.ticks.wrapping_abs();
        let previous = self.previous_ticks as f32;
        let ticks = previous + partial * (absolute as f32 - previous);
        if absolute < 160 {
            let value = f64::from(ticks / 240.0 * 2.0).powf(3.0);
            value.clamp(0.0, 1.0) as f32
        } else {
            ((240.0 - ticks) / 240.0 * 3.0).clamp(0.0, 1.0)
        }
    }
    pub fn rendered_travel(&self, partial: f32) -> f32 {
        self.rendered_offset(partial) * self.mode.head_travel()
    }
    fn spawn_particles(
        &mut self,
        origin: [i32; 3],
        context: &mut impl PressParticleContext<T>,
    ) -> usize {
        let compacting = self.mode == PressMode::Basin;
        let count = if compacting { 20 } else { 15 };
        let spread: f32 = if compacting { 0.175 } else { 0.125 };
        let x = f64::from(origin[0]) + 0.5;
        let y = f64::from(origin[1])
            - match self.mode {
                PressMode::World => 1.0,
                PressMode::Belt => 1.25,
                PressMode::Basin => 1.5,
            };
        let z = f64::from(origin[2]) + 0.5;
        let amount = self.particle_items.len() * count;
        for item in &self.particle_items {
            for _ in 0..count {
                // VecHelper.offsetRandomly draws x, y, z, even though multiply
                // (1,0,1) discards the middle component immediately afterward.
                let [vx, _, vz] = crate::particles::offset_randomly(context, spread);
                context.emit(
                    item,
                    [x, y, z],
                    [vx, if compacting { 0.25 } else { 0.125 }, vz],
                );
            }
        }
        self.particle_items.clear();
        amount
    }
}
/// Source float lerp/clamp followed by Java's saturating float-to-int cast.
pub fn tick_speed(rpm: f32) -> i32 {
    if rpm == 0.0 {
        return 0;
    }
    let value = (rpm.abs() / 512.0).clamp(0.0, 1.0);
    (1.0 + value * (60.0 - 1.0)) as i32
}

#[cfg(test)]
mod tests {
    use super::*;
    #[derive(Default)]
    struct Random {
        draws: usize,
        particles: Vec<(i32, [f64; 3], [f64; 3])>,
    }
    impl ParticleRandom for Random {
        fn next_float(&mut self) -> f32 {
            self.draws += 1;
            0.5
        }
        fn next_int(&mut self, _: i32) -> i32 {
            unreachable!()
        }
    }
    impl PressParticleContext<i32> for Random {
        fn emit(&mut self, item: &i32, position: [f64; 3], velocity: [f64; 3]) {
            self.particles.push((*item, position, velocity));
        }
    }
    #[test]
    fn pause_until_actual_packet_then_continue_and_wait_for_server_finish() {
        let mut client = PressClient::<i32> {
            running: true,
            ticks: 116,
            ..Default::default()
        };
        client.tick(32.0, false, true);
        assert_eq!((client.previous_ticks, client.ticks), (116, -120));
        client.tick(32.0, false, true);
        assert_eq!((client.previous_ticks, client.ticks), (120, -120));
        assert_eq!(client.rendered_offset(0.5), 1.0);
        client.read(PressPacket {
            running: true,
            ticks: 120,
            ..Default::default()
        });
        client.tick(32.0, false, true);
        assert_eq!((client.previous_ticks, client.ticks), (120, 124));
        client.ticks = 241;
        client.tick(32.0, false, true);
        assert!(client.running);
        assert!(!client.finished);
        assert_eq!(client.ticks, 245);
        client.read(PressPacket {
            finished: true,
            ..Default::default()
        });
        assert!(!client.running);
        assert_eq!(client.rendered_offset(1.0), 0.0);
    }
    #[test]
    fn identical_packets_each_emit_and_advance_rng_and_start_preserves_finished() {
        for (mode, count, y) in [
            (PressMode::World, 15, 9.0),
            (PressMode::Belt, 15, 8.75),
            (PressMode::Basin, 20, 8.5),
        ] {
            let mut client = PressClient::<i32>::default();
            let mut rng = Random::default();
            let packet = PressPacket {
                finished: true,
                mode,
                ..Default::default()
            };
            for _ in 0..2 {
                let amount = client.read_packet(packet, [42], [1, 10, 2], &mut rng);
                assert_eq!(amount, count);
                assert!(rng.particles.iter().all(|p| p.0 == 42
                    && p.1 == [1.5, y, 2.5]
                    && p.2
                        == [
                            0.0,
                            if mode == PressMode::Basin {
                                0.25
                            } else {
                                0.125
                            },
                            0.0
                        ]));
                assert!(client.particle_items.is_empty());
            }
            assert_eq!(rng.draws, count * 6);
            client.particle_items.push(7);
            client.start(mode);
            assert!(client.finished && client.running && client.particle_items.is_empty());
            assert_eq!((client.ticks, client.previous_ticks), (0, 0));
        }
    }
    #[test]
    fn particle_admission_happens_between_offset_random_draws() {
        #[derive(Default)]
        struct Context {
            draws: usize,
            emitted_at: Vec<usize>,
        }
        impl ParticleRandom for Context {
            fn next_float(&mut self) -> f32 {
                self.draws += 1;
                0.5
            }
            fn next_int(&mut self, _: i32) -> i32 {
                unreachable!()
            }
        }
        impl PressParticleContext<i32> for Context {
            fn emit(&mut self, _: &i32, _: [f64; 3], _: [f64; 3]) {
                self.emitted_at.push(self.draws);
                // The real platform's particle admission may draw from the
                // same ClientLevel RNG. It must precede the next offset.
                self.next_float();
            }
        }
        let mut context = Context::default();
        let mut client = PressClient::default();
        assert_eq!(
            client.read_packet(PressPacket::default(), [42], [0; 3], &mut context),
            15
        );
        assert_eq!(
            context.emitted_at,
            (0..15).map(|n| n * 4 + 3).collect::<Vec<_>>()
        );
        assert_eq!(context.draws, 60);
    }

    #[test]
    fn detached_zero_virtual_and_load_particle_semantics() {
        let mut client = PressClient {
            running: true,
            ticks: 116,
            particle_items: vec![7],
            ..Default::default()
        };
        client.tick(32.0, false, false);
        assert_eq!(client.ticks, 116);
        client.tick(0.0, false, true);
        assert_eq!(client.ticks, 116);
        client.tick(32.0, true, true);
        assert_eq!(client.ticks, 120);
        client.read(PressPacket {
            running: true,
            ticks: 120,
            mode: PressMode::Basin,
            ..Default::default()
        });
        client.tick(32.0, false, true);
        assert_eq!(client.particle_items, vec![7]);
        client.read(PressPacket {
            running: true,
            ticks: 120,
            mode: PressMode::World,
            ..Default::default()
        });
        client.tick(32.0, false, true);
        assert!(client.particle_items.is_empty());
    }
}
