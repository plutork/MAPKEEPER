//! DOM/query helpers for the WASM UI (D-94 B1).

use wasm_bindgen::JsCast;
use web_sys::{
    CanvasRenderingContext2d, Element, HtmlCanvasElement, HtmlInputElement, HtmlSelectElement,
    HtmlTextAreaElement,
};

pub(crate) fn perf_now() -> f64 {
    web_sys::window()
        .and_then(|w| w.performance())
        .map(|p| p.now())
        .unwrap_or_else(js_sys::Date::now)
}
pub(crate) fn window() -> web_sys::Window {
    web_sys::window().expect("no window")
}

pub(crate) fn document() -> web_sys::Document {
    window().document().expect("no document")
}

/// Click target may be a text node inside a button — walk up to the element.
pub(crate) fn click_target_element(event: &web_sys::MouseEvent) -> Option<web_sys::Element> {
    let target = event.target()?;
    if let Ok(el) = target.clone().dyn_into::<web_sys::Element>() {
        return Some(el);
    }
    target.dyn_into::<web_sys::Node>().ok()?.parent_element()
}

pub(crate) fn canvas() -> HtmlCanvasElement {
    document()
        .get_element_by_id("map")
        .expect("#map canvas missing")
        .dyn_into::<HtmlCanvasElement>()
        .expect("#map is not a canvas")
}

pub(crate) fn context() -> CanvasRenderingContext2d {
    canvas()
        .get_context("2d")
        .ok()
        .flatten()
        .expect("no 2d context")
        .dyn_into::<CanvasRenderingContext2d>()
        .expect("context is not 2d")
}

pub(crate) fn input(id: &str) -> HtmlInputElement {
    document()
        .get_element_by_id(id)
        .expect("missing input")
        .dyn_into()
        .expect("not an input")
}

pub(crate) fn textarea(id: &str) -> HtmlTextAreaElement {
    document()
        .get_element_by_id(id)
        .expect("missing textarea")
        .dyn_into()
        .expect("not a textarea")
}

pub(crate) fn set_text(id: &str, text: &str) {
    if let Some(el) = document().get_element_by_id(id) {
        el.set_text_content(Some(text));
    }
}

/// Dogfood diagnostics — wizard step 6 + editor rivers drawer.
pub(crate) fn set_water_diagnostics(text: &str) {
    for id in ["water-gen-diagnostics", "wizard-water-diagnostics"] {
        set_text(id, text);
    }
}

/// Active preset button value inside a container (`[attr].active`).
pub(crate) fn active_attr_in_group(container_id: &str, attr: &str, default: &str) -> String {
    let selector = format!("#{container_id} [{attr}].active");
    document()
        .query_selector(&selector)
        .ok()
        .flatten()
        .and_then(|el| el.get_attribute(attr))
        .unwrap_or_else(|| default.to_string())
}

pub(crate) fn toggle_active_in_group(container_id: &str, attr: &str, active: &Element) {
    let Ok(Some(root)) = document().query_selector(&format!("#{container_id}")) else {
        return;
    };
    if let Ok(list) = root.query_selector_all(&format!("[{attr}]")) {
        for i in 0..list.length() {
            if let Some(node) = list.item(i) {
                if let Ok(el) = node.dyn_into::<Element>() {
                    let _ = el.class_list().remove_1("active");
                }
            }
        }
    }
    let _ = active.class_list().add_1("active");
}
pub(crate) fn set_drawer_open(open: bool) {
    if workspace_panels_pinned() {
        if let Some(drawer) = document().get_element_by_id("dock-drawer") {
            let _ = drawer.class_list().remove_1("collapsed");
        }
        return;
    }
    if let Some(drawer) = document().get_element_by_id("dock-drawer") {
        if open {
            let _ = drawer.class_list().remove_1("collapsed");
        } else {
            let _ = drawer.class_list().add_1("collapsed");
        }
    }
}

pub(crate) fn workspace_panels_pinned() -> bool {
    document()
        .get_element_by_id("editor")
        .is_some_and(|el| el.class_list().contains("workspace-active"))
}

pub(crate) fn drawer_is_open() -> bool {
    if workspace_panels_pinned() {
        return true;
    }
    document()
        .get_element_by_id("dock-drawer")
        .is_some_and(|drawer| !drawer.class_list().contains("collapsed"))
}

pub(crate) fn set_dock_tab(tab: &str) {
    if let Some(drawer) = document().get_element_by_id("dock-drawer") {
        if let Ok(panes) = drawer.query_selector_all("[data-drawer]") {
            for i in 0..panes.length() {
                if let Some(node) = panes.item(i) {
                    if let Ok(el) = node.dyn_into::<Element>() {
                        let active = el.get_attribute("data-drawer").as_deref() == Some(tab);
                        if active {
                            let _ = el.class_list().add_1("active");
                        } else {
                            let _ = el.class_list().remove_1("active");
                        }
                    }
                }
            }
        }
    }
    if let Some(rail) = document().get_element_by_id("dock-rail") {
        if let Ok(items) = rail.query_selector_all("[data-dock]") {
            for i in 0..items.length() {
                if let Some(node) = items.item(i) {
                    if let Ok(el) = node.dyn_into::<Element>() {
                        let active = el.get_attribute("data-dock").as_deref() == Some(tab);
                        if active {
                            let _ = el.class_list().add_1("active");
                        } else {
                            let _ = el.class_list().remove_1("active");
                        }
                    }
                }
            }
        }
    }
}
pub(crate) fn set_world_label(world_id: &str) {
    // Same value in both surfaces: read-only display (Settings ▾) and the
    // compact top-bar label. Neither is editable (ui-shell-redesign Track 1).
    set_text("world-name", world_id);
    set_text("workspace-world-label", world_id);
}

/// Toggle between the Home (project picker) and Editor (hex map) screens.
pub(crate) fn show_view(name: &str) {
    if name == "editor" {
        if let Some(el) = document().get_element_by_id("editor") {
            let _ = el.class_list().add_1("workspace-active");
            if !el.class_list().contains("workspace-build") {
                let _ = el.class_list().add_1("workspace-mode-editor");
            }
            set_dock_tab("inspect");
            set_drawer_open(true);
        }
    } else if name != "editor" {
        if let Some(el) = document().get_element_by_id("editor") {
            let _ = el.class_list().remove_1("workspace-active");
        }
    }
    for id in ["home", "editor"] {
        if let Some(el) = document().get_element_by_id(id) {
            if id == name {
                let _ = el.class_list().add_1("active");
            } else {
                let _ = el.class_list().remove_1("active");
            }
        }
    }
    if name != "editor" {
        hide_post_finish_note();
    }
}
pub(crate) fn hide_wiz_confirm() {
    if let Some(el) = document().get_element_by_id("wiz-confirm-overlay") {
        let _ = el.class_list().remove_1("open");
    }
}

pub(crate) fn show_wiz_confirm(msg: &str) {
    set_text("wiz-confirm-msg", msg);
    if let Some(el) = document().get_element_by_id("wiz-confirm-overlay") {
        let _ = el.class_list().add_1("open");
    }
}
pub(crate) fn set_select_value(id: &str, value: &str) {
    if let Some(el) = document().get_element_by_id(id) {
        if let Ok(select) = el.dyn_into::<web_sys::HtmlSelectElement>() {
            select.set_value(value);
        }
    }
}
pub(crate) fn select_value(id: &str) -> String {
    document()
        .get_element_by_id(id)
        .expect("missing select")
        .dyn_into::<HtmlSelectElement>()
        .expect("not a select")
        .value()
}
pub(crate) fn hide_post_finish_note() {
    let Some(note) = document().get_element_by_id("post-finish-note") else {
        return;
    };
    let _ = note.class_list().remove_1("visible");
}

pub(crate) fn show_post_finish_note() {
    let Some(note) = document().get_element_by_id("post-finish-note") else {
        return;
    };
    let _ = note.class_list().add_1("visible");
}

/// Clear inspect panel fields (editor + wizard return home).
pub(crate) fn clear_inspect_panel() {
    input("title").set_value("");
    textarea("notes").set_value("");
    input("title").set_disabled(true);
    textarea("notes").set_disabled(true);
    set_text("status", "");
}

/// Grand/World preset warnings on Home and wizard (D-49).
pub(crate) fn sync_preset_size_warning(select_id: &str, warn_id: &str) {
    let preset = select_value(select_id);
    let Some(el) = document().get_element_by_id(warn_id) else {
        return;
    };
    let (text, class) = match preset.as_str() {
        "grand" => (
            "Large map — performance may vary on your machine.",
            "preset-warn-yellow",
        ),
        "world" => (
            "Experimental map size (not stable) — expect slowdowns.",
            "preset-warn-red",
        ),
        _ => ("", ""),
    };
    el.set_text_content(if text.is_empty() { None } else { Some(text) });
    let _ = el.class_list().remove_1("preset-warn-yellow");
    let _ = el.class_list().remove_1("preset-warn-red");
    if !class.is_empty() {
        let _ = el.class_list().add_1(class);
    }
}
