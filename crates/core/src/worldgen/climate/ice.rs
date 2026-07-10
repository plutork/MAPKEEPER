//! Ice cover from temperature and elevation.

pub(crate) fn ice_cover(temperature: i32, elevation: i32) -> i32 {
    if temperature <= -12 && elevation >= 35 {
        return 100;
    }
    if temperature <= -4 && elevation >= 55 {
        return 80;
    }
    if temperature <= 0 && elevation >= 70 {
        return 60;
    }
    0
}
