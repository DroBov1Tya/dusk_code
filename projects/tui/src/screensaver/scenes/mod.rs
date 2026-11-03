//! The scenes, each a picture painted from a point and the time alone.

mod desert;
mod knight;
mod night;
mod water;

use super::paint::{Pt, Rgb};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scene {
    FujiLake,
    RiverDawn,
    MilkyWayCamp,
    Dunes,
    Campfire,
    Caravan,
    KnightFire,
    LanternRiver,
    AuroraCabin,
    NeonSunset,
    OceanSunset,
}

impl Scene {
    pub const ALL: [Self; 11] = [
        Self::FujiLake,
        Self::RiverDawn,
        Self::MilkyWayCamp,
        Self::Dunes,
        Self::Campfire,
        Self::Caravan,
        Self::KnightFire,
        Self::LanternRiver,
        Self::AuroraCabin,
        Self::NeonSunset,
        Self::OceanSunset,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::FujiLake => "mountain lake at dawn",
            Self::RiverDawn => "river at sunrise",
            Self::MilkyWayCamp => "desert camp under the milky way",
            Self::Dunes => "dunes",
            Self::Campfire => "campfire under the stars",
            Self::Caravan => "caravan at dusk",
            Self::KnightFire => "a knight at his fire",
            Self::LanternRiver => "lantern festival",
            Self::AuroraCabin => "northern lights over a cabin",
            Self::NeonSunset => "neon sunset",
            Self::OceanSunset => "ocean sunset",
        }
    }

    pub fn paint(self, p: Pt, t: f32) -> Rgb {
        match self {
            Self::FujiLake => water::fuji_lake(p, t),
            Self::RiverDawn => water::river_dawn(p, t),
            Self::MilkyWayCamp => desert::milky_way_camp(p, t),
            Self::Dunes => desert::dunes(p, t),
            Self::Campfire => desert::campfire(p, t),
            Self::Caravan => desert::caravan(p, t),
            Self::KnightFire => knight::knight_fire(p, t),
            Self::LanternRiver => night::lantern_river(p, t),
            Self::AuroraCabin => night::aurora_cabin(p, t),
            Self::NeonSunset => water::neon_sunset(p, t),
            Self::OceanSunset => water::ocean_sunset(p, t),
        }
    }
}
