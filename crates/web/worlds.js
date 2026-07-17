import { state } from "./workspace-state.js";
import { api } from "./api.js";
import { loadSpatial } from "./spatial-transaction.js";

const home = document.querySelector("#home");
const workspace = document.querySelector("#workspace");
const grid = document.querySelector("#project-grid");
const empty = document.querySelector("#project-empty");
const createFirstBtn = document.querySelector("#create-first");
const createAnotherBtn = document.querySelector("#create-another");
const dialog = document.querySelector("#create-dialog");
const deleteDialog = document.querySelector("#delete-dialog");
const deleteTarget = document.querySelector("#delete-target");
const deleteError = document.querySelector("#delete-error");
const idInput = document.querySelector("#world-id");
const folderInput = document.querySelector("#world-folder");
const errorBox = document.querySelector("#create-error");
const homeError = document.querySelector("#home-error");
const presetGrid = document.querySelector("#preset-grid");
const presetIdInput = document.querySelector("#create-preset-id");

let createPresets = [];

const worldPath = (name) => {
  const slug = name.trim().toLowerCase().replace(/[^a-z0-9_]+/g, "-").replace(/^-|-$/g, "");
  const separator = state.defaultRoot.includes("\\") ? "\\" : "/";
  return `${state.defaultRoot.replace(/[\\/]$/, "")}${separator}${slug}`;
};

const selectPreset = (presetId) => {
  presetIdInput.value = presetId;
  presetGrid.querySelectorAll(".preset-card").forEach((card) => {
    const on = card.dataset.presetId === presetId;
    card.classList.toggle("active", on);
    card.setAttribute("aria-checked", on ? "true" : "false");
  });
};

const renderPresetCards = (presets, defaultId) => {
  createPresets = presets;
  presetGrid.replaceChildren();
  for (const preset of presets) {
    const card = document.createElement("button");
    card.type = "button";
    card.className = "preset-card";
    card.dataset.presetId = preset.id;
    card.setAttribute("role", "radio");
    const titleRow = document.createElement("div");
    titleRow.className = "title-row";
    const title = document.createElement("span");
    title.className = "title";
    title.textContent = preset.display_name || preset.id;
    titleRow.append(title);
    if (preset.is_default) {
      const badge = document.createElement("span");
      badge.className = "badge";
      badge.textContent = "Default";
      titleRow.append(badge);
    }
    const extent = document.createElement("span");
    extent.className = "extent";
    extent.textContent = `≈ ${preset.width_km} × ${preset.height_km} km`;
    const stats = document.createElement("span");
    stats.className = "stats";
    const areaLabel = Math.round(Number(preset.area_km2) || 0).toLocaleString("en-US").replace(/,/g, "\u00a0");
    const cellsLabel = Number(preset.cells || 0).toLocaleString("en-US").replace(/,/g, "\u00a0");
    stats.textContent = `${areaLabel} km² · ${cellsLabel} cells`;
    card.append(titleRow, extent, stats);
    card.addEventListener("click", () => selectPreset(preset.id));
    presetGrid.append(card);
  }
  selectPreset(defaultId || (presets.find((p) => p.is_default) || presets[0] || {}).id || "");
};

const loadCreatePresets = async () => {
  const data = await api("/api/map-presets");
  renderPresetCards(data.presets || [], data.default_preset_id);
};

const openCreate = async () => {
  idInput.value = "";
  folderInput.value = state.defaultRoot;
  state.folderTouched = false;
  errorBox.textContent = "";
  try {
    if (!createPresets.length) await loadCreatePresets();
    else selectPreset(presetIdInput.value || createPresets.find((p) => p.is_default)?.id || createPresets[0].id);
  } catch (error) {
    errorBox.textContent = error.message;
  }
  dialog.showModal();
  idInput.focus();
};

const closeManageMenus = (except) => {
  grid.querySelectorAll(".manage-menu").forEach((menu) => {
    if (menu !== except) menu.classList.add("hidden");
  });
};

const renderProjects = (projects) => {
  grid.replaceChildren();
  const hasWorlds = projects.length > 0;
  empty.classList.toggle("hidden", hasWorlds);
  createFirstBtn.classList.toggle("hidden", hasWorlds);
  createAnotherBtn.classList.toggle("hidden", !hasWorlds);
  for (const project of projects) {
    const card = document.createElement("article");
    card.className = "project-card";
    const title = document.createElement("h3");
    title.textContent = project.id;
    const path = document.createElement("p");
    path.className = "muted";
    path.textContent = project.path;
    const actions = document.createElement("div");
    actions.className = "actions";

    const open = document.createElement("button");
    open.textContent = project.valid ? "Open" : "Missing";
    open.dataset.action = "open";
    open.dataset.path = project.path;
    if (!project.valid) open.disabled = true;

    const manage = document.createElement("div");
    manage.className = "manage";
    const manageBtn = document.createElement("button");
    manageBtn.type = "button";
    manageBtn.className = "manage-toggle";
    manageBtn.textContent = "Manage ▾";
    manageBtn.setAttribute("aria-haspopup", "menu");
    const menu = document.createElement("div");
    menu.className = "manage-menu hidden";
    menu.setAttribute("role", "menu");
    const manageItems = project.valid
      ? [["delete", "Delete", "danger"]]
      : [["forget", "Forget", ""]];
    for (const [action, label, className] of manageItems) {
      const button = document.createElement("button");
      button.type = "button";
      button.textContent = label;
      button.dataset.action = action;
      button.dataset.path = project.path;
      button.dataset.id = project.id;
      button.className = className;
      button.setAttribute("role", "menuitem");
      menu.append(button);
    }
    manageBtn.addEventListener("click", (event) => {
      event.stopPropagation();
      const willOpen = menu.classList.contains("hidden");
      closeManageMenus();
      menu.classList.toggle("hidden", !willOpen);
    });
    manage.append(manageBtn, menu);
    actions.append(open, manage);
    card.append(title, path, actions);
    grid.append(card);
  }
};

export const showWorkspace = async (project) => {
  document.querySelector("#world-name").textContent = project.id;
  document.querySelector("#world-path").textContent = project.path;
  home.classList.add("hidden");
  workspace.classList.remove("hidden");
  try {
    await loadSpatial();
  } catch (error) {
    const spatialStatus = document.querySelector("#spatial-status");
    if (spatialStatus) spatialStatus.textContent = error.message;
  }
};

export const showHome = () => {
  workspace.classList.add("hidden");
  home.classList.remove("hidden");
  state.spatial = null;
};

export const refresh = async () => {
  homeError.textContent = "";
  try {
    const data = await api("/api/projects");
    state.defaultRoot = data.default_worlds_root;
    renderProjects(data.projects);
    if (data.active) await showWorkspace(data.active);
  } catch (error) {
    homeError.textContent = error.message || String(error);
  }
};

const askDelete = (path, id) => {
  state.pendingDeletePath = path;
  state.pendingDeleteId = id || "";
  deleteTarget.textContent = id ? `${id}\n${path}` : path;
  deleteError.textContent = "";
  deleteDialog.showModal();
};

export const bindWorldEvents = () => {
  createFirstBtn.addEventListener("click", openCreate);
  createAnotherBtn.addEventListener("click", openCreate);
  document.querySelector("#cancel-create").addEventListener("click", () => dialog.close());

  idInput.addEventListener("input", () => {
    if (!state.folderTouched) folderInput.value = worldPath(idInput.value);
  });
  folderInput.addEventListener("input", () => { state.folderTouched = true; });

  document.querySelector("#browse-folder").addEventListener("click", async () => {
    try {
      const picked = await window.__TAURI__?.core?.invoke("pick_folder");
      if (picked) {
        state.defaultRoot = picked;
        state.folderTouched = false;
        folderInput.value = worldPath(idInput.value);
      }
    } catch (error) {
      errorBox.textContent = String(error);
    }
  });

  document.querySelector("#create-form").addEventListener("submit", async (event) => {
    event.preventDefault();
    errorBox.textContent = "";
    try {
      const project = await api("/api/projects", {
        method: "POST",
        body: JSON.stringify({
          id: idInput.value,
          path: folderInput.value,
          preset_id: presetIdInput.value || undefined,
        }),
      });
      dialog.close();
      await showWorkspace(project);
    } catch (error) {
      errorBox.textContent = error.message;
    }
  });

  grid.addEventListener("click", async (event) => {
    const button = event.target.closest("button[data-action]");
    if (!button) {
      if (!event.target.closest(".manage")) closeManageMenus();
      return;
    }
    closeManageMenus();
    if (button.dataset.action === "delete") {
      askDelete(button.dataset.path, button.dataset.id);
      return;
    }
    homeError.textContent = "";
    try {
      const body = JSON.stringify({ path: button.dataset.path });
      const project = await api(`/api/projects/${button.dataset.action}`, { method: "POST", body });
      if (button.dataset.action === "open") await showWorkspace(project);
      else await refresh();
    } catch (error) {
      homeError.textContent = error.message || String(error);
    }
  });

  document.querySelector("#delete-form").addEventListener("submit", async (event) => {
    const submitter = event.submitter;
    if (!submitter || submitter.value !== "confirm") {
      state.pendingDeletePath = "";
      state.pendingDeleteId = "";
      return;
    }
    event.preventDefault();
    deleteError.textContent = "";
    try {
      await api("/api/projects/delete", {
        method: "POST",
        body: JSON.stringify({
          path: state.pendingDeletePath,
          expected_id: state.pendingDeleteId,
        }),
      });
      state.pendingDeletePath = "";
      state.pendingDeleteId = "";
      deleteDialog.close();
      await refresh();
    } catch (error) {
      deleteError.textContent = error.message;
    }
  });

  document.addEventListener("click", (event) => {
    if (!event.target.closest("#project-grid .manage")) closeManageMenus();
  });

  document.querySelector("#back-worlds").addEventListener("click", async () => {
    await api("/api/projects/close", { method: "POST" });
    showHome();
    await refresh();
  });
};
