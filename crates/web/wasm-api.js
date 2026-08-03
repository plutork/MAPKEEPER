import init, {
  probe_world_to_axial,
  probe_axial_to_world,
  probe_grid_centers,
  probe_grid_center_bounds,
  probe_hex_distance,
  probe_max_brush_radius,
  probe_disk_cells,
  probe_pulse_interval_ms,
  probe_next_relief,
  probe_next_relief_absolute,
  probe_smooth_relief_average,
  probe_relief_range,
} from "./mapkeeper_web.js";

await init();

export {
  probe_world_to_axial,
  probe_axial_to_world,
  probe_grid_centers,
  probe_grid_center_bounds,
  probe_hex_distance,
  probe_max_brush_radius,
  probe_disk_cells,
  probe_pulse_interval_ms,
  probe_next_relief,
  probe_next_relief_absolute,
  probe_smooth_relief_average,
  probe_relief_range,
};
