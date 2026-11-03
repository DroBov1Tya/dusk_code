//! The scenes, each a picture painted from a point and the time alone.

mod autumn;
mod desert;
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
    CountryRoad,
    Pagoda,
    LanternRiver,
    AuroraCabin,
    Forest,
    NeonSunset,
    OceanSunset,
}

impl Scene {
    pub const ALL: [Self; 13] = [
        Self::FujiLake,
        Self::RiverDawn,
        Self::MilkyWayCamp,
        Self::Dunes,
        Self::Campfire,
        Self::Caravan,
        Self::CountryRoad,
        Self::Pagoda,
        Self::LanternRiver,
        Self::AuroraCabin,
        Self::Forest,
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
            Self::CountryRoad => "autumn road",
            Self::Pagoda => "pagoda in the maples",
            Self::LanternRiver => "lantern festival",
            Self::AuroraCabin => "northern lights over a cabin",
            Self::Forest => "autumn forest",
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
            Self::CountryRoad => autumn::country_road(p, t),
            Self::Pagoda => autumn::pagoda(p, t),
            Self::LanternRiver => night::lantern_river(p, t),
            Self::AuroraCabin => night::aurora_cabin(p, t),
            Self::Forest => autumn::forest(p, t),
            Self::NeonSunset => water::neon_sunset(p, t),
            Self::OceanSunset => water::ocean_sunset(p, t),
        }
    }
}
