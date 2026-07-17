//! History timeline UI (D-107 tracks A–C).

use std::cell::RefCell;
use std::rc::Rc;

use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::Element;

use crate::api::{load_elevation, load_history, load_map, scoped_request};
use crate::dom::{document, set_text};
use crate::state::{AppState, DivergenceReviewWire, HistoryEventWire, HistoryStateWire};

fn el(id: &str) -> Element {
    document()
        .get_element_by_id(id)
        .unwrap_or_else(|| panic!("missing #{id}"))
}

fn set_hidden(node: &Element, hidden: bool) {
    if hidden {
        let _ = node.class_list().add_1("hidden");
    } else {
        let _ = node.class_list().remove_1("hidden");
    }
}

enum TimelineKind {
    State,
    Event,
}

struct TimelineRow {
    time_key: i64,
    kind: TimelineKind,
    state: Option<HistoryStateWire>,
    event: Option<HistoryEventWire>,
}

pub(crate) fn sync_history_ui(state: &AppState) {
    let editor = el("editor");
    let unlock_btn = el("unlock-history-btn");
    let bar = el("history-bar");
    let panel = el("history-panel");
    let divergence = el("history-divergence-banner");

    let in_wizard = editor.class_list().contains("workspace-mode-wizard")
        || editor.class_list().contains("workspace-build");
    // ui-shell-redesign Track 1: legacy D-107 availability locus. `build_draft_active`
    // stays out of WorkspaceMode; removing draft lifecycle later replaces only this gate.
    let show_unlock = !state.build_draft_active
        && !state.history_enabled
        && state.history_unlock_available
        && !in_wizard;

    set_hidden(&unlock_btn, !show_unlock);

    if !state.history_enabled {
        set_hidden(&bar, true);
        return;
    }

    set_hidden(&bar, false);
    set_hidden(&panel, !state.history_expanded);

    let selected = state
        .history_states
        .iter()
        .find(|s| s.id == state.history_selected_id);
    let label = selected
        .map(|s| {
            format!(
                "{} · {} · History {}",
                s.display_date,
                s.name,
                if state.history_expanded { "▾" } else { "▴" }
            )
        })
        .unwrap_or_else(|| "History ▴".to_string());
    set_text("history-collapsed-label", &label);

    set_hidden(&divergence, state.history_divergence.is_empty());
    if !state.history_divergence.is_empty() {
        set_text(
            "history-divergence-text",
            &format!(
                "Cross-epoch history divergence (not Wizard generation stale): {}",
                state.history_divergence.join(", ")
            ),
        );
    }

    render_divergence_review(state);
    set_hidden(
        &el("history-delete-state-btn"),
        !state.history_selected_can_delete,
    );

    render_timeline(state);
    if let Some(sel) = selected {
        el("history-lock-btn").set_text_content(Some(if sel.locked {
            "Unlock state"
        } else {
            "Lock state"
        }));
    }
}

fn timeline_rows(state: &AppState) -> Vec<TimelineRow> {
    let mut rows: Vec<TimelineRow> = state
        .history_states
        .iter()
        .map(|s| TimelineRow {
            time_key: s.time_key,
            kind: TimelineKind::State,
            state: Some(s.clone()),
            event: None,
        })
        .collect();
    for e in &state.history_events {
        rows.push(TimelineRow {
            time_key: e.time_key,
            kind: TimelineKind::Event,
            state: None,
            event: Some(e.clone()),
        });
    }
    rows.sort_by(|a, b| {
        a.time_key
            .cmp(&b.time_key)
            .then_with(|| match (&a.kind, &b.kind) {
                (TimelineKind::State, TimelineKind::Event) => std::cmp::Ordering::Less,
                (TimelineKind::Event, TimelineKind::State) => std::cmp::Ordering::Greater,
                _ => std::cmp::Ordering::Equal,
            })
    });
    rows
}

fn selected_divergence_review(state: &AppState) -> Option<&DivergenceReviewWire> {
    state
        .history_divergence_review
        .iter()
        .find(|r| r.state_id == state.history_selected_id)
}

fn render_divergence_review(state: &AppState) {
    let panel = el("history-divergence-review");
    panel.set_inner_html("");
    let Some(review) = selected_divergence_review(state) else {
        set_hidden(&panel, true);
        return;
    };
    set_hidden(&panel, false);
    let doc = document();
    for domain in &review.domains {
        let row = doc.create_element("div").expect("div");
        let _ = row.class_list().add_1("history-divergence-row");
        let _ = row.set_attribute("data-domain", &domain.domain);

        let msg = doc.create_element("p").expect("p");
        let _ = msg.class_list().add_1("history-divergence-msg");
        let detail = if let Some(name) = &domain.fork_source_state_name {
            format!(
                "{} — {} (inherits {}; forked by \"{}\")",
                domain.domain, domain.message, domain.local_ref, name
            )
        } else {
            format!(
                "{} — {} (ref {})",
                domain.domain, domain.message, domain.local_ref
            )
        };
        msg.set_text_content(Some(&detail));
        if let Some(fork_source_id) = &domain.fork_source_state_id {
            let _ = msg.set_attribute("data-fork-source-id", fork_source_id);
        }
        row.append_child(&msg).ok();

        let actions = doc.create_element("div").expect("div");
        let _ = actions.class_list().add_1("history-divergence-actions");

        let keep = doc.create_element("button").expect("button");
        keep.set_attribute("type", "button").ok();
        let _ = keep.class_list().add_1("history-keep-divergence-btn");
        let _ = keep.set_attribute("data-domain", &domain.domain);
        keep.set_text_content(Some("Keep as-is"));
        actions.append_child(&keep).ok();

        let rebase = doc.create_element("button").expect("button");
        rebase.set_attribute("type", "button").ok();
        let _ = rebase.class_list().add_1("history-rebase-btn");
        let _ = rebase.set_attribute("data-domain", &domain.domain);
        rebase.set_text_content(Some("Rebase to ancestor"));
        actions.append_child(&rebase).ok();

        row.append_child(&actions).ok();
        panel.append_child(&row).ok();
    }

    if state.history_divergence_review.len() > 1 {
        let others = doc.create_element("p").expect("p");
        let _ = others.class_list().add_1("history-divergence-others");
        let names: Vec<String> = state
            .history_divergence_review
            .iter()
            .filter(|r| r.state_id != state.history_selected_id)
            .map(|r| format!("{} {}", r.display_date, r.name))
            .collect();
        others.set_text_content(Some(&format!(
            "Other affected states: {}",
            names.join("; ")
        )));
        panel.append_child(&others).ok();
    }
}

fn render_timeline(state: &AppState) {
    let list = el("history-timeline-list");
    list.set_inner_html("");
    let doc = document();
    for row in timeline_rows(state) {
        match row.kind {
            TimelineKind::State => {
                let s = row.state.expect("state row");
                let btn = doc.create_element("button").expect("button");
                btn.set_attribute("type", "button").ok();
                let _ = btn.class_list().add_1("history-state-btn");
                let _ = btn.set_attribute("data-state-id", &s.id);
                if let Some(parent) = &s.based_on {
                    let _ = btn.set_attribute("data-based-on", parent);
                }
                if s.id == state.history_selected_id {
                    let _ = btn.class_list().add_1("active");
                }
                let mut label = format!("● {} {}", s.display_date, s.name);
                if s.locked {
                    label.push_str(" 🔒");
                }
                if !s.history_divergence.is_empty() {
                    label.push_str(" ⚠");
                }
                btn.set_text_content(Some(&label));
                list.append_child(&btn).ok();
            }
            TimelineKind::Event => {
                let e = row.event.expect("event row");
                let span = doc.create_element("div").expect("div");
                let _ = span.class_list().add_1("history-event-marker");
                let _ = span.set_attribute("data-event-id", &e.id);
                if !e.description.is_empty() {
                    let _ = span.set_attribute("title", &e.description);
                }
                if let Some(anchor) = &e.anchor_state_id {
                    let _ = span.set_attribute("data-anchor-state-id", anchor);
                }
                if let Some(result_state_id) = &e.result_state_id {
                    let _ = span.set_attribute("data-result-state-id", result_state_id);
                }
                let mut label = format!("│ {} {}", e.display_date, e.name);
                if e.change_set_id.is_some() {
                    label.push_str(" ⚡");
                }
                span.set_text_content(Some(&label));
                list.append_child(&span).ok();
            }
        }
    }
}

pub(crate) fn attach_history_handlers(state: Rc<RefCell<AppState>>) {
    {
        let state = state.clone();
        let btn = el("unlock-history-btn");
        let closure = Closure::wrap(Box::new(move || {
            let state = state.clone();
            wasm_bindgen_futures::spawn_local(async move {
                post_unlock(&state).await;
            });
        }) as Box<dyn FnMut()>);
        let _ = btn.add_event_listener_with_callback("click", closure.as_ref().unchecked_ref());
        closure.forget();
    }
    {
        let state = state.clone();
        let btn = el("history-toggle-btn");
        let closure = Closure::wrap(Box::new(move || {
            let mut s = state.borrow_mut();
            s.history_expanded = !s.history_expanded;
            sync_history_ui(&s);
        }) as Box<dyn FnMut()>);
        let _ = btn.add_event_listener_with_callback("click", closure.as_ref().unchecked_ref());
        closure.forget();
    }
    {
        let state = state.clone();
        let btn = el("history-earlier-btn");
        let closure = Closure::wrap(Box::new(move || {
            let state = state.clone();
            wasm_bindgen_futures::spawn_local(async move {
                prompt_add_state(&state, "earlier").await;
            });
        }) as Box<dyn FnMut()>);
        let _ = btn.add_event_listener_with_callback("click", closure.as_ref().unchecked_ref());
        closure.forget();
    }
    {
        let state = state.clone();
        let btn = el("history-later-btn");
        let closure = Closure::wrap(Box::new(move || {
            let state = state.clone();
            wasm_bindgen_futures::spawn_local(async move {
                prompt_add_state(&state, "later").await;
            });
        }) as Box<dyn FnMut()>);
        let _ = btn.add_event_listener_with_callback("click", closure.as_ref().unchecked_ref());
        closure.forget();
    }
    {
        let state = state.clone();
        let btn = el("history-event-btn");
        let closure = Closure::wrap(Box::new(move || {
            let state = state.clone();
            wasm_bindgen_futures::spawn_local(async move {
                prompt_add_event(&state).await;
            });
        }) as Box<dyn FnMut()>);
        let _ = btn.add_event_listener_with_callback("click", closure.as_ref().unchecked_ref());
        closure.forget();
    }
    {
        let state = state.clone();
        let btn = el("history-cataclysm-btn");
        let closure = Closure::wrap(Box::new(move || {
            let state = state.clone();
            wasm_bindgen_futures::spawn_local(async move {
                prompt_cataclysm(&state).await;
            });
        }) as Box<dyn FnMut()>);
        let _ = btn.add_event_listener_with_callback("click", closure.as_ref().unchecked_ref());
        closure.forget();
    }
    {
        let state = state.clone();
        let btn = el("history-lock-btn");
        let closure = Closure::wrap(Box::new(move || {
            let state = state.clone();
            wasm_bindgen_futures::spawn_local(async move {
                toggle_lock(&state).await;
            });
        }) as Box<dyn FnMut()>);
        let _ = btn.add_event_listener_with_callback("click", closure.as_ref().unchecked_ref());
        closure.forget();
    }
    {
        let state = state.clone();
        let panel = el("history-divergence-review");
        let closure = Closure::<dyn FnMut(web_sys::MouseEvent)>::new(move |ev: web_sys::MouseEvent| {
            let target = crate::dom::click_target_element(&ev);
            let mut node = target;
            while let Some(el) = node {
                if el.class_list().contains("history-keep-divergence-btn") {
                    if let Some(domain) = el.get_attribute("data-domain") {
                        let state = state.clone();
                        wasm_bindgen_futures::spawn_local(async move {
                            ack_divergence(&state, Some(&[domain.as_str()])).await;
                        });
                    }
                    break;
                }
                if el.class_list().contains("history-rebase-btn") {
                    if let Some(domain) = el.get_attribute("data-domain") {
                        let state = state.clone();
                        wasm_bindgen_futures::spawn_local(async move {
                            rebase_domain(&state, &domain).await;
                        });
                    }
                    break;
                }
                node = el.parent_element();
            }
        });
        let _ = panel.add_event_listener_with_callback("click", closure.as_ref().unchecked_ref());
        closure.forget();
    }
    {
        let state = state.clone();
        let btn = el("history-delete-state-btn");
        let closure = Closure::wrap(Box::new(move || {
            let state = state.clone();
            wasm_bindgen_futures::spawn_local(async move {
                delete_selected_state(&state).await;
            });
        }) as Box<dyn FnMut()>);
        let _ = btn.add_event_listener_with_callback("click", closure.as_ref().unchecked_ref());
        closure.forget();
    }
    {
        let state = state.clone();
        let list = el("history-timeline-list");
        let closure = Closure::<dyn FnMut(web_sys::MouseEvent)>::new(move |ev: web_sys::MouseEvent| {
            let target = crate::dom::click_target_element(&ev);
            let mut node = target;
            while let Some(el) = node {
                if let Some(id) = el.get_attribute("data-state-id") {
                    let state = state.clone();
                    wasm_bindgen_futures::spawn_local(async move {
                        select_state(&state, &id).await;
                    });
                    break;
                }
                if let Some(id) = el.get_attribute("data-result-state-id") {
                    let state = state.clone();
                    wasm_bindgen_futures::spawn_local(async move {
                        select_state(&state, &id).await;
                    });
                    break;
                }
                node = el.parent_element();
            }
        });
        let _ = list.add_event_listener_with_callback("click", closure.as_ref().unchecked_ref());
        closure.forget();
    }
}

async fn post_unlock(state: &Rc<RefCell<AppState>>) {
    let world_id = state.borrow().scoped_world_id.clone();
    let resp = scoped_request(
        gloo_net::http::Request::post("/api/history/unlock"),
        world_id.as_deref(),
    )
    .send()
    .await;
    if resp.is_ok() {
        load_history(state.clone()).await;
    }
}

async fn select_state(state: &Rc<RefCell<AppState>>, state_id: &str) {
    let world_id = state.borrow().scoped_world_id.clone();
    let url = format!("/api/history/states/{state_id}/select");
    let _ = scoped_request(
        gloo_net::http::Request::post(&url),
        world_id.as_deref(),
    )
    .send()
    .await;
    load_history(state.clone()).await;
    load_map(state.clone()).await;
    load_elevation(state).await;
}

async fn toggle_lock(state: &Rc<RefCell<AppState>>) {
    let (world_id, id, locked) = {
        let s = state.borrow();
        let id = s.history_selected_id.clone();
        let locked = s
            .history_states
            .iter()
            .find(|x| x.id == id)
            .map(|x| !x.locked)
            .unwrap_or(false);
        (s.scoped_world_id.clone(), id, locked)
    };
    let url = format!("/api/history/states/{id}/meta");
    let _ = scoped_request(
        gloo_net::http::Request::put(&url),
        world_id.as_deref(),
    )
    .json(&serde_json::json!({ "locked": locked }))
    .unwrap()
    .send()
    .await;
    load_history(state.clone()).await;
}

async fn prompt_add_state(state: &Rc<RefCell<AppState>>, direction: &str) {
    let (world_id, based_on, pivot_key) = {
        let s = state.borrow();
        let based_on = s.history_selected_id.clone();
        let pivot = s
            .history_states
            .iter()
            .find(|x| x.id == based_on)
            .map(|x| x.time_key)
            .unwrap_or(0);
        (s.scoped_world_id.clone(), based_on, pivot)
    };
    let delta = if direction == "earlier" { -100 } else { 100 };
    let time_key = pivot_key + delta;
    let display_date = format!("{:04}", time_key.abs());
    let name = if direction == "earlier" {
        "Earlier epoch"
    } else {
        "Later epoch"
    };
    let _ = scoped_request(
        gloo_net::http::Request::post("/api/history/states"),
        world_id.as_deref(),
    )
    .json(&serde_json::json!({
        "time_key": time_key,
        "display_date": display_date,
        "name": name,
        "based_on": based_on,
        "direction": direction,
    }))
    .unwrap()
    .send()
    .await;
    load_history(state.clone()).await;
}

async fn prompt_add_event(state: &Rc<RefCell<AppState>>) {
    let (world_id, anchor, pivot_key) = {
        let s = state.borrow();
        let anchor = s.history_selected_id.clone();
        let pivot = s
            .history_states
            .iter()
            .find(|x| x.id == anchor)
            .map(|x| x.time_key)
            .unwrap_or(0);
        (s.scoped_world_id.clone(), anchor, pivot)
    };
    let time_key = pivot_key + 50;
    let display_date = format!("{:04}", time_key.abs());
    let _ = scoped_request(
        gloo_net::http::Request::post("/api/history/events"),
        world_id.as_deref(),
    )
    .json(&serde_json::json!({
        "time_key": time_key,
        "display_date": display_date,
        "name": "Historical event",
        "description": "",
        "anchor_state_id": anchor,
    }))
    .unwrap()
    .send()
    .await;
    load_history(state.clone()).await;
}

async fn prompt_cataclysm(state: &Rc<RefCell<AppState>>) {
    let (world_id, based_on, pivot_key) = {
        let s = state.borrow();
        let based_on = s.history_selected_id.clone();
        let pivot = s
            .history_states
            .iter()
            .find(|x| x.id == based_on)
            .map(|x| x.time_key)
            .unwrap_or(0);
        (s.scoped_world_id.clone(), based_on, pivot)
    };
    let time_key = pivot_key + 100;
    let display_date = format!("{:04}", time_key.abs());
    let _ = scoped_request(
        gloo_net::http::Request::post("/api/history/cataclysm"),
        world_id.as_deref(),
    )
    .json(&serde_json::json!({
        "time_key": time_key,
        "display_date": display_date,
        "event_name": "Cataclysm",
        "description": "",
        "result_state_name": "After cataclysm",
        "based_on": based_on,
        "changed_domains": ["land"],
        "notes": "",
    }))
    .unwrap()
    .send()
    .await;
    load_history(state.clone()).await;
}

async fn ack_divergence(state: &Rc<RefCell<AppState>>, domains: Option<&[&str]>) {
    let (world_id, id, body) = {
        let s = state.borrow();
        let domains: Vec<String> = domains
            .map(|list| list.iter().map(|d| (*d).to_string()).collect())
            .unwrap_or_default();
        let body = if domains.is_empty() {
            serde_json::json!({})
        } else {
            serde_json::json!({ "domains": domains })
        };
        (s.scoped_world_id.clone(), s.history_selected_id.clone(), body)
    };
    let url = format!("/api/history/states/{id}/divergence/ack");
    let _ = scoped_request(
        gloo_net::http::Request::post(&url),
        world_id.as_deref(),
    )
    .json(&body)
    .unwrap()
    .send()
    .await;
    load_history(state.clone()).await;
}

async fn rebase_domain(state: &Rc<RefCell<AppState>>, domain: &str) {
    let (world_id, id) = {
        let s = state.borrow();
        (s.scoped_world_id.clone(), s.history_selected_id.clone())
    };
    let url = format!("/api/history/states/{id}/rebase");
    let _ = scoped_request(
        gloo_net::http::Request::post(&url),
        world_id.as_deref(),
    )
    .json(&serde_json::json!({ "domain": domain }))
    .unwrap()
    .send()
    .await;
    load_history(state.clone()).await;
    load_map(state.clone()).await;
    load_elevation(state).await;
}

async fn delete_selected_state(state: &Rc<RefCell<AppState>>) {
    let (world_id, id) = {
        let s = state.borrow();
        (s.scoped_world_id.clone(), s.history_selected_id.clone())
    };
    let url = format!("/api/history/states/{id}");
    let _ = scoped_request(
        gloo_net::http::Request::delete(&url),
        world_id.as_deref(),
    )
    .send()
    .await;
    load_history(state.clone()).await;
    load_map(state.clone()).await;
    load_elevation(state).await;
}
