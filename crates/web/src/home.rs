//! Home screen: project list, create world, first-run flows (D-94 B4).

use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;

use crate::api::{
    load_elevation, load_geology, load_map, load_rivers, persist_build_draft, refresh_projects,
};
use crate::canvas::schedule_redraw;
use crate::dom::{
    document, hide_post_finish_note, input, perf_now, select_value, set_select_value, set_text,
    show_view, sync_preset_size_warning, window,
};
use crate::state::{
    AppState, CreateProjectInput, DeleteProjectInput,
    ForgetProjectInput, OpenProjectInput, ProjectStatus,
};
use crate::wizard::{
    build_step_label, close_build_wizard, ensure_wizard_recipe, open_build_wizard,
    preset_id_for_bounds, set_wizard_status, sync_wizard_actions,
};
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{Element, HtmlElement};

pub(crate) fn render_project_list(projects: &[ProjectStatus], state: &Rc<RefCell<AppState>>) {
    let document = document();
    let Some(list) = document.get_element_by_id("project-list") else {
        return;
    };
    let empty = document.get_element_by_id("project-empty");
    let first_world_cta = document.get_element_by_id("first-world-cta");
    let create_wrap = document.get_element_by_id("create-wrap");

    if projects.is_empty() {
        list.set_inner_html("");
        if let Some(empty) = empty {
            let _ = empty.class_list().add_1("visible");
        }
        if let Some(cta) = first_world_cta {
            let _ = cta.class_list().add_1("visible");
        }
        if let Some(wrap) = create_wrap {
            let _ = wrap.class_list().add_1("demoted");
        }
        if let Some(btn) = document.get_element_by_id("first-world-advanced") {
            btn.set_text_content(Some("Advanced options"));
        }
        sync_first_world_defaults(state, projects);
        return;
    }
    if let Some(empty) = empty {
        let _ = empty.class_list().remove_1("visible");
    }
    if let Some(cta) = first_world_cta {
        let _ = cta.class_list().remove_1("visible");
    }
    if let Some(wrap) = create_wrap {
        let _ = wrap.class_list().remove_1("demoted");
    }

    let mut html = String::new();
    for p in projects {
        let missing = if !p.valid {
            "<div class=\"missing\">folder not found</div>"
        } else if p.legacy_map {
            "<div class=\"missing\">legacy map тАФ no map/manifest.json</div>"
        } else {
            ""
        };
        let actions = if p.valid {
            let draft_attr = if p.build_draft { "1" } else { "0" };
            format!(
                "<button class=\"open-btn\" data-path=\"{path}\" data-build-draft=\"{draft_attr}\" data-build-step=\"{step}\" type=\"button\">Open</button><button class=\"manage-btn\" type=\"button\">Manage</button>",
                path = html_escape(&p.path),
                draft_attr = draft_attr,
                step = p.build_step.unwrap_or(3)
            )
        } else {
            format!(
                "<button class=\"remove-btn\" data-path=\"{path}\" type=\"button\">Remove</button>",
                path = html_escape(&p.path)
            )
        };
        let manage_row = if p.valid {
            format!(
                "<div class=\"manage-row\"><button class=\"remove-btn\" data-path=\"{path}\" type=\"button\">Remove</button><button class=\"delete-btn\" data-path=\"{path}\" type=\"button\">DeleteтАж</button></div>",
                path = html_escape(&p.path)
            )
        } else {
            String::new()
        };
        let draft_badge = if p.build_draft {
            "<span class=\"badge draft-badge\">Draft</span>"
        } else {
            ""
        };
        let build_hint = if p.build_draft {
            format!(
                "<div class=\"build-hint\">{}</div>",
                build_step_label(p.build_step.unwrap_or(3))
            )
        } else {
            String::new()
        };
        html.push_str(&format!(
            "<li data-path=\"{path}\"><div class=\"main-row\"><div class=\"info\"><span class=\"id\">{id}</span>{draft_badge}{build_hint}<span class=\"path\">{path}</span>{missing}</div><div class=\"actions\">{actions}</div></div>{manage_row}</li>",
            id = html_escape(&p.id),
            path = html_escape(&p.path),
            draft_badge = draft_badge,
            build_hint = build_hint,
            missing = missing,
            actions = actions,
            manage_row = manage_row,
        ));
    }
    list.set_inner_html(&html);
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn suggested_world_path(state: &AppState, world_id: &str) -> Option<String> {
    let root = state.default_worlds_root.as_ref()?;
    let tail = if world_id.trim().is_empty() {
        "my-world"
    } else {
        world_id.trim()
    };
    let sep = if root.ends_with('\\') || root.ends_with('/') {
        ""
    } else {
        "\\"
    };
    Some(format!("{root}{sep}{tail}"))
}

pub(crate) fn refresh_suggested_path(state: &Rc<RefCell<AppState>>) {
    let state_ref = state.borrow();
    if !state_ref.path_touched {
        let id = input("new-id").value();
        if let Some(path) = suggested_world_path(&state_ref, &id) {
            input("new-path").set_value(&path);
        }
    }
    if !state_ref.build_path_touched {
        let id = input("generate-id").value();
        if let Some(path) = suggested_world_path(&state_ref, &id) {
            input("generate-path").set_value(&path);
        }
    }
}

fn slugify_world_id(name: &str) -> String {
    let mut out = String::new();
    let mut prev_dash = false;
    for ch in name.chars() {
        let lower = ch.to_ascii_lowercase();
        if lower.is_ascii_alphanumeric() {
            out.push(lower);
            prev_dash = false;
        } else if !prev_dash {
            out.push('-');
            prev_dash = true;
        }
    }
    let trimmed = out.trim_matches('-');
    if trimmed.is_empty() {
        "my-world".to_string()
    } else {
        trimmed.to_string()
    }
}

fn pick_unique_first_world_id_and_path(
    state: &Rc<RefCell<AppState>>,
    projects: &[ProjectStatus],
) -> (String, String) {
    let base = slugify_world_id("My First World");
    let used_ids: HashSet<String> = projects.iter().map(|p| p.id.to_ascii_lowercase()).collect();
    let used_paths: HashSet<String> = projects
        .iter()
        .map(|p| p.path.to_ascii_lowercase())
        .collect();
    for n in 1..1000 {
        let candidate = if n == 1 {
            base.clone()
        } else {
            format!("{base}-{n}")
        };
        if used_ids.contains(&candidate) {
            continue;
        }
        let path = {
            let state_ref = state.borrow();
            suggested_world_path(&state_ref, &candidate).unwrap_or_default()
        };
        if path.is_empty() || !used_paths.contains(&path.to_ascii_lowercase()) {
            return (candidate, path);
        }
    }
    let fallback_id = format!("{base}-{}", perf_now() as u64);
    let fallback_path = {
        let state_ref = state.borrow();
        suggested_world_path(&state_ref, &fallback_id).unwrap_or_default()
    };
    (fallback_id, fallback_path)
}

fn sync_first_world_defaults(state: &Rc<RefCell<AppState>>, projects: &[ProjectStatus]) {
    let (id, path) = pick_unique_first_world_id_and_path(state, projects);
    if let Some(btn) = document().get_element_by_id("first-world-start") {
        let _ = btn.set_attribute("data-default-id", &id);
        let _ = btn.set_attribute("data-default-path", &path);
    }
    set_text(
        "first-world-hint",
        &format!("Start Build World with defaults ({id} in Documents, Small map) тАФ then adjust if needed."),
    );
}

pub fn attach_preset_warn_handlers() {
    for (select_id, warn_id) in [
        ("new-preset", "new-preset-warn"),
        ("generate-preset", "generate-preset-warn"),
        ("wiz-preset", "wiz-preset-warn"),
    ] {
        sync_preset_size_warning(select_id, warn_id);
        let select_id = select_id.to_string();
        let warn_id = warn_id.to_string();
        let select_for_change = select_id.clone();
        let warn_for_change = warn_id.clone();
        let on_change = Closure::<dyn FnMut()>::new(move || {
            sync_preset_size_warning(&select_for_change, &warn_for_change);
        });
        if let Ok(Some(select)) = document().query_selector(&format!("#{select_id}")) {
            let _ = select
                .add_event_listener_with_callback("change", on_change.as_ref().unchecked_ref());
        }
        on_change.forget();
    }
}
pub fn attach_create_click(state: Rc<RefCell<AppState>>) {
    let closure = Closure::<dyn FnMut()>::new(move || {
        let id = input("new-id").value();
        let path = input("new-path").value();
        if id.trim().is_empty() || path.trim().is_empty() {
            set_text("home-status", "World name and folder are both required.");
            return;
        }
        let preset = select_value("new-preset");
        let state = state.clone();
        set_text("home-status", "CreatingтАж");
        wasm_bindgen_futures::spawn_local(async move {
            let body = CreateProjectInput {
                id: &id,
                path: &path,
                map_preset: &preset,
                build_wizard: None,
            };
            let sent = gloo_net::http::Request::post("/api/projects").json(&body);
            let sent = match sent {
                Ok(req) => req.send().await,
                Err(err) => {
                    set_text("home-status", &format!("Error: {err}"));
                    return;
                }
            };
            match sent {
                Ok(resp) if resp.ok() => {
                    set_text("home-status", "");
                    input("new-id").set_value("");
                    state.borrow_mut().path_touched = false;
                    refresh_suggested_path(&state);
                    show_view("editor");
                    wasm_bindgen_futures::spawn_local(load_map(state));
                }
                Ok(resp) => {
                    let msg = resp
                        .text()
                        .await
                        .unwrap_or_else(|_| "create failed".to_string());
                    set_text("home-status", &msg);
                }
                Err(err) => set_text("home-status", &format!("Error: {err}")),
            }
        });
    });
    document()
        .get_element_by_id("create")
        .expect("missing #create")
        .add_event_listener_with_callback("click", closure.as_ref().unchecked_ref())
        .expect("attaching create handler");
    closure.forget();
}

pub fn attach_build_start_click(state: Rc<RefCell<AppState>>) {
    let closure = Closure::<dyn FnMut()>::new(move || {
        let id = input("generate-id").value();
        let path = input("generate-path").value();
        if id.trim().is_empty() || path.trim().is_empty() {
            set_text(
                "generate-status",
                "World name and folder are both required.",
            );
            return;
        }
        let preset = select_value("generate-preset");
        let state = state.clone();
        set_text("generate-status", "CreatingтАж");
        wasm_bindgen_futures::spawn_local(async move {
            let body = CreateProjectInput {
                id: &id,
                path: &path,
                map_preset: &preset,
                build_wizard: Some(true),
            };
            let sent = gloo_net::http::Request::post("/api/projects").json(&body);
            let sent = match sent {
                Ok(req) => req.send().await,
                Err(err) => {
                    set_text("generate-status", &format!("Error: {err}"));
                    return;
                }
            };
            match sent {
                Ok(resp) if resp.ok() => {
                    set_text("generate-status", "");
                    input("generate-id").set_value("");
                    state.borrow_mut().build_path_touched = false;
                    refresh_suggested_path(&state);
                    show_view("editor");
                    load_map(state.clone()).await;
                    open_build_wizard();
                    {
                        let mut s = state.borrow_mut();
                        s.wizard_step = 1;
                        s.wizard_layout_class = "pangea".to_string();
                        s.wizard_regenerate_nonce = 0;
                        s.wizard_recipe_id.clear();
                        s.wizard_gen_seed = None;
                        s.wizard_accepted = false;
                        s.wizard_edit_mode = false;
                        s.show_grid = true;
                        if let Some(id) = preset_id_for_bounds(&s.map_bounds) {
                            set_select_value("wiz-preset", id);
                        } else {
                            set_select_value("wiz-preset", &preset);
                        }
                        sync_preset_size_warning("wiz-preset", "wiz-preset-warn");
                        sync_wizard_actions(&s);
                    }
                    let _ = persist_build_draft(1).await;
                    set_wizard_status("Confirm map size on the blank grid, then continue.");
                    schedule_redraw(state.clone());
                }
                Ok(resp) => {
                    let msg = resp
                        .text()
                        .await
                        .unwrap_or_else(|_| "create failed".to_string());
                    set_text("generate-status", &msg);
                }
                Err(err) => set_text("generate-status", &format!("Error: {err}")),
            }
        });
    });
    document()
        .get_element_by_id("build-start")
        .expect("missing #build-start")
        .add_event_listener_with_callback("click", closure.as_ref().unchecked_ref())
        .expect("attaching build-start handler");
    closure.forget();
}
pub fn attach_new_id_input(state: Rc<RefCell<AppState>>) {
    let closure = Closure::<dyn FnMut(web_sys::Event)>::new(move |_| {
        refresh_suggested_path(&state);
    });
    let _ =
        input("new-id").add_event_listener_with_callback("input", closure.as_ref().unchecked_ref());
    closure.forget();
}

pub fn attach_new_path_input(state: Rc<RefCell<AppState>>) {
    let closure = Closure::<dyn FnMut(web_sys::Event)>::new(move |_| {
        state.borrow_mut().path_touched = true;
    });
    let _ = input("new-path")
        .add_event_listener_with_callback("input", closure.as_ref().unchecked_ref());
    closure.forget();
}

pub fn attach_generate_id_input(state: Rc<RefCell<AppState>>) {
    let closure = Closure::<dyn FnMut(web_sys::Event)>::new(move |_| {
        refresh_suggested_path(&state);
    });
    let _ = input("generate-id")
        .add_event_listener_with_callback("input", closure.as_ref().unchecked_ref());
    closure.forget();
}

pub fn attach_generate_path_input(state: Rc<RefCell<AppState>>) {
    let closure = Closure::<dyn FnMut(web_sys::Event)>::new(move |_| {
        state.borrow_mut().build_path_touched = true;
    });
    let _ = input("generate-path")
        .add_event_listener_with_callback("input", closure.as_ref().unchecked_ref());
    closure.forget();
}

pub fn attach_browse_folder_click(state: Rc<RefCell<AppState>>) {
    if let Some(button) = document().get_element_by_id("browse-folder") {
        let state = state.clone();
        let closure = Closure::<dyn FnMut()>::new(move || {
            let state = state.clone();
            wasm_bindgen_futures::spawn_local(async move {
                if let Some(path) = pick_folder_via_tauri().await {
                    input("new-path").set_value(&path);
                    state.borrow_mut().path_touched = true;
                }
            });
        });
        let _ = button.add_event_listener_with_callback("click", closure.as_ref().unchecked_ref());
        closure.forget();
    }
    if let Some(button) = document().get_element_by_id("browse-generate-folder") {
        let state = state.clone();
        let closure = Closure::<dyn FnMut()>::new(move || {
            let state = state.clone();
            wasm_bindgen_futures::spawn_local(async move {
                if let Some(path) = pick_folder_via_tauri().await {
                    input("generate-path").set_value(&path);
                    state.borrow_mut().build_path_touched = true;
                }
            });
        });
        let _ = button.add_event_listener_with_callback("click", closure.as_ref().unchecked_ref());
        closure.forget();
    }
}

/// Calls the `window.mapkeeperPickFolder()` bridge defined in `index.html`
/// (only present inside the Tauri shell) via `js_sys`, so this crate has no
/// direct Tauri dependency тАФ it stays a plain WASM/web-sys build either way.
async fn pick_folder_via_tauri() -> Option<String> {
    let bridge = js_sys::Reflect::get(&window(), &JsValue::from_str("mapkeeperPickFolder")).ok()?;
    let bridge: js_sys::Function = bridge.dyn_into().ok()?;
    let promise: js_sys::Promise = bridge.call0(&window()).ok()?.dyn_into().ok()?;
    let result = wasm_bindgen_futures::JsFuture::from(promise).await.ok()?;
    result.as_string()
}

// tester-first-run-flow-0.2: empty-home primary CTA + demoted advanced create path.
pub fn attach_first_world_handlers(state: Rc<RefCell<AppState>>) {
    if let Some(btn) = document().get_element_by_id("first-world-start") {
        let state = state.clone();
        let closure = Closure::<dyn FnMut()>::new(move || {
            let button = document()
                .get_element_by_id("first-world-start")
                .expect("missing #first-world-start");
            if let Some(id) = button.get_attribute("data-default-id") {
                input("generate-id").set_value(&id);
            }
            if let Some(path) = button.get_attribute("data-default-path") {
                input("generate-path").set_value(&path);
                state.borrow_mut().build_path_touched = false;
            }
            set_select_value("generate-preset", "small");
            if let Some(start) = document().get_element_by_id("build-start") {
                if let Ok(start) = start.dyn_into::<HtmlElement>() {
                    start.click();
                }
            }
        });
        let _ = btn.add_event_listener_with_callback("click", closure.as_ref().unchecked_ref());
        closure.forget();
    }
    if let Some(btn) = document().get_element_by_id("first-world-advanced") {
        let closure = Closure::<dyn FnMut()>::new(move || {
            let Some(wrap) = document().get_element_by_id("create-wrap") else {
                return;
            };
            if wrap.class_list().contains("demoted") {
                let _ = wrap.class_list().remove_1("demoted");
                set_text(
                    "first-world-hint",
                    "Defaults ready above. Advanced create options are now visible below.",
                );
                if let Some(btn) = document().get_element_by_id("first-world-advanced") {
                    btn.set_text_content(Some("Hide advanced options"));
                }
            } else {
                let _ = wrap.class_list().add_1("demoted");
                set_text(
                    "first-world-hint",
                    "Start Build World with defaults (Small map in Documents) тАФ then adjust if needed.",
                );
                if let Some(btn) = document().get_element_by_id("first-world-advanced") {
                    btn.set_text_content(Some("Advanced options"));
                }
            }
        });
        let _ = btn.add_event_listener_with_callback("click", closure.as_ref().unchecked_ref());
        closure.forget();
    }
}
pub fn attach_post_finish_note_dismiss() {
    let Some(btn) = document().get_element_by_id("post-finish-dismiss") else {
        return;
    };
    let closure = Closure::<dyn FnMut()>::new(move || hide_post_finish_note());
    let _ = btn.add_event_listener_with_callback("click", closure.as_ref().unchecked_ref());
    closure.forget();
}
pub fn attach_project_list_click(state: Rc<RefCell<AppState>>) {
    let closure = Closure::<dyn FnMut(web_sys::MouseEvent)>::new(
        move |event: web_sys::MouseEvent| {
            let Some(target) = event.target().and_then(|t| t.dyn_into::<Element>().ok()) else {
                return;
            };
            if let Ok(Some(button)) = target.closest(".manage-btn") {
                let Some(row) = button.closest("li").ok().flatten() else {
                    return;
                };
                if row.class_list().contains("manage-open") {
                    let _ = row.class_list().remove_1("manage-open");
                } else {
                    let _ = row.class_list().add_1("manage-open");
                }
                return;
            }
            if let Ok(Some(button)) = target.closest(".delete-btn") {
                let Some(path) = button.get_attribute("data-path") else {
                    return;
                };
                if !button.class_list().contains("armed") {
                    let _ = button.class_list().add_1("armed");
                    button.set_text_content(Some("Delete now"));
                    set_text(
                        "home-status",
                        "Click \"Delete now\" again to permanently remove this world from disk.",
                    );
                    return;
                }
                let state = state.clone();
                set_text("home-status", "DeletingтАж");
                wasm_bindgen_futures::spawn_local(async move {
                    let body = DeleteProjectInput { path: &path };
                    let sent = gloo_net::http::Request::post("/api/projects/delete").json(&body);
                    let sent = match sent {
                        Ok(req) => req.send().await,
                        Err(err) => {
                            set_text("home-status", &format!("Error: {err}"));
                            return;
                        }
                    };
                    match sent {
                        Ok(resp) if resp.ok() => {
                            set_text("home-status", "");
                            wasm_bindgen_futures::spawn_local(refresh_projects(state));
                        }
                        Ok(resp) => {
                            let msg = resp
                                .text()
                                .await
                                .unwrap_or_else(|_| "delete failed".to_string());
                            set_text("home-status", &msg);
                        }
                        Err(err) => set_text("home-status", &format!("Error: {err}")),
                    }
                });
                return;
            }
            if let Ok(Some(button)) = target.closest(".remove-btn") {
                let Some(path) = button.get_attribute("data-path") else {
                    return;
                };
                let state = state.clone();
                set_text("home-status", "Removing from launcherтАж");
                wasm_bindgen_futures::spawn_local(async move {
                    let body = ForgetProjectInput { path: &path };
                    let sent = gloo_net::http::Request::post("/api/projects/forget").json(&body);
                    let sent = match sent {
                        Ok(req) => req.send().await,
                        Err(err) => {
                            set_text("home-status", &format!("Error: {err}"));
                            return;
                        }
                    };
                    match sent {
                        Ok(resp) if resp.ok() => {
                            set_text("home-status", "");
                            wasm_bindgen_futures::spawn_local(refresh_projects(state));
                        }
                        Ok(resp) => {
                            let msg = resp
                                .text()
                                .await
                                .unwrap_or_else(|_| "remove failed".to_string());
                            set_text("home-status", &msg);
                        }
                        Err(err) => set_text("home-status", &format!("Error: {err}")),
                    }
                });
                return;
            }

            let Ok(Some(button)) = target.closest(".open-btn") else {
                return;
            };
            let Some(path) = button.get_attribute("data-path") else {
                return;
            };
            let resume_wizard = button.get_attribute("data-build-draft").as_deref() == Some("1");
            let resume_step = button
                .get_attribute("data-build-step")
                .and_then(|s| s.parse::<u32>().ok())
                .unwrap_or(1)
                .clamp(1, 4);

            let state = state.clone();
            set_text("home-status", "OpeningтАж");
            wasm_bindgen_futures::spawn_local(async move {
                let body = OpenProjectInput { path: &path };
                let sent = gloo_net::http::Request::post("/api/projects/open").json(&body);
                let sent = match sent {
                    Ok(req) => req.send().await,
                    Err(err) => {
                        set_text("home-status", &format!("Error: {err}"));
                        return;
                    }
                };
                match sent {
                    Ok(resp) if resp.ok() => {
                        set_text("home-status", "");
                        show_view("editor");
                        load_map(state.clone()).await;
                        if resume_wizard {
                            open_build_wizard();
                            {
                                let mut s = state.borrow_mut();
                                s.wizard_step = resume_step;
                                s.wizard_accepted = resume_step > 2;
                                s.wizard_edit_mode = false;
                                s.wizard_geo_accepted = resume_step > 3;
                                s.show_grid = resume_step <= 1;
                                if let Some(id) = preset_id_for_bounds(&s.map_bounds) {
                                    set_select_value("wiz-preset", id);
                                }
                                sync_preset_size_warning("wiz-preset", "wiz-preset-warn");
                                if resume_step >= 2 {
                                    ensure_wizard_recipe(&mut s);
                                }
                                sync_wizard_actions(&s);
                            }
                            match resume_step {
                                1 => {
                                    set_wizard_status(
                                        "Resumed at size тАФ confirm scale on the blank grid, then continue.",
                                    );
                                    schedule_redraw(state.clone());
                                }
                                3 => {
                                    set_wizard_status(
                                        "Resumed at tectonics тАФ generate or accept geology.",
                                    );
                                    wasm_bindgen_futures::spawn_local(async move {
                                        load_geology(&state).await;
                                        schedule_redraw(state.clone());
                                    });
                                }
                                4 => {
                                    set_wizard_status(
                                        "Resumed at elevation тАФ generate or continue to climate.",
                                    );
                                    wasm_bindgen_futures::spawn_local(async move {
                                        load_geology(&state).await;
                                        load_elevation(&state).await;
                                        schedule_redraw(state.clone());
                                    });
                                }
                                5 => {
                                    set_wizard_status(
                                        "Resumed at climate тАФ generate or continue to water.",
                                    );
                                    wasm_bindgen_futures::spawn_local(async move {
                                        load_geology(&state).await;
                                        load_elevation(&state).await;
                                        schedule_redraw(state.clone());
                                    });
                                }
                                6 => {
                                    set_wizard_status(
                                        "Resumed at water тАФ generate rivers or Finish.",
                                    );
                                    wasm_bindgen_futures::spawn_local(async move {
                                        load_geology(&state).await;
                                        load_elevation(&state).await;
                                        load_rivers(&state).await;
                                        schedule_redraw(state.clone());
                                    });
                                }
                                _ => {
                                    set_wizard_status("Resumed at land silhouette.");
                                    schedule_redraw(state.clone());
                                }
                            }
                        } else {
                            close_build_wizard();
                        }
                    }
                    Ok(resp) => {
                        let msg = resp
                            .text()
                            .await
                            .unwrap_or_else(|_| "open failed".to_string());
                        set_text("home-status", &msg);
                    }
                    Err(err) => set_text("home-status", &format!("Error: {err}")),
                }
            });
        },
    );
    document()
        .get_element_by_id("project-list")
        .expect("missing #project-list")
        .add_event_listener_with_callback("click", closure.as_ref().unchecked_ref())
        .expect("attaching project-list handler");
    closure.forget();
}
