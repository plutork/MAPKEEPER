//! Climate T2 zonal heuristic (D-90 climate-t2--zonal-heuristic).

mod generate;
mod ice;
mod precipitation;
mod temperature;
#[cfg(test)]
mod tests;
mod types;

pub use generate::generate_climate_layers;
pub use types::*;
