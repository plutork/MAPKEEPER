import { state } from "./workspace-state.js";

/** Update hover / details elevation readout (UI module). */
export const setHoverReadout = (axial, h, { outside = false } = {}) => {
  const hoverReliefEl = document.querySelector("#hover-relief");
  const detailsCellIdEl = document.querySelector("#details-cell-id");
  const detailsCellElevEl = document.querySelector("#details-cell-elev");
  if (!hoverReliefEl) return;
  if (outside || !axial) {
    hoverReliefEl.textContent = "Elevation: —";
    if (state.activeTool === "relief" && detailsCellIdEl && detailsCellElevEl) {
      detailsCellIdEl.textContent = "Cell: —";
      detailsCellElevEl.textContent = "Elevation: —";
    }
    return;
  }
  hoverReliefEl.textContent = `Elevation: ${h} @ ${axial.q},${axial.r}`;
  if (state.activeTool !== "relief" || !detailsCellIdEl || !detailsCellElevEl) return;
  detailsCellIdEl.textContent = `Cell: ${axial.q}, ${axial.r}`;
  detailsCellElevEl.textContent = `Elevation: ${h}`;
};
