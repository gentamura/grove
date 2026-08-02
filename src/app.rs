use crate::{
    models::{
        ActivityKind, CodingAgent, CodingSession, CodingSubagent, ConversationMessage,
        ConversationRole, SessionActivity, SessionScan, SessionStatus,
    },
    preferences::{Group, MapNodeOffset, Preferences},
    scanner,
    text_input::{InputEvent, TextInput},
};
use chrono::{DateTime, Utc};
use gpui::{
    App, ClipboardItem, Context, CursorStyle, Div, Entity, FocusHandle, Focusable, FontWeight,
    Hsla, KeyDownEvent, KeyUpEvent, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent,
    PathBuilder, ScrollHandle, ScrollWheelEvent, SharedString, Timer, Window, canvas, div, point,
    prelude::*, px, rgb, rgba,
};
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};

const BG: u32 = 0x0b100d;
const PANEL: u32 = 0x101713;
const PANEL_SOFT: u32 = 0x141d18;
const PANEL_RAISED: u32 = 0x18221c;
const LINE: u32 = 0x253129;
const LINE_STRONG: u32 = 0x35443a;
const TEXT: u32 = 0xe7eee8;
const MUTED: u32 = 0x8d9b91;
const FAINT: u32 = 0x667269;
const GREEN: u32 = 0xa5d56f;
const AMBER: u32 = 0xe8b86a;
const BLUE: u32 = 0x7db8c9;
const DANGER: u32 = 0xdd7b72;
const MAP_ZOOM_MIN: f32 = 0.5;
const MAP_ZOOM_MAX: f32 = 1.6;
const MAP_ZOOM_STEP: f32 = 0.1;
const MAP_ZOOM_DEFAULT: f32 = 1.0;
const MAP_AGENT_COMPACT_THRESHOLD: usize = 12;
const MAP_AGENT_CLUSTER_WIDTH: f32 = 280.;
const MAP_AGENT_CLUSTER_HEIGHT: f32 = 126.;
const MAP_AGENT_TRAY_CARD_WIDTH: f32 = 218.;
const MAP_AGENT_TRAY_CARD_HEIGHT: f32 = 58.;
const MAP_AGENT_TRAY_GAP: f32 = 6.;
const MAP_AGENT_TRAY_PADDING: f32 = 10.;
const MAP_AGENT_TRAY_WIDTH_BUFFER: f32 = 12.;
const MAP_AGENT_TRAY_HEIGHT_BUFFER: f32 = 32.;
const MAP_CANVAS_MIN_RADIUS_X: f32 = 1_000.;
const MAP_CANVAS_MIN_RADIUS_Y: f32 = 640.;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StatusFilter {
    All,
    Active,
    Waiting,
    Idle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProviderFilter {
    All,
    ClaudeCode,
    Codex,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ViewMode {
    Detail,
    Map,
}

#[derive(Clone)]
enum MapNode {
    Grove,
    Session(CodingSession),
    AgentCluster {
        session_id: String,
        subagents: Vec<CodingSubagent>,
    },
    Subagent {
        session_id: String,
        subagent: CodingSubagent,
    },
}

#[derive(Clone)]
struct PositionedMapNode {
    id: String,
    node: MapNode,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
}

#[derive(Clone)]
struct MapEdge {
    from_id: String,
    to_id: String,
    from_x: f32,
    from_y: f32,
    to_x: f32,
    to_y: f32,
    color: Hsla,
}

struct MindMapLayout {
    width: f32,
    height: f32,
    nodes: Vec<PositionedMapNode>,
    edges: Vec<MapEdge>,
}

#[derive(Clone, Copy)]
struct MapPan {
    pointer_origin: (f32, f32),
    scroll_origin: (f32, f32),
}

#[derive(Clone)]
struct MapNodeDrag {
    node_id: String,
    pointer_origin: (f32, f32),
    offset_origin: MapNodeOffset,
    current_offset: MapNodeOffset,
    did_move: bool,
}

#[derive(Clone)]
struct SessionDrag {
    session_id: String,
    title: String,
}

impl gpui::Render for SessionDrag {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .max_w(px(260.))
            .px(px(12.))
            .py(px(8.))
            .rounded(px(7.))
            .border_1()
            .border_color(rgb(LINE_STRONG))
            .bg(rgba(0x18221ce8))
            .shadow_lg()
            .text_size(px(11.))
            .text_color(rgb(TEXT))
            .truncate()
            .child(self.title.clone())
    }
}

pub struct Grove {
    scan: SessionScan,
    selected_id: Option<String>,
    preferences: Preferences,
    filter: StatusFilter,
    provider_filter: ProviderFilter,
    view_mode: ViewMode,
    query: String,
    search_input: Entity<TextInput>,
    group_input: Entity<TextInput>,
    creating_group: bool,
    collapsed_groups: HashSet<String>,
    scan_error: Option<String>,
    preferences_error: Option<String>,
    copied_session: Option<String>,
    selected_agent_id: Option<String>,
    map_focus: FocusHandle,
    map_scroll: ScrollHandle,
    map_zoom: f32,
    map_pan: Option<MapPan>,
    map_node_drag: Option<MapNodeDrag>,
    map_suppress_click: Option<String>,
    map_space_held: bool,
    expanded_agent_clusters: HashSet<String>,
    map_inspector_open: bool,
    messages_open: bool,
    messages_session_id: Option<String>,
    messages_title: String,
    messages: Vec<ConversationMessage>,
    messages_loading: bool,
    messages_error: Option<String>,
    scanning: bool,
}

impl Grove {
    pub fn new(scan: SessionScan, cx: &mut Context<Self>) -> Self {
        let selected_id = scan.sessions.first().map(CodingSession::key);
        let search_input = cx.new(|cx| TextInput::new(cx, "Find sessions"));
        let group_input = cx.new(|cx| TextInput::new(cx, "Group name"));
        let map_focus = cx.focus_handle();

        cx.subscribe(&search_input, |this, input, event, cx| {
            if matches!(event, InputEvent::Changed) {
                this.query = input.read(cx).text();
                cx.notify();
            }
        })
        .detach();

        cx.subscribe(&group_input, |this, input, event, cx| match event {
            InputEvent::Submitted => {
                let name = input.read(cx).text();
                this.create_group(&name);
                input.update(cx, |input, cx| input.clear(cx));
                this.creating_group = false;
                cx.notify();
            }
            InputEvent::Cancelled => {
                input.update(cx, |input, cx| input.clear(cx));
                this.creating_group = false;
                cx.notify();
            }
            InputEvent::Changed => {}
        })
        .detach();

        let this = Self {
            scan,
            selected_id,
            preferences: Preferences::load(),
            filter: StatusFilter::All,
            provider_filter: ProviderFilter::All,
            view_mode: ViewMode::Detail,
            query: String::new(),
            search_input,
            group_input,
            creating_group: false,
            collapsed_groups: HashSet::new(),
            scan_error: None,
            preferences_error: None,
            copied_session: None,
            selected_agent_id: None,
            map_focus,
            map_scroll: ScrollHandle::new(),
            map_zoom: MAP_ZOOM_DEFAULT,
            map_pan: None,
            map_node_drag: None,
            map_suppress_click: None,
            map_space_held: false,
            expanded_agent_clusters: HashSet::new(),
            map_inspector_open: false,
            messages_open: false,
            messages_session_id: None,
            messages_title: String::new(),
            messages: Vec::new(),
            messages_loading: false,
            messages_error: None,
            scanning: false,
        };
        this.start_refresh_loop(cx);
        this
    }

    fn start_refresh_loop(&self, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            loop {
                Timer::after(Duration::from_secs(5)).await;
                let roots = session_roots();
                let executor = cx.background_executor().clone();
                let result = executor
                    .spawn(async move {
                        scanner::scan_sessions_at(&roots, SystemTime::now())
                            .map_err(|error| error.to_string())
                    })
                    .await;
                if this
                    .update(cx, |this, cx| {
                        this.apply_scan_result(result);
                        cx.notify();
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
    }

    fn refresh_now(&mut self, cx: &mut Context<Self>) {
        if self.scanning {
            return;
        }
        self.scanning = true;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let roots = session_roots();
            let executor = cx.background_executor().clone();
            let result = executor
                .spawn(async move {
                    scanner::scan_sessions_at(&roots, SystemTime::now())
                        .map_err(|error| error.to_string())
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.scanning = false;
                this.apply_scan_result(result);
                cx.notify();
            });
        })
        .detach();
    }

    fn open_messages(
        &mut self,
        session_id: String,
        provider: CodingAgent,
        title: String,
        cx: &mut Context<Self>,
    ) {
        let session_key = provider.session_key(&session_id);
        self.messages_open = true;
        self.messages_session_id = Some(session_key.clone());
        self.messages_title = title;
        self.messages.clear();
        self.messages_error = None;
        self.messages_loading = true;
        cx.notify();

        cx.spawn(async move |this, cx| {
            let roots = session_roots();
            let requested_session_id = session_id.clone();
            let executor = cx.background_executor().clone();
            let result = executor
                .spawn(async move {
                    scanner::load_session_messages_for(&roots, provider, &requested_session_id)
                        .map_err(|error| error.to_string())
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.messages_session_id.as_deref() != Some(session_key.as_str()) {
                    return;
                }
                this.messages_loading = false;
                match result {
                    Ok(messages) => {
                        this.messages = messages;
                        this.messages_error = None;
                    }
                    Err(error) => {
                        this.messages.clear();
                        this.messages_error = Some(error);
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn close_messages(&mut self, cx: &mut Context<Self>) {
        self.clear_messages();
        cx.notify();
    }

    fn clear_messages(&mut self) {
        self.messages_open = false;
        self.messages_session_id = None;
        self.messages.clear();
        self.messages_loading = false;
        self.messages_error = None;
    }

    fn dismiss_map_overlays(&mut self) {
        self.map_inspector_open = false;
        self.selected_agent_id = None;
        self.clear_messages();
    }

    fn dismiss_active_overlay_on_escape(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        if event.keystroke.key != "escape" {
            return;
        }

        if self.messages_open {
            self.clear_messages();
        } else if self.view_mode == ViewMode::Map && self.map_inspector_open {
            self.map_inspector_open = false;
            self.selected_agent_id = None;
        } else {
            return;
        }

        cx.stop_propagation();
        cx.notify();
    }

    fn apply_scan_result(&mut self, result: Result<SessionScan, String>) {
        match result {
            Ok(scan) => {
                if self
                    .selected_id
                    .as_ref()
                    .is_none_or(|id| !scan.sessions.iter().any(|session| session.key() == *id))
                {
                    self.selected_id = scan.sessions.first().map(CodingSession::key);
                }
                self.scan = scan;
                self.scan_error = None;
            }
            Err(error) => self.scan_error = Some(error),
        }
    }

    fn update_preferences(&mut self, update: impl FnOnce(&mut Preferences) -> bool) -> bool {
        match self.preferences.update_and_save(update) {
            Ok(updated) => {
                if updated {
                    self.preferences_error = None;
                }
                updated
            }
            Err(error) => {
                self.preferences_error = Some(format!("Could not save preferences: {error}"));
                false
            }
        }
    }

    fn create_group(&mut self, name: &str) -> bool {
        self.update_preferences(|preferences| preferences.create_group(name).is_some())
    }

    fn delete_group(&mut self, group_id: &str) {
        self.update_preferences(|preferences| {
            preferences.delete_group(group_id);
            true
        });
    }

    fn assign_session(&mut self, session_id: &str, group_id: Option<&str>) {
        self.update_preferences(|preferences| {
            preferences.assign(session_id, group_id);
            true
        });
    }

    fn warning_message(&self) -> Option<String> {
        let mut warnings = Vec::new();
        if let Some(error) = self.scan_error.as_ref() {
            warnings.push(format!("Scan failed: {error}"));
        } else if !self.scan.warnings.is_empty() {
            let detail = self.scan.warnings.first().cloned().unwrap_or_default();
            warnings.push(format!(
                "Partial scan: {} session path(s) skipped. {detail}",
                self.scan.skipped_files
            ));
        }
        if let Some(error) = self.preferences_error.as_ref() {
            warnings.push(error.clone());
        }
        (!warnings.is_empty()).then(|| warnings.join("  ·  "))
    }

    fn selected(&self) -> Option<CodingSession> {
        self.selected_id
            .as_ref()
            .and_then(|id| {
                self.scan
                    .sessions
                    .iter()
                    .find(|session| session.key() == *id)
            })
            .cloned()
            .or_else(|| self.filtered_sessions().first().cloned())
    }

    fn filtered_sessions(&self) -> Vec<CodingSession> {
        self.scan
            .sessions
            .iter()
            .filter(|session| session_matches_provider_filter(session, self.provider_filter))
            .filter(|session| session_matches_status_filter(session, self.filter))
            .cloned()
            .collect()
    }

    fn visible_sessions(&self) -> Vec<CodingSession> {
        let query = self.query.trim().to_lowercase();
        self.scan
            .sessions
            .iter()
            .filter(|session| session_matches_provider_filter(session, self.provider_filter))
            .filter(|session| session_matches_status_filter(session, self.filter))
            .filter(|session| {
                query.is_empty()
                    || [
                        session.title.as_str(),
                        session.project_name.as_str(),
                        session.cwd.as_str(),
                        session.git_branch.as_deref().unwrap_or(""),
                    ]
                    .join(" ")
                    .to_lowercase()
                    .contains(&query)
            })
            .cloned()
            .collect()
    }

    fn status_filtered_sessions(&self) -> Vec<CodingSession> {
        self.scan
            .sessions
            .iter()
            .filter(|session| session_matches_provider_filter(session, self.provider_filter))
            .filter(|session| session_matches_status_filter(session, self.filter))
            .cloned()
            .collect()
    }

    fn status_count(&self, status: SessionStatus) -> usize {
        self.scan
            .sessions
            .iter()
            .filter(|session| session_matches_provider_filter(session, self.provider_filter))
            .filter(|session| session.status == status)
            .count()
    }

    fn render_titlebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let source = if self.scan_error.is_some() {
            "Scanner unavailable"
        } else if self.preferences_error.is_some() {
            "Groups not saved"
        } else if !self.scan.warnings.is_empty() {
            "Partial scan"
        } else {
            "Watching local sessions"
        };
        let has_error = self.scan_error.is_some() || self.preferences_error.is_some();
        let has_warning = !self.scan.warnings.is_empty();
        let refreshing = self.scanning;
        div()
            .id("titlebar")
            .h(px(48.))
            .flex_none()
            .flex()
            .items_center()
            .border_b_1()
            .border_color(rgb(LINE))
            .bg(rgba(0x0d130ff5))
            .on_mouse_down(MouseButton::Left, |_, window, _| window.start_window_move())
            .child(div().w(px(80.)).flex_none())
            .child(
                div()
                    .w(px(190.))
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(px(9.))
                    .child(
                        div()
                            .size(px(26.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(7.))
                            .border_1()
                            .border_color(rgb(LINE_STRONG))
                            .bg(rgb(PANEL_RAISED))
                            .text_color(rgb(GREEN))
                            .text_size(px(15.))
                            .child("♧"),
                    )
                    .child(
                        div()
                            .text_size(px(12.))
                            .font_weight(FontWeight::BOLD)
                            .text_color(rgb(TEXT))
                            .child("G R O V E"),
                    )
                    .child(
                        div()
                            .text_size(px(8.))
                            .text_color(rgb(FAINT))
                            .child("LOCAL AGENTS"),
                    ),
            )
            .child(div().flex_1())
            .child(self.render_view_mode_button("Tree", ViewMode::Detail, cx))
            .child(self.render_view_mode_button("Map", ViewMode::Map, cx))
            .child(
                div()
                    .ml(px(9.))
                    .p(px(2.))
                    .flex()
                    .items_center()
                    .rounded(px(6.))
                    .border_1()
                    .border_color(rgb(LINE))
                    .bg(rgb(PANEL))
                    .child(self.render_provider_filter_button("All", ProviderFilter::All, cx))
                    .child(self.render_provider_filter_button(
                        "Claude",
                        ProviderFilter::ClaudeCode,
                        cx,
                    ))
                    .child(self.render_provider_filter_button("Codex", ProviderFilter::Codex, cx)),
            )
            .child(
                div()
                    .ml(px(12.))
                    .flex()
                    .items_center()
                    .gap(px(7.))
                    .text_size(px(10.))
                    .text_color(rgb(MUTED))
                    .child(div().size(px(6.)).rounded_full().bg(if has_error {
                        rgb(DANGER)
                    } else if has_warning {
                        rgb(AMBER)
                    } else {
                        rgb(GREEN)
                    }))
                    .child(source),
            )
            .child(
                div()
                    .id("refresh")
                    .mx(px(13.))
                    .px(px(11.))
                    .h(px(30.))
                    .flex()
                    .items_center()
                    .gap(px(7.))
                    .rounded(px(6.))
                    .border_1()
                    .border_color(rgb(LINE))
                    .bg(rgb(PANEL_SOFT))
                    .text_size(px(10.))
                    .text_color(rgb(MUTED))
                    .cursor_pointer()
                    .hover(|style| style.border_color(rgb(LINE_STRONG)).text_color(rgb(TEXT)))
                    .on_click(cx.listener(|this, _, _, cx| this.refresh_now(cx)))
                    .child(if refreshing {
                        "↻ Scanning"
                    } else {
                        "↻ Refresh"
                    }),
            )
    }

    fn current_map_layout(&self) -> MindMapLayout {
        let mut offsets = self.preferences.map_node_offsets.clone();
        if let Some(drag) = self.map_node_drag.as_ref() {
            offsets.insert(drag.node_id.clone(), drag.current_offset);
        }
        build_mind_map_layout_with_state(
            &self.status_filtered_sessions(),
            &offsets,
            &self.expanded_agent_clusters,
        )
    }

    fn center_map(&self, window: &Window) {
        let layout = self.current_map_layout();
        let viewport = window.viewport_size();
        let viewport_width = f32::from(viewport.width);
        let available_height = (f32::from(viewport.height) - 106.).max(320.);
        let (x, y) = centered_map_offset(
            layout.width * self.map_zoom,
            layout.height * self.map_zoom,
            viewport_width,
            available_height,
        );
        self.map_scroll.set_offset(point(px(x), px(y)));
    }

    fn focus_map_node(&self, node_id: &str, window: &Window) {
        let layout = self.current_map_layout();
        let Some(node) = layout.nodes.iter().find(|node| node.id == node_id) else {
            self.center_map(window);
            return;
        };
        let scroll_bounds = self.map_scroll.bounds();
        let window_viewport = window.viewport_size();
        let viewport_width = if scroll_bounds.size.width > px(1.) {
            f32::from(scroll_bounds.size.width)
        } else {
            f32::from(window_viewport.width)
        };
        let viewport_height = if scroll_bounds.size.height > px(1.) {
            f32::from(scroll_bounds.size.height)
        } else {
            (f32::from(window_viewport.height) - 106.).max(320.)
        };
        let requested = (
            viewport_width / 2. - (node.x + node.width / 2.) * self.map_zoom,
            viewport_height / 2. - (node.y + node.height / 2.) * self.map_zoom,
        );
        let offset = clamped_map_offset(
            requested,
            (layout.width * self.map_zoom, layout.height * self.map_zoom),
            (viewport_width, viewport_height),
        );
        self.map_scroll
            .set_offset(point(px(offset.0), px(offset.1)));
    }

    fn set_map_zoom(&mut self, requested_zoom: f32, window: &Window) {
        self.set_map_zoom_at(requested_zoom, window, None);
    }

    fn set_map_zoom_at(
        &mut self,
        requested_zoom: f32,
        window: &Window,
        anchor: Option<(f32, f32)>,
    ) {
        let new_zoom = clamp_map_zoom(requested_zoom);
        if (new_zoom - self.map_zoom).abs() < f32::EPSILON {
            return;
        }

        let old_zoom = self.map_zoom;
        let layout = self.current_map_layout();
        let scroll_bounds = self.map_scroll.bounds();
        let window_viewport = window.viewport_size();
        let viewport_width = if scroll_bounds.size.width > px(1.) {
            f32::from(scroll_bounds.size.width)
        } else {
            f32::from(window_viewport.width)
        };
        let viewport_height = if scroll_bounds.size.height > px(1.) {
            f32::from(scroll_bounds.size.height)
        } else {
            (f32::from(window_viewport.height) - 106.).max(320.)
        };
        let old_offset = self.map_scroll.offset();
        let anchor = anchor.unwrap_or((viewport_width / 2., viewport_height / 2.));
        let (x, y) = zoomed_map_offset_around(
            (f32::from(old_offset.x), f32::from(old_offset.y)),
            old_zoom,
            new_zoom,
            (layout.width, layout.height),
            (viewport_width, viewport_height),
            anchor,
        );

        self.map_zoom = new_zoom;
        self.map_scroll.set_offset(point(px(x), px(y)));
    }

    fn zoom_map_from_scroll(
        &mut self,
        event: &ScrollWheelEvent,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        let delta = event.delta.pixel_delta(window.line_height());
        let delta_y = f32::from(delta.y);
        if delta_y.abs() < f32::EPSILON {
            return;
        }
        let bounds = self.map_scroll.bounds();
        let anchor = (
            f32::from(event.position.x - bounds.origin.x),
            f32::from(event.position.y - bounds.origin.y),
        );
        self.set_map_zoom_at(self.map_zoom + delta_y * 0.0025, window, Some(anchor));
        cx.notify();
    }

    fn start_map_pan(&mut self, event: &MouseDownEvent, cx: &mut Context<Self>) {
        self.map_node_drag = None;
        let offset = self.map_scroll.offset();
        self.map_pan = Some(MapPan {
            pointer_origin: (f32::from(event.position.x), f32::from(event.position.y)),
            scroll_origin: (f32::from(offset.x), f32::from(offset.y)),
        });
        cx.notify();
    }

    fn update_map_pan(&mut self, event: &MouseMoveEvent, cx: &mut Context<Self>) {
        let Some(pan) = self.map_pan else {
            return;
        };
        if !event.dragging() {
            self.map_pan = None;
            cx.notify();
            return;
        }

        let pointer = (f32::from(event.position.x), f32::from(event.position.y));
        let requested = (
            pan.scroll_origin.0 + pointer.0 - pan.pointer_origin.0,
            pan.scroll_origin.1 + pointer.1 - pan.pointer_origin.1,
        );
        let layout = self.current_map_layout();
        let bounds = self.map_scroll.bounds();
        let viewport = (f32::from(bounds.size.width), f32::from(bounds.size.height));
        let offset = clamped_map_offset(
            requested,
            (layout.width * self.map_zoom, layout.height * self.map_zoom),
            viewport,
        );
        self.map_scroll
            .set_offset(point(px(offset.0), px(offset.1)));
        cx.notify();
    }

    fn end_map_pan(&mut self, _: &MouseUpEvent, cx: &mut Context<Self>) {
        let Some(pan) = self.map_pan.take() else {
            return;
        };
        let offset = self.map_scroll.offset();
        if (f32::from(offset.x) - pan.scroll_origin.0).abs() > 1.
            || (f32::from(offset.y) - pan.scroll_origin.1).abs() > 1.
        {
            self.map_suppress_click = Some("canvas".into());
        }
        cx.notify();
    }

    fn start_map_node_drag(
        &mut self,
        node_id: String,
        event: &MouseDownEvent,
        cx: &mut Context<Self>,
    ) {
        self.map_suppress_click = None;
        let offset_origin = self
            .preferences
            .map_node_offsets
            .get(&node_id)
            .copied()
            .unwrap_or(MapNodeOffset { x: 0, y: 0 });
        self.map_pan = None;
        self.map_node_drag = Some(MapNodeDrag {
            node_id,
            pointer_origin: (f32::from(event.position.x), f32::from(event.position.y)),
            offset_origin,
            current_offset: offset_origin,
            did_move: false,
        });
        cx.notify();
    }

    fn update_map_node_drag(&mut self, event: &MouseMoveEvent, cx: &mut Context<Self>) {
        let Some(drag) = self.map_node_drag.as_ref() else {
            return;
        };
        if !event.dragging() {
            return;
        }

        let pointer = (f32::from(event.position.x), f32::from(event.position.y));
        let requested = MapNodeOffset {
            x: drag.offset_origin.x
                + ((pointer.0 - drag.pointer_origin.0) / self.map_zoom).round() as i32,
            y: drag.offset_origin.y
                + ((pointer.1 - drag.pointer_origin.1) / self.map_zoom).round() as i32,
        };
        let node_id = drag.node_id.clone();
        let base_layout = build_mind_map_layout(&self.scan.sessions);
        let Some(base_node) = base_layout.nodes.iter().find(|node| node.id == node_id) else {
            return;
        };
        let current_offset = clamped_node_offset(base_node, &base_layout, requested);
        let moved_node_id = self.map_node_drag.as_mut().and_then(|drag| {
            if drag.current_offset == current_offset {
                return None;
            }
            drag.current_offset = current_offset;
            drag.did_move = true;
            Some(drag.node_id.clone())
        });
        if let Some(node_id) = moved_node_id {
            self.map_suppress_click = Some(node_id);
            cx.notify();
        }
    }

    fn end_map_node_drag(&mut self, _: &MouseUpEvent, cx: &mut Context<Self>) {
        let Some(drag) = self.map_node_drag.take() else {
            return;
        };
        if drag.did_move {
            self.map_suppress_click = Some(drag.node_id.clone());
        }
        if drag.current_offset != drag.offset_origin {
            let node_id = drag.node_id;
            let offset = drag.current_offset;
            self.update_preferences(move |preferences| {
                if offset == (MapNodeOffset { x: 0, y: 0 }) {
                    preferences.map_node_offsets.remove(&node_id);
                } else {
                    preferences.map_node_offsets.insert(node_id, offset);
                }
                true
            });
        }
        cx.notify();
    }

    fn consume_suppressed_map_click(&mut self, node_id: &str) -> bool {
        if self.map_suppress_click.as_deref() == Some(node_id) {
            self.map_suppress_click = None;
            true
        } else {
            false
        }
    }

    fn map_key_down(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        if event.keystroke.key == "space" && !self.map_space_held {
            self.map_space_held = true;
            cx.stop_propagation();
            cx.notify();
        }
    }

    fn map_key_up(&mut self, event: &KeyUpEvent, cx: &mut Context<Self>) {
        if event.keystroke.key == "space" {
            self.map_space_held = false;
            cx.stop_propagation();
            cx.notify();
        }
    }

    fn render_map_zoom_button(
        &self,
        id: &'static str,
        label: &'static str,
        delta: f32,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let enabled = if delta.is_sign_negative() {
            self.map_zoom > MAP_ZOOM_MIN
        } else {
            self.map_zoom < MAP_ZOOM_MAX
        };
        div()
            .id(id)
            .w(px(25.))
            .h(px(25.))
            .flex()
            .items_center()
            .justify_center()
            .text_size(px(13.))
            .text_color(if enabled { rgb(MUTED) } else { rgb(0x445047) })
            .when(enabled, |button| {
                button
                    .cursor_pointer()
                    .hover(|style| style.bg(rgb(PANEL_RAISED)).text_color(rgb(TEXT)))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.set_map_zoom(this.map_zoom + delta, window);
                        cx.notify();
                    }))
            })
            .child(label)
    }

    fn render_map_status_filter(
        &self,
        label: &'static str,
        filter: StatusFilter,
        color: gpui::Rgba,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let selected = self.filter == filter;
        div()
            .id(SharedString::from(format!("map-filter-{label}")))
            .ml(px(5.))
            .h(px(25.))
            .px(px(7.))
            .flex()
            .items_center()
            .gap(px(5.))
            .rounded(px(5.))
            .border_1()
            .border_color(if selected { color } else { rgb(LINE) })
            .bg(if selected {
                rgb(PANEL_RAISED)
            } else {
                rgba(0x00000000)
            })
            .text_size(px(8.))
            .font_weight(if selected {
                FontWeight::SEMIBOLD
            } else {
                FontWeight::NORMAL
            })
            .text_color(if selected { rgb(TEXT) } else { rgb(MUTED) })
            .cursor_pointer()
            .hover(move |style| style.border_color(color).text_color(rgb(TEXT)))
            .on_click(cx.listener(move |this, _, window, cx| {
                this.filter = filter;
                let visible_sessions = this.status_filtered_sessions();
                if this.selected_id.as_ref().is_none_or(|selected_id| {
                    !visible_sessions
                        .iter()
                        .any(|session| session.key() == *selected_id)
                }) {
                    this.selected_id = visible_sessions.first().map(CodingSession::key);
                    this.selected_agent_id = None;
                    this.map_inspector_open = false;
                }
                this.center_map(window);
                cx.notify();
            }))
            .child(div().size(px(6.)).rounded_full().bg(color))
            .child(label)
    }

    fn render_view_mode_button(
        &self,
        label: &'static str,
        mode: ViewMode,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let selected = self.view_mode == mode;
        div()
            .id(SharedString::from(format!("view-{label}")))
            .h(px(27.))
            .px(px(9.))
            .flex()
            .items_center()
            .rounded(px(5.))
            .bg(if selected {
                rgb(PANEL_RAISED)
            } else {
                rgba(0x00000000)
            })
            .text_size(px(9.))
            .font_weight(if selected {
                FontWeight::SEMIBOLD
            } else {
                FontWeight::NORMAL
            })
            .text_color(if selected { rgb(GREEN) } else { rgb(FAINT) })
            .cursor_pointer()
            .hover(|style| style.text_color(rgb(TEXT)))
            .on_click(cx.listener(move |this, _, window, cx| {
                this.view_mode = mode;
                if mode == ViewMode::Map {
                    this.map_focus.focus(window);
                    this.center_map(window);
                }
                cx.notify();
            }))
            .child(label)
    }

    fn render_provider_filter_button(
        &self,
        label: &'static str,
        filter: ProviderFilter,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let selected = self.provider_filter == filter;
        div()
            .id(SharedString::from(format!("provider-{label}")))
            .h(px(23.))
            .px(px(7.))
            .flex()
            .items_center()
            .rounded(px(4.))
            .bg(if selected {
                rgb(PANEL_RAISED)
            } else {
                rgba(0x00000000)
            })
            .text_size(px(8.))
            .font_weight(if selected {
                FontWeight::SEMIBOLD
            } else {
                FontWeight::NORMAL
            })
            .text_color(if selected { rgb(GREEN) } else { rgb(FAINT) })
            .cursor_pointer()
            .hover(|style| style.text_color(rgb(TEXT)))
            .on_click(cx.listener(move |this, _, window, cx| {
                this.provider_filter = filter;
                let visible_sessions = this.status_filtered_sessions();
                if this.selected_id.as_ref().is_none_or(|selected_id| {
                    !visible_sessions
                        .iter()
                        .any(|session| session.key() == *selected_id)
                }) {
                    this.selected_id = visible_sessions.first().map(CodingSession::key);
                    this.selected_agent_id = None;
                    this.dismiss_map_overlays();
                }
                if this.view_mode == ViewMode::Map {
                    this.center_map(window);
                }
                cx.notify();
            }))
            .child(label)
    }

    fn render_filter(
        &self,
        label: &'static str,
        value: StatusFilter,
        count: usize,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let selected = self.filter == value;
        div()
            .id(SharedString::from(format!("filter-{label}")))
            .h(px(25.))
            .px(px(8.))
            .flex()
            .items_center()
            .gap(px(5.))
            .rounded(px(5.))
            .bg(if selected {
                rgb(PANEL_RAISED)
            } else {
                rgba(0x00000000)
            })
            .text_size(px(10.))
            .text_color(if selected { rgb(TEXT) } else { rgb(FAINT) })
            .cursor_pointer()
            .on_click(cx.listener(move |this, _, _, cx| {
                this.filter = value;
                cx.notify();
            }))
            .child(label)
            .child(div().text_color(rgb(FAINT)).child(count.to_string()))
    }

    fn render_sidebar(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> Div {
        let visible = self.visible_sessions();
        let mut by_group: HashMap<String, Vec<CodingSession>> = HashMap::new();
        let mut ungrouped = Vec::new();
        for session in visible {
            if let Some(group_id) = self.preferences.assignments.get(&session.key())
                && self
                    .preferences
                    .groups
                    .iter()
                    .any(|group| &group.id == group_id)
            {
                by_group.entry(group_id.clone()).or_default().push(session);
            } else {
                ungrouped.push(session);
            }
        }

        let groups = self.preferences.groups.clone();
        let selected_id = self.selected_id.clone();
        let session_count = self
            .scan
            .sessions
            .iter()
            .filter(|session| session_matches_provider_filter(session, self.provider_filter))
            .count();
        let active = self.status_count(SessionStatus::Active);
        let waiting = self.status_count(SessionStatus::Waiting);

        div()
            .w(px(292.))
            .flex_none()
            .h_full()
            .flex()
            .flex_col()
            .border_r_1()
            .border_color(rgb(LINE))
            .bg(rgb(PANEL))
            .child(
                div()
                    .p(px(14.))
                    .pb(px(12.))
                    .flex_none()
                    .border_b_1()
                    .border_color(rgba(0x253129a0))
                    .child(self.search_input.clone())
                    .child(
                        div()
                            .mt(px(9.))
                            .flex()
                            .items_center()
                            .gap(px(3.))
                            .child(self.render_filter("All", StatusFilter::All, session_count, cx))
                            .child(self.render_filter("Working", StatusFilter::Active, active, cx))
                            .child(self.render_filter(
                                "Waiting",
                                StatusFilter::Waiting,
                                waiting,
                                cx,
                            ))
                            .child(self.render_filter(
                                "Idle",
                                StatusFilter::Idle,
                                self.status_count(SessionStatus::Idle),
                                cx,
                            )),
                    ),
            )
            .child(
                div()
                    .id("session-tree")
                    .flex_1()
                    .overflow_y_scroll()
                    .px(px(13.))
                    .py(px(15.))
                    .child(
                        div()
                            .h(px(38.))
                            .flex()
                            .items_center()
                            .gap(px(9.))
                            .child(
                                div()
                                    .size(px(27.))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .rounded_full()
                                    .border_1()
                                    .border_color(rgb(LINE_STRONG))
                                    .bg(rgb(PANEL_RAISED))
                                    .text_color(rgb(GREEN))
                                    .child("♧"),
                            )
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap(px(1.))
                                    .child(
                                        div()
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_size(px(12.))
                                            .child("My Grove"),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(9.))
                                            .text_color(rgb(FAINT))
                                            .child(format!("{session_count} sessions")),
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .ml(px(14.))
                            .pl(px(12.))
                            .border_l_1()
                            .border_color(rgb(LINE_STRONG))
                            .children(groups.into_iter().map(|group| {
                                let sessions = by_group.remove(&group.id).unwrap_or_default();
                                self.render_group(group, sessions, selected_id.clone(), cx)
                                    .into_any_element()
                            }))
                            .child(
                                self.render_group(
                                    Group {
                                        id: "__ungrouped__".into(),
                                        name: "Ungrouped".into(),
                                    },
                                    ungrouped,
                                    selected_id,
                                    cx,
                                )
                                .into_any_element(),
                            ),
                    ),
            )
            .child(
                div()
                    .flex_none()
                    .p(px(13.))
                    .border_t_1()
                    .border_color(rgb(LINE))
                    .when(self.creating_group, |parent| {
                        parent.child(
                            div()
                                .flex()
                                .gap(px(6.))
                                .child(div().flex_1().child(self.group_input.clone()))
                                .child(
                                    div()
                                        .id("confirm-group")
                                        .w(px(31.))
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .rounded(px(6.))
                                        .bg(rgb(GREEN))
                                        .text_color(rgb(BG))
                                        .cursor_pointer()
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            let name = this.group_input.read(cx).text();
                                            this.create_group(&name);
                                            this.group_input
                                                .update(cx, |input, cx| input.clear(cx));
                                            this.creating_group = false;
                                            cx.notify();
                                        }))
                                        .child("✓"),
                                ),
                        )
                    })
                    .when(!self.creating_group, |parent| {
                        parent.child(
                            div()
                                .id("new-group")
                                .h(px(28.))
                                .flex()
                                .items_center()
                                .gap(px(7.))
                                .text_size(px(10.))
                                .text_color(rgb(MUTED))
                                .cursor_pointer()
                                .hover(|style| style.text_color(rgb(GREEN)))
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.creating_group = true;
                                    let focus = this.group_input.read(cx).focus_handle(cx).clone();
                                    window.focus(&focus);
                                    cx.notify();
                                }))
                                .child("+")
                                .child("New group"),
                        )
                    })
                    .child(
                        div()
                            .mt(px(3.))
                            .text_size(px(8.))
                            .text_color(rgb(0x4c574f))
                            .child("Drag leaves onto a branch to organize"),
                    ),
            )
    }

    fn render_group(
        &self,
        group: Group,
        sessions: Vec<CodingSession>,
        selected_id: Option<String>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let is_ungrouped = group.id == "__ungrouped__";
        let collapsed = self.collapsed_groups.contains(&group.id);
        let group_id = group.id.clone();
        let toggle_id = group.id.clone();
        let delete_id = group.id.clone();
        let count = sessions.len();

        div()
            .id(SharedString::from(format!("group-{}", group.id)))
            .py(px(3.))
            .can_drop(|value, _, _| value.downcast_ref::<SessionDrag>().is_some())
            .on_drop(cx.listener(move |this, drag: &SessionDrag, _, cx| {
                let target = (!is_ungrouped).then_some(group_id.as_str());
                this.assign_session(&drag.session_id, target);
                cx.notify();
            }))
            .child(
                div()
                    .h(px(34.))
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .rounded(px(6.))
                    .hover(|style| style.bg(rgb(PANEL_SOFT)))
                    .child(
                        div()
                            .id(SharedString::from(format!("toggle-{toggle_id}")))
                            .w(px(18.))
                            .h_full()
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_size(px(10.))
                            .text_color(rgb(FAINT))
                            .cursor_pointer()
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if !this.collapsed_groups.remove(&toggle_id) {
                                    this.collapsed_groups.insert(toggle_id.clone());
                                }
                                cx.notify();
                            }))
                            .child(if collapsed { "›" } else { "⌄" }),
                    )
                    .child(
                        div()
                            .text_size(px(11.))
                            .text_color(rgb(MUTED))
                            .child(if is_ungrouped { "▱" } else { "⑂" }),
                    )
                    .child(
                        div()
                            .flex_1()
                            .truncate()
                            .text_size(px(11.))
                            .text_color(rgb(TEXT))
                            .child(group.name.clone()),
                    )
                    .child(
                        div()
                            .min_w(px(20.))
                            .h(px(18.))
                            .px(px(5.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded_full()
                            .bg(rgb(PANEL_RAISED))
                            .text_size(px(9.))
                            .text_color(rgb(FAINT))
                            .child(count.to_string()),
                    )
                    .when(!is_ungrouped, |row| {
                        row.child(
                            div()
                                .id(SharedString::from(format!("delete-{delete_id}")))
                                .w(px(22.))
                                .h_full()
                                .flex()
                                .items_center()
                                .justify_center()
                                .text_size(px(10.))
                                .text_color(rgb(FAINT))
                                .cursor_pointer()
                                .hover(|style| style.text_color(rgb(DANGER)))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.delete_group(&delete_id);
                                    cx.notify();
                                }))
                                .child("×"),
                        )
                    }),
            )
            .when(!collapsed, |branch| {
                branch.child(
                    div()
                        .ml(px(8.))
                        .pl(px(13.))
                        .border_l_1()
                        .border_color(rgb(LINE))
                        .when(sessions.is_empty(), |list| {
                            list.child(
                                div()
                                    .h(px(31.))
                                    .flex()
                                    .items_center()
                                    .text_size(px(9.))
                                    .text_color(rgb(0x4f5b53))
                                    .child("Drop a session here"),
                            )
                        })
                        .children(sessions.into_iter().map(|session| {
                            self.render_session_leaf(session, selected_id.as_deref(), cx)
                                .into_any_element()
                        })),
                )
            })
    }

    fn render_session_leaf(
        &self,
        session: CodingSession,
        selected_id: Option<&str>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let session_key = session.key();
        let selected = selected_id == Some(session_key.as_str());
        let selected_session_key = session_key.clone();
        let drag = SessionDrag {
            session_id: session_key.clone(),
            title: session.title.clone(),
        };
        let status_color = status_color(session.status);
        let title = session.title.clone();
        let detail = if session.subagents.is_empty() {
            format!(
                "{} · {}",
                session.project_name,
                relative_time(&session.updated_at)
            )
        } else {
            format!(
                "{} · {} · {} agents",
                session.project_name,
                relative_time(&session.updated_at),
                session.subagents.len()
            )
        };

        div()
            .id(SharedString::from(format!("session-{session_key}")))
            .min_h(px(46.))
            .w_full()
            .px(px(8.))
            .py(px(5.))
            .flex()
            .items_center()
            .gap(px(7.))
            .rounded(px(6.))
            .bg(if selected {
                rgb(0x1d2921)
            } else {
                rgba(0x00000000)
            })
            .cursor_pointer()
            .hover(|style| style.bg(rgb(0x17201a)))
            .on_click(cx.listener(move |this, _, _, cx| {
                this.selected_id = Some(selected_session_key.clone());
                this.copied_session = None;
                cx.notify();
            }))
            .on_drag(drag, |drag, _, _, cx| {
                cx.new(|_| SessionDrag {
                    session_id: drag.session_id.clone(),
                    title: drag.title.clone(),
                })
            })
            .child(
                div()
                    .size(px(7.))
                    .flex_none()
                    .rounded_full()
                    .bg(status_color),
            )
            .child(
                div()
                    .min_w(px(0.))
                    .flex_1()
                    .flex()
                    .flex_col()
                    .gap(px(3.))
                    .child(
                        div()
                            .truncate()
                            .text_size(px(11.))
                            .font_weight(if selected {
                                FontWeight::SEMIBOLD
                            } else {
                                FontWeight::NORMAL
                            })
                            .text_color(if selected { rgb(TEXT) } else { rgb(MUTED) })
                            .child(title),
                    )
                    .child(
                        div()
                            .truncate()
                            .text_size(px(9.))
                            .text_color(rgb(FAINT))
                            .child(detail),
                    ),
            )
    }

    fn render_mind_map(&self, cx: &mut Context<Self>) -> Div {
        let layout = scaled_mind_map_layout(self.current_map_layout(), self.map_zoom);
        let edges = layout.edges.clone();
        let map_zoom = self.map_zoom;
        let filtered_sessions = self.status_filtered_sessions();
        let session_count = filtered_sessions.len();
        let subagent_count: usize = filtered_sessions
            .iter()
            .map(|session| session.subagents.len())
            .sum();

        div()
            .relative()
            .min_w(px(0.))
            .flex_1()
            .h_full()
            .flex()
            .flex_col()
            .bg(rgb(BG))
            .child(
                div()
                    .h(px(58.))
                    .flex_none()
                    .px(px(22.))
                    .flex()
                    .items_center()
                    .border_b_1()
                    .border_color(rgb(LINE))
                    .bg(rgba(0x101713ee))
                    .child(
                        div().child(eyebrow("GRAPH VIEW")).child(
                            div()
                                .mt(px(3.))
                                .text_size(px(15.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(rgb(TEXT))
                                .child("Agent mind map"),
                        ),
                    )
                    .child(
                        div()
                            .ml(px(14.))
                            .px(px(8.))
                            .h(px(22.))
                            .flex()
                            .items_center()
                            .rounded_full()
                            .bg(rgb(PANEL_RAISED))
                            .text_size(px(9.))
                            .text_color(rgb(MUTED))
                            .child(format!(
                                "{session_count} sessions  ·  {subagent_count} subagents"
                            )),
                    )
                    .child(
                        div()
                            .ml(px(8.))
                            .px(px(8.))
                            .h(px(22.))
                            .flex()
                            .items_center()
                            .rounded_full()
                            .border_1()
                            .border_color(rgb(LINE))
                            .text_size(px(8.))
                            .text_color(rgb(FAINT))
                            .child("↳ parent → spawned child"),
                    )
                    .child(div().flex_1())
                    .child(self.render_map_status_filter("All", StatusFilter::All, rgb(MUTED), cx))
                    .child(self.render_map_status_filter(
                        "Working",
                        StatusFilter::Active,
                        rgb(GREEN),
                        cx,
                    ))
                    .child(self.render_map_status_filter(
                        "Waiting",
                        StatusFilter::Waiting,
                        rgb(AMBER),
                        cx,
                    ))
                    .child(self.render_map_status_filter(
                        "Idle",
                        StatusFilter::Idle,
                        rgb(FAINT),
                        cx,
                    ))
                    .child(
                        div()
                            .ml(px(15.))
                            .w(px(97.))
                            .h(px(27.))
                            .flex_none()
                            .flex()
                            .items_center()
                            .rounded(px(5.))
                            .border_1()
                            .border_color(rgb(LINE))
                            .overflow_hidden()
                            .child(self.render_map_zoom_button(
                                "map-zoom-out",
                                "−",
                                -MAP_ZOOM_STEP,
                                cx,
                            ))
                            .child(
                                div()
                                    .id("map-zoom-reset")
                                    .w(px(45.))
                                    .h_full()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .border_l_1()
                                    .border_r_1()
                                    .border_color(rgb(LINE))
                                    .text_size(px(8.))
                                    .text_color(rgb(MUTED))
                                    .cursor_pointer()
                                    .hover(|style| {
                                        style.bg(rgb(PANEL_RAISED)).text_color(rgb(TEXT))
                                    })
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.set_map_zoom(MAP_ZOOM_DEFAULT, window);
                                        cx.notify();
                                    }))
                                    .child(format!("{:.0}%", self.map_zoom * 100.)),
                            )
                            .child(self.render_map_zoom_button(
                                "map-zoom-in",
                                "+",
                                MAP_ZOOM_STEP,
                                cx,
                            )),
                    )
                    .child(
                        div()
                            .id("center-map")
                            .ml(px(8.))
                            .px(px(8.))
                            .h(px(25.))
                            .flex()
                            .items_center()
                            .rounded(px(5.))
                            .border_1()
                            .border_color(rgb(LINE))
                            .text_size(px(8.))
                            .text_color(rgb(MUTED))
                            .cursor_pointer()
                            .hover(|style| style.border_color(rgb(GREEN)).text_color(rgb(TEXT)))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.center_map(window);
                                cx.notify();
                            }))
                            .child("◎ Center"),
                    )
                    .child(
                        div()
                            .ml(px(10.))
                            .text_size(px(8.))
                            .text_color(rgb(0x566259))
                            .child("Space + drag canvas · Drag node · ⌘Scroll zoom"),
                    ),
            )
            .child(
                div()
                    .id("mind-map-scroll")
                    .flex_1()
                    .min_h(px(0.))
                    .overflow_scroll()
                    .track_focus(&self.map_focus)
                    .track_scroll(&self.map_scroll)
                    .cursor(if self.map_pan.is_some() {
                        CursorStyle::ClosedHand
                    } else if self.map_space_held {
                        CursorStyle::OpenHand
                    } else {
                        CursorStyle::Arrow
                    })
                    .on_key_down(
                        cx.listener(|this, event: &KeyDownEvent, _, cx| {
                            this.map_key_down(event, cx)
                        }),
                    )
                    .on_key_up(
                        cx.listener(|this, event: &KeyUpEvent, _, cx| this.map_key_up(event, cx)),
                    )
                    .on_scroll_wheel(cx.listener(|this, event: &ScrollWheelEvent, window, cx| {
                        if event.modifiers.platform {
                            this.zoom_map_from_scroll(event, window, cx);
                            window.prevent_default();
                            cx.stop_propagation();
                        }
                    }))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, event: &MouseDownEvent, window, cx| {
                            this.map_focus.focus(window);
                            if this.map_space_held {
                                this.start_map_pan(event, cx);
                                cx.stop_propagation();
                            }
                        }),
                    )
                    .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _, cx| {
                        this.update_map_pan(event, cx);
                        this.update_map_node_drag(event, cx);
                    }))
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|this, event: &MouseUpEvent, _, cx| {
                            this.end_map_pan(event, cx);
                            this.end_map_node_drag(event, cx);
                        }),
                    )
                    .on_mouse_up_out(
                        MouseButton::Left,
                        cx.listener(|this, event: &MouseUpEvent, _, cx| {
                            this.end_map_pan(event, cx);
                            this.end_map_node_drag(event, cx);
                        }),
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        if this.consume_suppressed_map_click("canvas") {
                            cx.notify();
                            return;
                        }
                        this.dismiss_map_overlays();
                        cx.notify();
                    }))
                    .child(
                        div()
                            .relative()
                            .w(px(layout.width))
                            .h(px(layout.height))
                            .bg(rgb(0x0c120e))
                            .child(
                                canvas(
                                    |_, _, _| {},
                                    move |bounds, _, window, _| {
                                        for edge in &edges {
                                            let from = point(
                                                bounds.left() + px(edge.from_x),
                                                bounds.top() + px(edge.from_y),
                                            );
                                            let to = point(
                                                bounds.left() + px(edge.to_x),
                                                bounds.top() + px(edge.to_y),
                                            );
                                            let control_distance =
                                                ((edge.to_x - edge.from_x).abs() * 0.46)
                                                    .max(40. * map_zoom);
                                            let direction =
                                                if edge.to_x >= edge.from_x { 1. } else { -1. };
                                            let control_a = point(
                                                from.x + px(control_distance * direction),
                                                from.y,
                                            );
                                            let control_b = point(
                                                to.x - px(control_distance * direction),
                                                to.y,
                                            );
                                            let mut builder =
                                                PathBuilder::stroke(px(1.4 * map_zoom));
                                            builder.move_to(from);
                                            builder.cubic_bezier_to(to, control_a, control_b);
                                            if let Ok(path) = builder.build() {
                                                window.paint_path(path, edge.color);
                                            }
                                        }
                                    },
                                )
                                .absolute()
                                .left(px(0.))
                                .top(px(0.))
                                .size_full(),
                            )
                            .children(layout.nodes.into_iter().map(|node| {
                                self.render_map_node(node, map_zoom, cx).into_any_element()
                            })),
                    ),
            )
            .when(self.map_inspector_open, |map| {
                map.child(self.render_map_inspector(cx))
            })
    }

    fn render_map_node(
        &self,
        positioned: PositionedMapNode,
        zoom: f32,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let node_id = positioned.id.clone();
        let node_id_for_drag = node_id.clone();
        let is_dragging = self
            .map_node_drag
            .as_ref()
            .is_some_and(|drag| drag.node_id == node_id);
        let node_cursor = if is_dragging || self.map_pan.is_some() {
            CursorStyle::ClosedHand
        } else {
            CursorStyle::OpenHand
        };
        let base = div()
            .absolute()
            .left(px(positioned.x))
            .top(px(positioned.y))
            .w(px(positioned.width))
            .h(px(positioned.height))
            .cursor(node_cursor)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                    this.map_focus.focus(window);
                    if this.map_space_held {
                        this.start_map_pan(event, cx);
                    } else {
                        this.start_map_node_drag(node_id_for_drag.clone(), event, cx);
                    }
                    cx.stop_propagation();
                }),
            );

        match positioned.node {
            MapNode::Grove => base
                .id("map-grove")
                .p(map_px(13., zoom))
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .rounded(map_px(16., zoom))
                .border_1()
                .border_color(rgb(GREEN))
                .bg(rgba(0x1b2a20f5))
                .shadow_lg()
                .child(
                    div()
                        .size(map_px(27., zoom))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded_full()
                        .bg(rgba(0xa5d56f22))
                        .text_size(map_px(16., zoom))
                        .text_color(rgb(GREEN))
                        .child("♧"),
                )
                .child(
                    div()
                        .mt(map_px(7., zoom))
                        .text_size(map_px(12., zoom))
                        .font_weight(FontWeight::BOLD)
                        .text_color(rgb(TEXT))
                        .child("G R O V E"),
                )
                .child(
                    div()
                        .mt(map_px(2., zoom))
                        .text_size(map_px(8., zoom))
                        .text_color(rgb(FAINT))
                        .child("LOCAL AGENT MAP"),
                )
                .into_any_element(),
            MapNode::Session(session) => {
                let session_key = session.key();
                let selected = self.selected_id.as_deref() == Some(session_key.as_str())
                    && self.selected_agent_id.is_none();
                let selected_session_key = session_key.clone();
                let map_node_id = node_id.clone();
                let color = status_color(session.status);
                let subagent_count = session.subagents.len();
                base.id(SharedString::from(format!("map-session-{session_key}")))
                    .p(map_px(12., zoom))
                    .rounded(map_px(10., zoom))
                    .border_1()
                    .border_color(if selected {
                        color
                    } else {
                        rgb(LINE_STRONG).into()
                    })
                    .bg(rgba(0x141d18f5))
                    .shadow_md()
                    .hover(move |style| style.border_color(color).bg(rgb(PANEL_RAISED)))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        cx.stop_propagation();
                        if this.consume_suppressed_map_click(&map_node_id) {
                            cx.notify();
                            return;
                        }
                        this.clear_messages();
                        this.selected_id = Some(selected_session_key.clone());
                        this.selected_agent_id = None;
                        this.map_inspector_open = true;
                        cx.notify();
                    }))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(map_px(6., zoom))
                            .child(div().size(map_px(7., zoom)).rounded_full().bg(color))
                            .child(
                                div()
                                    .text_size(map_px(8., zoom))
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(color)
                                    .child(session.provider.display_name().to_uppercase()),
                            )
                            .child(div().flex_1())
                            .child(
                                div()
                                    .text_size(map_px(8., zoom))
                                    .text_color(rgb(FAINT))
                                    .child(format!("{subagent_count} agents")),
                            ),
                    )
                    .child(
                        div()
                            .mt(map_px(7., zoom))
                            .line_clamp(2)
                            .text_size(map_px(11., zoom))
                            .font_weight(FontWeight::SEMIBOLD)
                            .line_height(map_px(15., zoom))
                            .text_color(rgb(TEXT))
                            .child(session.title),
                    )
                    .child(
                        div()
                            .mt(map_px(7., zoom))
                            .flex()
                            .items_center()
                            .gap(map_px(5., zoom))
                            .text_size(map_px(8., zoom))
                            .text_color(rgb(FAINT))
                            .child(session.project_name)
                            .child("·")
                            .child(relative_time(&session.updated_at)),
                    )
                    .into_any_element()
            }
            MapNode::AgentCluster {
                session_id,
                subagents,
            } => {
                let expanded = self.expanded_agent_clusters.contains(&session_id);
                let session_id_for_toggle = session_id.clone();
                let session_id_for_cluster = session_id.clone();
                let map_node_id = node_id.clone();
                let type_summary = subagent_type_summary(&subagents);
                let status_summary = subagent_status_summary(&subagents);
                let columns = agent_tray_column_count(subagents.len());
                let groups = grouped_subagents(&subagents);
                base.id(SharedString::from(format!(
                    "map-agent-cluster-{session_id}"
                )))
                .flex()
                .flex_col()
                .rounded(map_px(11., zoom))
                .border_1()
                .border_color(if expanded {
                    rgb(BLUE)
                } else {
                    rgb(LINE_STRONG)
                })
                .bg(rgba(0x101b17f7))
                .shadow_md()
                .overflow_hidden()
                .hover(|style| style.border_color(rgb(BLUE)).bg(rgb(PANEL_RAISED)))
                .on_click(cx.listener(move |this, _, _, cx| {
                    cx.stop_propagation();
                    if this.consume_suppressed_map_click(&map_node_id) {
                        cx.notify();
                        return;
                    }
                    this.clear_messages();
                    this.selected_id = Some(session_id_for_cluster.clone());
                    this.selected_agent_id = None;
                    this.map_inspector_open = true;
                    cx.notify();
                }))
                .child(
                    div()
                        .h(map_px(MAP_AGENT_CLUSTER_HEIGHT, zoom))
                        .flex_none()
                        .p(map_px(12., zoom))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(map_px(7., zoom))
                                .child(
                                    div()
                                        .size(map_px(24., zoom))
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .rounded(map_px(7., zoom))
                                        .bg(rgba(0x7db8c922))
                                        .text_size(map_px(12., zoom))
                                        .text_color(rgb(BLUE))
                                        .child("⌘"),
                                )
                                .child(
                                    div()
                                        .text_size(map_px(9., zoom))
                                        .font_weight(FontWeight::BOLD)
                                        .text_color(rgb(BLUE))
                                        .child(format!("{} SUBAGENTS", subagents.len())),
                                )
                                .child(div().flex_1())
                                .child(
                                    div()
                                        .text_size(map_px(8., zoom))
                                        .text_color(rgb(FAINT))
                                        .child(if expanded { "EXPANDED" } else { "COMPACT" }),
                                ),
                        )
                        .child(
                            div()
                                .mt(map_px(9., zoom))
                                .truncate()
                                .text_size(map_px(9., zoom))
                                .text_color(rgb(TEXT))
                                .child(type_summary),
                        )
                        .child(
                            div()
                                .mt(map_px(5., zoom))
                                .text_size(map_px(8., zoom))
                                .text_color(rgb(MUTED))
                                .child(status_summary),
                        )
                        .child(
                            div()
                                .id(SharedString::from(format!(
                                    "map-agent-cluster-toggle-{session_id}"
                                )))
                                .mt(map_px(8., zoom))
                                .h(map_px(24., zoom))
                                .px(map_px(8., zoom))
                                .flex()
                                .items_center()
                                .justify_center()
                                .gap(map_px(4., zoom))
                                .rounded(map_px(6., zoom))
                                .border_1()
                                .border_color(rgb(LINE))
                                .text_size(map_px(8., zoom))
                                .text_color(rgb(BLUE))
                                .cursor_pointer()
                                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                                .hover(|style| style.bg(rgb(PANEL_RAISED)).text_color(rgb(TEXT)))
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    cx.stop_propagation();
                                    this.clear_messages();
                                    let now_expanded = this
                                        .expanded_agent_clusters
                                        .insert(session_id_for_toggle.clone());
                                    if !now_expanded {
                                        this.expanded_agent_clusters.remove(&session_id_for_toggle);
                                    }
                                    this.selected_id = Some(session_id_for_toggle.clone());
                                    this.selected_agent_id = None;
                                    this.map_inspector_open = false;
                                    this.focus_map_node(
                                        &format!("cluster:{session_id_for_toggle}"),
                                        window,
                                    );
                                    cx.notify();
                                }))
                                .child(if expanded { "⌃" } else { "⌄" })
                                .child(if expanded {
                                    "Collapse group"
                                } else {
                                    "Show grouped agents"
                                }),
                        ),
                )
                .when(expanded, |cluster| {
                    cluster.child(
                        div()
                            .flex_none()
                            .p(map_px(10., zoom))
                            .border_t_1()
                            .border_color(rgb(LINE_STRONG))
                            .bg(rgba(0x0f1915fa))
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .children(groups.into_iter().map(|(agent_type, agents)| {
                                let rows = agents
                                    .chunks(columns)
                                    .map(<[CodingSubagent]>::to_vec)
                                    .collect::<Vec<_>>();
                                div()
                                    .mb(map_px(11., zoom))
                                    .child(
                                        div()
                                            .mb(map_px(6., zoom))
                                            .flex()
                                            .items_center()
                                            .gap(map_px(6., zoom))
                                            .text_size(map_px(8., zoom))
                                            .font_weight(FontWeight::BOLD)
                                            .text_color(rgb(TEXT))
                                            .child(agent_type)
                                            .child(
                                                div()
                                                    .px(map_px(5., zoom))
                                                    .py(map_px(1., zoom))
                                                    .rounded_full()
                                                    .bg(rgb(PANEL_RAISED))
                                                    .text_size(map_px(7., zoom))
                                                    .text_color(rgb(FAINT))
                                                    .child(agents.len().to_string()),
                                            ),
                                    )
                                    .child(div().flex().flex_col().gap(map_px(6., zoom)).children(
                                        rows.into_iter().map(|row| {
                                            div()
                                                .flex()
                                                .gap(map_px(MAP_AGENT_TRAY_GAP, zoom))
                                                .children(row.into_iter().map(|agent| {
                                                    self.render_map_tray_agent(
                                                        session_id.clone(),
                                                        agent,
                                                        zoom,
                                                        cx,
                                                    )
                                                }))
                                        }),
                                    ))
                            })),
                    )
                })
                .into_any_element()
            }
            MapNode::Subagent {
                session_id,
                subagent,
            } => {
                let selected = self.selected_agent_id.as_deref() == Some(subagent.id.as_str());
                let agent_id = subagent.id.clone();
                let map_node_id = node_id;
                let color = status_color(subagent.status);
                let last_tool = subagent
                    .last_tool
                    .clone()
                    .unwrap_or_else(|| "No tool".into());
                base.id(SharedString::from(format!("map-agent-{}", subagent.id)))
                    .p(map_px(10., zoom))
                    .rounded(map_px(8., zoom))
                    .border_1()
                    .border_color(if selected { rgb(BLUE) } else { rgb(LINE) })
                    .bg(rgba(0x111a15f2))
                    .hover(|style| style.border_color(rgb(BLUE)).bg(rgb(PANEL_SOFT)))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        cx.stop_propagation();
                        if this.consume_suppressed_map_click(&map_node_id) {
                            cx.notify();
                            return;
                        }
                        this.clear_messages();
                        this.selected_id = Some(session_id.clone());
                        this.selected_agent_id = Some(agent_id.clone());
                        this.map_inspector_open = true;
                        cx.notify();
                    }))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(map_px(6., zoom))
                            .child(div().size(map_px(6., zoom)).rounded_full().bg(color))
                            .child(
                                div()
                                    .max_w(px(positioned.width - 74. * zoom))
                                    .truncate()
                                    .text_size(map_px(8., zoom))
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(rgb(BLUE))
                                    .child(subagent.agent_type),
                            )
                            .child(div().flex_1())
                            .child(
                                div()
                                    .text_size(map_px(7., zoom))
                                    .text_color(rgb(FAINT))
                                    .child(format!("L{}", subagent.spawn_depth)),
                            ),
                    )
                    .child(
                        div()
                            .mt(map_px(5., zoom))
                            .line_clamp(2)
                            .text_size(map_px(9., zoom))
                            .line_height(map_px(12., zoom))
                            .text_color(rgb(MUTED))
                            .child(subagent.description),
                    )
                    .child(
                        div()
                            .mt(map_px(5., zoom))
                            .truncate()
                            .text_size(map_px(7., zoom))
                            .text_color(rgb(FAINT))
                            .child(format!(
                                "{} · {} · Details →",
                                last_tool,
                                relative_time(&subagent.updated_at)
                            )),
                    )
                    .into_any_element()
            }
        }
    }

    fn render_map_tray_agent(
        &self,
        session_id: String,
        agent: CodingSubagent,
        zoom: f32,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let selected = self.selected_agent_id.as_deref() == Some(agent.id.as_str());
        let agent_id = agent.id.clone();
        let element_id = format!("map-tray-agent-{session_id}-{}", agent.id);
        let color = status_color(agent.status);
        let last_tool = agent.last_tool.clone().unwrap_or_else(|| "No tool".into());
        div()
            .id(SharedString::from(element_id))
            .w(map_px(MAP_AGENT_TRAY_CARD_WIDTH, zoom))
            .h(map_px(MAP_AGENT_TRAY_CARD_HEIGHT, zoom))
            .p(map_px(8., zoom))
            .rounded(map_px(7., zoom))
            .border_1()
            .border_color(if selected { rgb(BLUE) } else { rgb(LINE) })
            .bg(if selected {
                rgba(0x7db8c91c)
            } else {
                rgb(PANEL_SOFT)
            })
            .cursor_pointer()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .hover(|style| style.border_color(rgb(BLUE)).bg(rgb(PANEL_RAISED)))
            .on_click(cx.listener(move |this, _, _, cx| {
                cx.stop_propagation();
                this.clear_messages();
                this.selected_id = Some(session_id.clone());
                this.selected_agent_id = Some(agent_id.clone());
                this.map_inspector_open = true;
                cx.notify();
            }))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(map_px(5., zoom))
                    .child(div().size(map_px(6., zoom)).rounded_full().bg(color))
                    .child(
                        div()
                            .min_w(px(0.))
                            .flex_1()
                            .truncate()
                            .text_size(map_px(8., zoom))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(rgb(TEXT))
                            .child(agent.description),
                    )
                    .child(
                        div()
                            .text_size(map_px(7., zoom))
                            .text_color(rgb(FAINT))
                            .child(format!("L{}", agent.spawn_depth)),
                    ),
            )
            .child(
                div()
                    .mt(map_px(6., zoom))
                    .flex()
                    .items_center()
                    .text_size(map_px(7., zoom))
                    .text_color(rgb(FAINT))
                    .child(last_tool)
                    .child(div().flex_1())
                    .child("Details →"),
            )
            .into_any_element()
    }

    fn render_map_inspector(&self, cx: &mut Context<Self>) -> Div {
        let Some(session) = self.selected() else {
            return div();
        };
        let selected_agent = self.selected_agent_id.as_ref().and_then(|agent_id| {
            session
                .subagents
                .iter()
                .find(|agent| &agent.id == agent_id)
                .cloned()
        });
        let heading = if selected_agent.is_some() {
            "SUBAGENT DETAIL"
        } else {
            "SESSION DETAIL"
        };
        let body = if let Some(agent) = selected_agent {
            let status_color = status_color(agent.status);
            let last_tool = agent.last_tool.clone().unwrap_or_else(|| "No tool".into());
            div()
                .p(px(16.))
                .child(
                    div()
                        .id("map-inspector-back")
                        .mb(px(15.))
                        .text_size(px(9.))
                        .text_color(rgb(BLUE))
                        .cursor_pointer()
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.selected_agent_id = None;
                            cx.notify();
                        }))
                        .child("← All agents"),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(7.))
                        .child(div().size(px(7.)).rounded_full().bg(status_color))
                        .child(
                            div()
                                .text_size(px(10.))
                                .font_weight(FontWeight::BOLD)
                                .text_color(rgb(BLUE))
                                .child(agent.agent_type),
                        )
                        .child(div().flex_1())
                        .child(
                            div()
                                .text_size(px(8.))
                                .text_color(rgb(FAINT))
                                .child(format!("L{}", agent.spawn_depth)),
                        ),
                )
                .child(
                    div()
                        .mt(px(12.))
                        .text_size(px(13.))
                        .line_height(px(19.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(rgb(TEXT))
                        .child(agent.description),
                )
                .child(
                    div()
                        .mt(px(18.))
                        .pt(px(13.))
                        .border_t_1()
                        .border_color(rgb(LINE))
                        .flex()
                        .flex_col()
                        .gap(px(10.))
                        .child(metadata_row("Status", status_label(agent.status).into()))
                        .child(metadata_row("Messages", agent.message_count.to_string()))
                        .child(metadata_row("Last tool", last_tool))
                        .child(metadata_row("Updated", relative_time(&agent.updated_at)))
                        .child(metadata_row(
                            "Parent",
                            agent
                                .parent_agent_id
                                .clone()
                                .unwrap_or_else(|| "Session (L1)".into()),
                        )),
                )
                .into_any_element()
        } else {
            let session_id = session.key();
            let session_id_for_messages = session.id.clone();
            let provider_for_messages = session.provider;
            let session_title_for_messages = session.title.clone();
            let status_color = status_color(session.status);
            let branch = session
                .git_branch
                .clone()
                .unwrap_or_else(|| "no branch".into());
            div()
                .p(px(16.))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(7.))
                        .child(div().size(px(7.)).rounded_full().bg(status_color))
                        .child(
                            div()
                                .text_size(px(9.))
                                .font_weight(FontWeight::BOLD)
                                .text_color(status_color)
                                .child(status_label(session.status)),
                        ),
                )
                .child(
                    div()
                        .mt(px(9.))
                        .text_size(px(14.))
                        .line_height(px(19.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(rgb(TEXT))
                        .child(session.title.clone()),
                )
                .child(
                    div()
                        .mt(px(14.))
                        .flex()
                        .flex_col()
                        .gap(px(9.))
                        .child(metadata_row("Project", session.project_name.clone()))
                        .child(metadata_row(
                            "Agent",
                            session.provider.display_name().into(),
                        ))
                        .child(metadata_row("Branch", branch))
                        .child(metadata_action_row(
                            format!("map-messages-{}", session.id),
                            "Messages",
                            session.message_count.to_string(),
                            cx.listener(move |this, _, _, cx| {
                                this.open_messages(
                                    session_id_for_messages.clone(),
                                    provider_for_messages,
                                    session_title_for_messages.clone(),
                                    cx,
                                );
                            }),
                        ))
                        .child(metadata_row(
                            "Subagents",
                            session.subagents.len().to_string(),
                        ))
                        .child(metadata_row("Updated", relative_time(&session.updated_at))),
                )
                .when(!session.subagents.is_empty(), |body| {
                    body.child(
                        div()
                            .mt(px(20.))
                            .pt(px(15.))
                            .border_t_1()
                            .border_color(rgb(LINE))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .child(eyebrow(
                                        if session.subagents.len() > MAP_AGENT_COMPACT_THRESHOLD {
                                            "ALL SUBAGENTS"
                                        } else {
                                            "SUBAGENTS"
                                        },
                                    ))
                                    .child(div().flex_1())
                                    .child(
                                        div()
                                            .text_size(px(8.))
                                            .text_color(rgb(FAINT))
                                            .child(subagent_status_summary(&session.subagents)),
                                    ),
                            )
                            .child(div().mt(px(10.)).flex().flex_col().gap(px(5.)).children(
                                session.subagents.iter().cloned().map(|agent| {
                                    self.render_map_inspector_agent(session_id.clone(), agent, cx)
                                }),
                            )),
                    )
                })
                .into_any_element()
        };

        div()
            .absolute()
            .top(px(58.))
            .right(px(0.))
            .bottom(px(0.))
            .w(px(340.))
            .flex()
            .flex_col()
            .border_l_1()
            .border_color(rgb(LINE_STRONG))
            .bg(rgba(0x0f1713fc))
            .shadow_lg()
            .occlude()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .h(px(45.))
                    .flex_none()
                    .px(px(14.))
                    .flex()
                    .items_center()
                    .border_b_1()
                    .border_color(rgb(LINE))
                    .child(eyebrow(heading))
                    .child(div().flex_1())
                    .child(
                        div()
                            .id("map-inspector-close")
                            .size(px(25.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(5.))
                            .text_size(px(13.))
                            .text_color(rgb(FAINT))
                            .cursor_pointer()
                            .hover(|style| style.bg(rgb(PANEL_RAISED)).text_color(rgb(TEXT)))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.map_inspector_open = false;
                                this.selected_agent_id = None;
                                cx.notify();
                            }))
                            .child("×"),
                    ),
            )
            .child(
                div()
                    .id("map-inspector-scroll")
                    .flex_1()
                    .min_h(px(0.))
                    .overflow_y_scroll()
                    .child(body),
            )
    }

    fn render_map_inspector_agent(
        &self,
        session_id: String,
        agent: CodingSubagent,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let selected = self.selected_agent_id.as_deref() == Some(agent.id.as_str());
        let agent_id = agent.id.clone();
        let color = status_color(agent.status);
        div()
            .id(SharedString::from(format!(
                "map-inspector-agent-{}",
                agent.id
            )))
            .min_h(px(47.))
            .px(px(9.))
            .py(px(7.))
            .rounded(px(6.))
            .border_1()
            .border_color(if selected { rgb(BLUE) } else { rgb(LINE) })
            .bg(if selected {
                rgba(0x7db8c918)
            } else {
                rgb(PANEL_SOFT)
            })
            .cursor_pointer()
            .hover(|style| style.border_color(rgb(BLUE)))
            .on_click(cx.listener(move |this, _, _, cx| {
                this.selected_id = Some(session_id.clone());
                this.selected_agent_id = Some(agent_id.clone());
                this.map_inspector_open = true;
                cx.notify();
            }))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .child(div().size(px(6.)).rounded_full().bg(color))
                    .child(
                        div()
                            .text_size(px(8.))
                            .font_weight(FontWeight::BOLD)
                            .text_color(rgb(BLUE))
                            .child(agent.agent_type),
                    )
                    .child(div().flex_1())
                    .child(
                        div()
                            .text_size(px(7.))
                            .text_color(rgb(FAINT))
                            .child(format!("L{}", agent.spawn_depth)),
                    ),
            )
            .child(
                div()
                    .mt(px(4.))
                    .truncate()
                    .text_size(px(9.))
                    .text_color(rgb(MUTED))
                    .child(agent.description),
            )
            .into_any_element()
    }

    fn render_messages_drawer(&self, cx: &mut Context<Self>) -> Div {
        let messages_session = self.messages_session_id.as_ref().and_then(|session_id| {
            self.scan
                .sessions
                .iter()
                .find(|session| session.key() == *session_id)
        });
        let session_event_count = messages_session.map_or(0, |session| session.message_count);
        let assistant_label = messages_session
            .map(|session| session.provider.display_name().to_uppercase())
            .unwrap_or_else(|| "AGENT".into());
        let body =
            if self.messages_loading {
                div()
                    .flex_1()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(11.))
                    .text_color(rgb(MUTED))
                    .child("Loading complete history…")
                    .into_any_element()
            } else if let Some(error) = self.messages_error.as_ref() {
                div()
                    .m(px(18.))
                    .p(px(14.))
                    .rounded(px(7.))
                    .border_1()
                    .border_color(rgba(0xdd7b7255))
                    .bg(rgba(0x3b1d1a88))
                    .text_size(px(10.))
                    .line_height(px(15.))
                    .text_color(rgb(DANGER))
                    .whitespace_normal()
                    .child(error.clone())
                    .into_any_element()
            } else if self.messages.is_empty() {
                div()
                    .flex_1()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(11.))
                    .text_color(rgb(MUTED))
                    .child("No readable user or assistant messages.")
                    .into_any_element()
            } else {
                div()
                    .id("messages-history-scroll")
                    .flex_1()
                    .min_h(px(0.))
                    .overflow_y_scroll()
                    .p(px(18.))
                    .children(self.messages.iter().cloned().map(|message| {
                        render_conversation_message(message, assistant_label.clone())
                    }))
                    .into_any_element()
            };

        div()
            .absolute()
            .top(px(48.))
            .right(px(0.))
            .bottom(px(0.))
            .w(px(560.))
            .flex()
            .flex_col()
            .border_l_1()
            .border_color(rgb(LINE_STRONG))
            .bg(rgba(0x0e1511fc))
            .shadow_lg()
            .occlude()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .min_h(px(74.))
                    .flex_none()
                    .px(px(18.))
                    .py(px(13.))
                    .flex()
                    .items_center()
                    .border_b_1()
                    .border_color(rgb(LINE))
                    .child(
                        div()
                            .min_w(px(0.))
                            .flex_1()
                            .child(eyebrow("MESSAGE HISTORY"))
                            .child(
                                div()
                                    .mt(px(4.))
                                    .line_clamp(1)
                                    .text_size(px(13.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(rgb(TEXT))
                                    .child(self.messages_title.clone()),
                            )
                            .child(
                                div()
                                    .mt(px(3.))
                                    .text_size(px(8.))
                                    .text_color(rgb(FAINT))
                                    .child(format!(
                                        "{} readable messages · {} session events",
                                        self.messages.len(),
                                        session_event_count
                                    )),
                            ),
                    )
                    .child(
                        div()
                            .id("messages-history-close")
                            .ml(px(12.))
                            .size(px(28.))
                            .flex_none()
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(6.))
                            .text_size(px(14.))
                            .text_color(rgb(FAINT))
                            .cursor_pointer()
                            .hover(|style| style.bg(rgb(PANEL_RAISED)).text_color(rgb(TEXT)))
                            .on_click(cx.listener(|this, _, _, cx| this.close_messages(cx)))
                            .child("×"),
                    ),
            )
            .child(body)
    }

    fn render_workspace(&self, cx: &mut Context<Self>) -> Div {
        let Some(session) = self.selected() else {
            return div()
                .flex_1()
                .h_full()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .text_color(rgb(FAINT))
                .child(div().text_size(px(28.)).text_color(rgb(GREEN)).child("♧"))
                .child(
                    div()
                        .mt(px(15.))
                        .text_size(px(17.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(rgb(TEXT))
                        .child("No sessions found"),
                )
                .child(
                    div()
                        .mt(px(7.))
                        .text_size(px(11.))
                        .child("Start Claude Code or Codex in another terminal."),
                );
        };

        let status = status_label(session.status);
        let status_color = status_color(session.status);
        let cwd = short_path(&session.cwd);
        let branch = session
            .git_branch
            .clone()
            .unwrap_or_else(|| "no branch".into());

        div()
            .min_w(px(0.))
            .flex_1()
            .h_full()
            .flex()
            .flex_col()
            .child(
                div()
                    .min_h(px(164.))
                    .flex_none()
                    .px(px(43.))
                    .py(px(29.))
                    .border_b_1()
                    .border_color(rgb(LINE))
                    .bg(rgb(0x111914))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(7.))
                            .text_size(px(10.))
                            .text_color(rgb(FAINT))
                            .child(div().size(px(7.)).rounded_full().bg(status_color))
                            .child(
                                div()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(status_color)
                                    .child(status),
                            )
                            .child("·")
                            .child(relative_time(&session.updated_at)),
                    )
                    .child(
                        div()
                            .mt(px(13.))
                            .line_clamp(2)
                            .whitespace_normal()
                            .text_size(px(30.))
                            .line_height(px(36.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(rgb(0xf1f6f2))
                            .child(session.title.clone()),
                    )
                    .child(
                        div()
                            .mt(px(13.))
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .text_size(px(10.))
                            .text_color(rgb(MUTED))
                            .child("▱")
                            .child(cwd)
                            .child(div().mx(px(4.)).w(px(1.)).h(px(12.)).bg(rgb(LINE_STRONG)))
                            .child("⑂")
                            .child(branch),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .min_h(px(0.))
                    .flex()
                    .child(self.render_activity(&session))
                    .child(self.render_inspector(&session, cx)),
            )
    }

    fn render_activity(&self, session: &CodingSession) -> impl IntoElement {
        let status_color = status_color(session.status);
        let state_description = match session.status {
            SessionStatus::Active => format!(
                "{} activity was written recently.",
                session
                    .last_tool
                    .as_deref()
                    .unwrap_or(session.provider.display_name())
            ),
            SessionStatus::Waiting => {
                "The last turn finished and may need your next prompt.".into()
            }
            SessionStatus::Idle => "No recent log activity was detected.".into(),
        };

        div()
            .id("activity")
            .min_w(px(360.))
            .flex_1()
            .h_full()
            .overflow_y_scroll()
            .px(px(40.))
            .py(px(28.))
            .child(
                div()
                    .flex()
                    .items_end()
                    .justify_between()
                    .child(
                        div().child(eyebrow("ACTIVITY")).child(
                            div()
                                .mt(px(4.))
                                .text_size(px(16.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(rgb(TEXT))
                                .child("Latest movement"),
                        ),
                    )
                    .child(
                        div()
                            .text_size(px(9.))
                            .text_color(rgb(FAINT))
                            .child("⌁ Live · 5s"),
                    ),
            )
            .child(
                div()
                    .mt(px(17.))
                    .min_h(px(76.))
                    .px(px(16.))
                    .py(px(13.))
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .border_1()
                    .border_l_2()
                    .border_color(status_color)
                    .bg(rgb(PANEL_SOFT))
                    .child(
                        div()
                            .size(px(35.))
                            .flex_none()
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded_full()
                            .border_1()
                            .border_color(status_color)
                            .text_color(status_color)
                            .child("●"),
                    )
                    .child(
                        div()
                            .flex_1()
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(rgb(TEXT))
                                    .child(status_label(session.status)),
                            )
                            .child(
                                div()
                                    .mt(px(4.))
                                    .text_size(px(10.))
                                    .text_color(rgb(MUTED))
                                    .child(state_description),
                            ),
                    )
                    .child(
                        div()
                            .text_size(px(9.))
                            .text_color(rgb(FAINT))
                            .child(relative_time(&session.updated_at)),
                    ),
            )
            .child(
                div()
                    .mt(px(25.))
                    .ml(px(13.))
                    .pl(px(31.))
                    .border_l_1()
                    .border_color(rgb(LINE_STRONG))
                    .when(session.activities.is_empty(), |list| {
                        list.child(
                            div()
                                .text_size(px(10.))
                                .text_color(rgb(FAINT))
                                .child("No displayable activity."),
                        )
                    })
                    .children(session.activities.iter().cloned().map(render_activity_item)),
            )
    }

    fn render_inspector(
        &self,
        session: &CodingSession,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let session_key = session.key();
        let assigned = self.preferences.assignments.get(&session_key).cloned();
        let session_id_for_ungroup = session_key.clone();
        let session_id_for_copy = session.id.clone();
        let session_id_for_messages = session.id.clone();
        let provider_for_messages = session.provider;
        let session_title_for_messages = session.title.clone();
        let copied = self.copied_session.as_deref() == Some(session.id.as_str());
        let command = session.provider.resume_command(&session.id);
        let groups = self.preferences.groups.clone();

        div()
            .id("inspector")
            .w(px(276.))
            .flex_none()
            .h_full()
            .overflow_y_scroll()
            .border_l_1()
            .border_color(rgb(LINE))
            .bg(rgba(0x0e1511aa))
            .child(
                div()
                    .p(px(21.))
                    .border_b_1()
                    .border_color(rgb(LINE))
                    .child(eyebrow("ORGANIZE"))
                    .child(
                        div()
                            .mt(px(14.))
                            .text_size(px(9.))
                            .text_color(rgb(MUTED))
                            .child("Branch"),
                    )
                    .child(
                        div()
                            .mt(px(7.))
                            .flex()
                            .flex_wrap()
                            .gap(px(6.))
                            .child(branch_chip(
                                "__ungrouped__".into(),
                                "Ungrouped".into(),
                                assigned.is_none(),
                                cx.listener(move |this, _, _, cx| {
                                    this.assign_session(&session_id_for_ungroup, None);
                                    cx.notify();
                                }),
                            ))
                            .children(groups.into_iter().map(|group| {
                                let selected = assigned.as_deref() == Some(group.id.as_str());
                                let chip_id = group.id.clone();
                                let group_id = group.id.clone();
                                let session_id = session_key.clone();
                                branch_chip(
                                    chip_id,
                                    group.name,
                                    selected,
                                    cx.listener(move |this, _, _, cx| {
                                        this.assign_session(&session_id, Some(&group_id));
                                        cx.notify();
                                    }),
                                )
                            })),
                    ),
            )
            .child(
                div()
                    .p(px(21.))
                    .border_b_1()
                    .border_color(rgb(LINE))
                    .child(eyebrow("SESSION"))
                    .child(
                        div()
                            .mt(px(15.))
                            .flex()
                            .flex_col()
                            .gap(px(11.))
                            .child(metadata_row("Status", status_label(session.status).into()))
                            .child(metadata_row(
                                "Agent",
                                session.provider.display_name().into(),
                            ))
                            .child(metadata_action_row(
                                format!("tree-messages-{}", session.id),
                                "Messages",
                                session.message_count.to_string(),
                                cx.listener(move |this, _, _, cx| {
                                    this.open_messages(
                                        session_id_for_messages.clone(),
                                        provider_for_messages,
                                        session_title_for_messages.clone(),
                                        cx,
                                    );
                                }),
                            ))
                            .child(metadata_row(
                                "Subagents",
                                session.subagents.len().to_string(),
                            ))
                            .child(metadata_row(
                                "Started",
                                session
                                    .started_at
                                    .as_deref()
                                    .map(relative_time)
                                    .unwrap_or_else(|| "Unknown".into()),
                            ))
                            .child(metadata_row(
                                "Slug",
                                session.slug.clone().unwrap_or_else(|| "—".into()),
                            )),
                    ),
            )
            .child(
                div()
                    .p(px(21.))
                    .border_b_1()
                    .border_color(rgb(LINE))
                    .child(eyebrow("RESUME"))
                    .child(
                        div()
                            .mt(px(12.))
                            .text_size(px(9.))
                            .line_height(px(14.))
                            .text_color(rgb(MUTED))
                            .child(format!(
                                "Continue from any terminal with {}.",
                                session.provider.display_name()
                            )),
                    )
                    .child(
                        div()
                            .id("copy-session")
                            .mt(px(11.))
                            .h(px(32.))
                            .px(px(8.))
                            .flex()
                            .items_center()
                            .justify_between()
                            .rounded(px(5.))
                            .border_1()
                            .border_color(rgb(LINE_STRONG))
                            .bg(rgb(PANEL_SOFT))
                            .cursor_pointer()
                            .on_click(cx.listener(move |this, _, _, cx| {
                                cx.write_to_clipboard(ClipboardItem::new_string(
                                    session_id_for_copy.clone(),
                                ));
                                this.copied_session = Some(session_id_for_copy.clone());
                                cx.notify();
                            }))
                            .child(
                                div()
                                    .max_w(px(205.))
                                    .truncate()
                                    .text_size(px(8.))
                                    .text_color(rgb(MUTED))
                                    .child(session.id.clone()),
                            )
                            .child(if copied { "✓" } else { "⧉" }),
                    )
                    .child(
                        div()
                            .mt(px(8.))
                            .pl(px(8.))
                            .border_l_2()
                            .border_color(rgb(LINE_STRONG))
                            .truncate()
                            .text_size(px(8.))
                            .text_color(rgb(FAINT))
                            .child(command),
                    ),
            )
            .child(
                div()
                    .m(px(20.))
                    .p(px(11.))
                    .flex()
                    .items_start()
                    .gap(px(8.))
                    .border_1()
                    .border_color(rgb(LINE))
                    .text_size(px(8.))
                    .line_height(px(12.))
                    .text_color(rgb(FAINT))
                    .child(div().flex_none().child("◷"))
                    .child(div().min_w(px(0.)).flex_1().whitespace_normal().child(
                        "Read-only local viewer. Status is inferred from local agent log events.",
                    )),
            )
    }
}

impl gpui::Render for Grove {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let warning = self.warning_message();
        let main_content = match self.view_mode {
            ViewMode::Detail => div()
                .flex_1()
                .min_h(px(0.))
                .flex()
                .child(self.render_sidebar(window, cx))
                .child(self.render_workspace(cx))
                .into_any_element(),
            ViewMode::Map => self.render_mind_map(cx).into_any_element(),
        };
        div()
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(BG))
            .text_color(rgb(TEXT))
            .font_family(".SystemUIFont")
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                this.dismiss_active_overlay_on_escape(event, cx);
            }))
            .child(self.render_titlebar(cx))
            .when_some(warning, |app, warning| {
                app.child(
                    div()
                        .min_h(px(32.))
                        .flex_none()
                        .px(px(16.))
                        .py(px(7.))
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .border_b_1()
                        .border_color(rgba(0xe8b86a55))
                        .bg(rgba(0x3b2d18dd))
                        .text_size(px(9.))
                        .text_color(rgb(AMBER))
                        .child("⚠")
                        .child(div().min_w(px(0.)).truncate().child(warning)),
                )
            })
            .child(main_content)
            .when(self.messages_open, |app| {
                app.child(self.render_messages_drawer(cx))
            })
    }
}

fn build_mind_map_layout(sessions: &[CodingSession]) -> MindMapLayout {
    build_mind_map_layout_with_expanded(sessions, &HashSet::new())
}

fn build_mind_map_layout_with_expanded(
    sessions: &[CodingSession],
    expanded_agent_clusters: &HashSet<String>,
) -> MindMapLayout {
    const ROOT_WIDTH: f32 = 190.;
    const ROOT_HEIGHT: f32 = 96.;
    const SESSION_WIDTH: f32 = 260.;
    const SESSION_HEIGHT: f32 = 98.;
    const AGENT_WIDTH: f32 = 220.;
    const AGENT_HEIGHT: f32 = 68.;
    const AGENT_ROW: f32 = 82.;
    const MAP_PADDING_Y: f32 = 90.;

    let max_depth = sessions
        .iter()
        .filter(|session| session.subagents.len() <= MAP_AGENT_COMPACT_THRESHOLD)
        .flat_map(|session| session.subagents.iter())
        .map(|subagent| subagent.spawn_depth)
        .max()
        .unwrap_or(1);
    let width = 2000. + max_depth.saturating_sub(1) as f32 * 640.;
    let center_x = width / 2.;
    let mut left_cursor = MAP_PADDING_Y;
    let mut right_cursor = MAP_PADDING_Y;
    let mut slots = Vec::new();

    for (index, session) in sessions.iter().cloned().enumerate() {
        let session_key = session.key();
        let block_height = if session.subagents.is_empty() {
            132.
        } else if session.subagents.len() > MAP_AGENT_COMPACT_THRESHOLD {
            if expanded_agent_clusters.contains(&session_key) {
                expanded_agent_cluster_size(&session.subagents).1 + 40.
            } else {
                166.
            }
        } else {
            session.subagents.len() as f32 * AGENT_ROW + 40.
        };
        let place_left = match index {
            0 => true,
            1 => false,
            _ => left_cursor <= right_cursor,
        };
        let top = if place_left {
            let top = left_cursor;
            left_cursor += block_height + 46.;
            top
        } else {
            let top = right_cursor;
            right_cursor += block_height + 46.;
            top
        };
        slots.push((session, place_left, top, block_height));
    }

    let height = (left_cursor.max(right_cursor) + MAP_PADDING_Y).max(720.);
    let root_x = center_x - ROOT_WIDTH / 2.;
    let root_y = height / 2. - ROOT_HEIGHT / 2.;
    let root_id = "grove".to_owned();
    let mut nodes = vec![PositionedMapNode {
        id: root_id.clone(),
        node: MapNode::Grove,
        x: root_x,
        y: root_y,
        width: ROOT_WIDTH,
        height: ROOT_HEIGHT,
    }];
    let mut edges = Vec::new();

    for (session, place_left, top, block_height) in slots {
        let session_key = session.key();
        let session_node_id = format!("session:{session_key}");
        let session_x = if place_left {
            center_x - 280. - SESSION_WIDTH
        } else {
            center_x + 280.
        };
        let session_y = top + (block_height - SESSION_HEIGHT) / 2.;
        let edge_color = status_color(session.status);
        let (root_edge_x, session_edge_x) = if place_left {
            (root_x, session_x + SESSION_WIDTH)
        } else {
            (root_x + ROOT_WIDTH, session_x)
        };
        edges.push(MapEdge {
            from_id: root_id.clone(),
            to_id: session_node_id.clone(),
            from_x: root_edge_x,
            from_y: root_y + ROOT_HEIGHT / 2.,
            to_x: session_edge_x,
            to_y: session_y + SESSION_HEIGHT / 2.,
            color: edge_color,
        });
        nodes.push(PositionedMapNode {
            id: session_node_id.clone(),
            node: MapNode::Session(session.clone()),
            x: session_x,
            y: session_y,
            width: SESSION_WIDTH,
            height: SESSION_HEIGHT,
        });

        if session.subagents.len() > MAP_AGENT_COMPACT_THRESHOLD {
            let cluster_node_id = format!("cluster:{session_key}");
            let (cluster_width, cluster_height) = if expanded_agent_clusters.contains(&session_key)
            {
                expanded_agent_cluster_size(&session.subagents)
            } else {
                (MAP_AGENT_CLUSTER_WIDTH, MAP_AGENT_CLUSTER_HEIGHT)
            };
            let cluster_x = if place_left {
                session_x - 130. - cluster_width
            } else {
                session_x + SESSION_WIDTH + 130.
            };
            let cluster_y = top + (block_height - cluster_height) / 2.;
            let (session_cluster_edge_x, cluster_edge_x) = if place_left {
                (session_x, cluster_x + cluster_width)
            } else {
                (session_x + SESSION_WIDTH, cluster_x)
            };
            edges.push(MapEdge {
                from_id: session_node_id.clone(),
                to_id: cluster_node_id.clone(),
                from_x: session_cluster_edge_x,
                from_y: session_y + SESSION_HEIGHT / 2.,
                to_x: cluster_edge_x,
                to_y: cluster_y + MAP_AGENT_CLUSTER_HEIGHT / 2.,
                color: rgb(BLUE).into(),
            });
            nodes.push(PositionedMapNode {
                id: cluster_node_id,
                node: MapNode::AgentCluster {
                    session_id: session_key.clone(),
                    subagents: session.subagents.clone(),
                },
                x: cluster_x,
                y: cluster_y,
                width: cluster_width,
                height: cluster_height,
            });
            continue;
        }

        let mut agent_positions: HashMap<String, (f32, f32, f32, f32)> = HashMap::new();
        for (agent_index, subagent) in session.subagents.iter().cloned().enumerate() {
            let agent_node_id = format!("agent:{session_key}:{}", subagent.id);
            let depth = subagent.spawn_depth.max(1) as f32;
            let agent_x = if place_left {
                session_x - 130. - AGENT_WIDTH - (depth - 1.) * 320.
            } else {
                session_x + SESSION_WIDTH + 130. + (depth - 1.) * 320.
            };
            let agent_y = top + 20. + agent_index as f32 * AGENT_ROW;
            let parent = subagent
                .parent_agent_id
                .as_ref()
                .and_then(|parent_id| agent_positions.get(parent_id))
                .copied()
                .unwrap_or((session_x, session_y, SESSION_WIDTH, SESSION_HEIGHT));
            let parent_node_id = subagent
                .parent_agent_id
                .as_ref()
                .filter(|parent_id| agent_positions.contains_key(*parent_id))
                .map(|parent_id| format!("agent:{session_key}:{parent_id}"))
                .unwrap_or_else(|| session_node_id.clone());
            let (parent_edge_x, agent_edge_x) = if place_left {
                (parent.0, agent_x + AGENT_WIDTH)
            } else {
                (parent.0 + parent.2, agent_x)
            };
            edges.push(MapEdge {
                from_id: parent_node_id,
                to_id: agent_node_id.clone(),
                from_x: parent_edge_x,
                from_y: parent.1 + parent.3 / 2.,
                to_x: agent_edge_x,
                to_y: agent_y + AGENT_HEIGHT / 2.,
                color: rgb(BLUE).into(),
            });
            agent_positions.insert(
                subagent.id.clone(),
                (agent_x, agent_y, AGENT_WIDTH, AGENT_HEIGHT),
            );
            nodes.push(PositionedMapNode {
                id: agent_node_id,
                node: MapNode::Subagent {
                    session_id: session_key.clone(),
                    subagent,
                },
                x: agent_x,
                y: agent_y,
                width: AGENT_WIDTH,
                height: AGENT_HEIGHT,
            });
        }
    }

    let mut layout = MindMapLayout {
        width,
        height,
        nodes,
        edges,
    };
    add_map_perimeter_padding(&mut layout);
    layout
}

fn add_map_perimeter_padding(layout: &mut MindMapLayout) {
    let Some(grove) = layout.nodes.iter().find(|node| node.id == "grove") else {
        return;
    };
    let root_center_x = grove.x + grove.width / 2.;
    let root_center_y = grove.y + grove.height / 2.;
    let min_x = layout
        .nodes
        .iter()
        .map(|node| node.x)
        .fold(root_center_x, f32::min);
    let max_x = layout
        .nodes
        .iter()
        .map(|node| node.x + node.width)
        .fold(root_center_x, f32::max);
    let min_y = layout
        .nodes
        .iter()
        .map(|node| node.y)
        .fold(root_center_y, f32::min);
    let max_y = layout
        .nodes
        .iter()
        .map(|node| node.y + node.height)
        .fold(root_center_y, f32::max);
    let radius_x = (root_center_x - min_x)
        .max(max_x - root_center_x)
        .max(MAP_CANVAS_MIN_RADIUS_X);
    let radius_y = (root_center_y - min_y)
        .max(max_y - root_center_y)
        .max(MAP_CANVAS_MIN_RADIUS_Y);
    let shift_x = radius_x * 2. - root_center_x;
    let shift_y = radius_y * 2. - root_center_y;

    for node in &mut layout.nodes {
        node.x += shift_x;
        node.y += shift_y;
    }
    for edge in &mut layout.edges {
        edge.from_x += shift_x;
        edge.from_y += shift_y;
        edge.to_x += shift_x;
        edge.to_y += shift_y;
    }
    layout.width = radius_x * 4.;
    layout.height = radius_y * 4.;
}

#[cfg(test)]
fn build_mind_map_layout_with_offsets(
    sessions: &[CodingSession],
    offsets: &HashMap<String, MapNodeOffset>,
) -> MindMapLayout {
    build_mind_map_layout_with_state(sessions, offsets, &HashSet::new())
}

fn build_mind_map_layout_with_state(
    sessions: &[CodingSession],
    offsets: &HashMap<String, MapNodeOffset>,
    expanded_agent_clusters: &HashSet<String>,
) -> MindMapLayout {
    let mut layout = build_mind_map_layout_with_expanded(sessions, expanded_agent_clusters);
    for node in &mut layout.nodes {
        if let Some(offset) = offsets.get(&node.id) {
            node.x += offset.x as f32;
            node.y += offset.y as f32;
        }
    }
    refresh_map_edge_geometry(&mut layout);
    layout
}

fn refresh_map_edge_geometry(layout: &mut MindMapLayout) {
    let node_bounds = layout
        .nodes
        .iter()
        .map(|node| {
            let anchor_height = if matches!(&node.node, MapNode::AgentCluster { .. }) {
                MAP_AGENT_CLUSTER_HEIGHT
            } else {
                node.height
            };
            (node.id.clone(), (node.x, node.y, node.width, anchor_height))
        })
        .collect::<HashMap<_, _>>();
    for edge in &mut layout.edges {
        let (Some(from), Some(to)) = (node_bounds.get(&edge.from_id), node_bounds.get(&edge.to_id))
        else {
            continue;
        };
        let from_center_x = from.0 + from.2 / 2.;
        let to_center_x = to.0 + to.2 / 2.;
        if to_center_x < from_center_x {
            edge.from_x = from.0;
            edge.to_x = to.0 + to.2;
        } else {
            edge.from_x = from.0 + from.2;
            edge.to_x = to.0;
        }
        edge.from_y = from.1 + from.3 / 2.;
        edge.to_y = to.1 + to.3 / 2.;
    }
}

fn clamped_node_offset(
    node: &PositionedMapNode,
    layout: &MindMapLayout,
    requested: MapNodeOffset,
) -> MapNodeOffset {
    const NODE_MARGIN: f32 = 20.;
    let requested_x = node.x + requested.x as f32;
    let requested_y = node.y + requested.y as f32;
    let clamped_x = requested_x.clamp(NODE_MARGIN, layout.width - node.width - NODE_MARGIN);
    let clamped_y = requested_y.clamp(NODE_MARGIN, layout.height - node.height - NODE_MARGIN);
    MapNodeOffset {
        x: (clamped_x - node.x).round() as i32,
        y: (clamped_y - node.y).round() as i32,
    }
}

fn scaled_mind_map_layout(mut layout: MindMapLayout, zoom: f32) -> MindMapLayout {
    let zoom = clamp_map_zoom(zoom);
    layout.width *= zoom;
    layout.height *= zoom;
    for node in &mut layout.nodes {
        node.x *= zoom;
        node.y *= zoom;
        node.width *= zoom;
        node.height *= zoom;
    }
    for edge in &mut layout.edges {
        edge.from_x *= zoom;
        edge.from_y *= zoom;
        edge.to_x *= zoom;
        edge.to_y *= zoom;
    }
    layout
}

fn clamp_map_zoom(zoom: f32) -> f32 {
    zoom.clamp(MAP_ZOOM_MIN, MAP_ZOOM_MAX)
}

fn clamped_map_offset(
    requested: (f32, f32),
    map_size: (f32, f32),
    viewport_size: (f32, f32),
) -> (f32, f32) {
    let min_x = (viewport_size.0 - map_size.0).min(0.);
    let min_y = (viewport_size.1 - map_size.1).min(0.);
    (requested.0.clamp(min_x, 0.), requested.1.clamp(min_y, 0.))
}

#[cfg(test)]
fn zoomed_map_offset(
    old_offset: (f32, f32),
    old_zoom: f32,
    new_zoom: f32,
    map_size: (f32, f32),
    viewport_size: (f32, f32),
) -> (f32, f32) {
    zoomed_map_offset_around(
        old_offset,
        old_zoom,
        new_zoom,
        map_size,
        viewport_size,
        (viewport_size.0 / 2., viewport_size.1 / 2.),
    )
}

fn zoomed_map_offset_around(
    old_offset: (f32, f32),
    old_zoom: f32,
    new_zoom: f32,
    map_size: (f32, f32),
    viewport_size: (f32, f32),
    anchor: (f32, f32),
) -> (f32, f32) {
    let old_zoom = clamp_map_zoom(old_zoom);
    let new_zoom = clamp_map_zoom(new_zoom);
    let content_anchor_x = (anchor.0 - old_offset.0) / old_zoom;
    let content_anchor_y = (anchor.1 - old_offset.1) / old_zoom;
    let requested_x = anchor.0 - content_anchor_x * new_zoom;
    let requested_y = anchor.1 - content_anchor_y * new_zoom;
    clamped_map_offset(
        (requested_x, requested_y),
        (map_size.0 * new_zoom, map_size.1 * new_zoom),
        viewport_size,
    )
}

fn centered_map_offset(
    map_width: f32,
    map_height: f32,
    viewport_width: f32,
    viewport_height: f32,
) -> (f32, f32) {
    (
        (viewport_width / 2. - map_width / 2.).min(0.),
        (viewport_height / 2. - map_height / 2.).min(0.),
    )
}

fn map_px(value: f32, zoom: f32) -> gpui::Pixels {
    px(value * zoom)
}

fn subagent_type_summary(subagents: &[CodingSubagent]) -> String {
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for subagent in subagents {
        *counts.entry(subagent.agent_type.as_str()).or_default() += 1;
    }
    let mut counts = counts.into_iter().collect::<Vec<_>>();
    counts.sort_by(|(type_a, count_a), (type_b, count_b)| {
        count_b.cmp(count_a).then_with(|| type_a.cmp(type_b))
    });
    counts
        .into_iter()
        .take(3)
        .map(|(agent_type, count)| format!("{agent_type} × {count}"))
        .collect::<Vec<_>>()
        .join("  ·  ")
}

fn grouped_subagents(subagents: &[CodingSubagent]) -> Vec<(String, Vec<CodingSubagent>)> {
    let mut groups: HashMap<String, Vec<CodingSubagent>> = HashMap::new();
    for subagent in subagents {
        groups
            .entry(subagent.agent_type.clone())
            .or_default()
            .push(subagent.clone());
    }
    let mut groups = groups.into_iter().collect::<Vec<_>>();
    groups.sort_by(|(type_a, agents_a), (type_b, agents_b)| {
        agents_b
            .len()
            .cmp(&agents_a.len())
            .then_with(|| type_a.cmp(type_b))
    });
    groups
}

fn agent_tray_column_count(agent_count: usize) -> usize {
    if agent_count > 48 {
        4
    } else if agent_count > 24 {
        3
    } else {
        2
    }
}

fn expanded_agent_cluster_size(subagents: &[CodingSubagent]) -> (f32, f32) {
    const GROUP_HEADER_HEIGHT: f32 = 16.;
    const GROUP_HEADER_MARGIN: f32 = 6.;
    const GROUP_BOTTOM_MARGIN: f32 = 11.;

    let columns = agent_tray_column_count(subagents.len());
    let width = MAP_AGENT_TRAY_PADDING * 2.
        + MAP_AGENT_TRAY_CARD_WIDTH * columns as f32
        + MAP_AGENT_TRAY_GAP * columns.saturating_sub(1) as f32
        + MAP_AGENT_TRAY_WIDTH_BUFFER;
    let groups_height = grouped_subagents(subagents)
        .into_iter()
        .map(|(_, agents)| {
            let rows = agents.len().div_ceil(columns);
            GROUP_HEADER_HEIGHT
                + GROUP_HEADER_MARGIN
                + MAP_AGENT_TRAY_CARD_HEIGHT * rows as f32
                + MAP_AGENT_TRAY_GAP * rows.saturating_sub(1) as f32
                + GROUP_BOTTOM_MARGIN
        })
        .sum::<f32>();
    let height = MAP_AGENT_CLUSTER_HEIGHT
        + MAP_AGENT_TRAY_PADDING * 2.
        + groups_height
        + MAP_AGENT_TRAY_HEIGHT_BUFFER;
    (width.max(MAP_AGENT_CLUSTER_WIDTH), height)
}

fn subagent_status_summary(subagents: &[CodingSubagent]) -> String {
    let active = subagents
        .iter()
        .filter(|agent| agent.status == SessionStatus::Active)
        .count();
    let waiting = subagents
        .iter()
        .filter(|agent| agent.status == SessionStatus::Waiting)
        .count();
    let idle = subagents.len().saturating_sub(active + waiting);
    format!("Working {active}  ·  Waiting {waiting}  ·  Idle {idle}")
}

fn render_activity_item(activity: SessionActivity) -> impl IntoElement {
    let (symbol, color) = match activity.kind {
        ActivityKind::Tool => ("⌘", rgb(BLUE)),
        ActivityKind::Prompt => ("↳", rgb(AMBER)),
        ActivityKind::Response => ("✦", rgb(GREEN)),
    };
    div()
        .relative()
        .min_h(px(66.))
        .pb(px(17.))
        .child(
            div()
                .absolute()
                .left(px(-46.))
                .top(px(-1.))
                .size(px(28.))
                .flex()
                .items_center()
                .justify_center()
                .rounded_full()
                .border_1()
                .border_color(rgb(LINE_STRONG))
                .bg(rgb(PANEL_SOFT))
                .text_color(color)
                .text_size(px(12.))
                .child(symbol),
        )
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .text_size(px(11.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(rgb(TEXT))
                        .child(activity.label),
                )
                .child(
                    div().text_size(px(9.)).text_color(rgb(FAINT)).child(
                        activity
                            .timestamp
                            .as_deref()
                            .map(relative_time)
                            .unwrap_or_default(),
                    ),
                ),
        )
        .when_some(activity.detail, |row, detail| {
            row.child(
                div()
                    .mt(px(5.))
                    .line_clamp(2)
                    .text_size(px(10.))
                    .line_height(px(15.))
                    .text_color(rgb(MUTED))
                    .child(detail),
            )
        })
}

fn branch_chip(
    element_id: String,
    label: String,
    selected: bool,
    listener: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    div()
        .id(branch_chip_element_id(&element_id))
        .px(px(8.))
        .h(px(25.))
        .flex()
        .items_center()
        .rounded(px(5.))
        .border_1()
        .border_color(if selected {
            rgb(GREEN)
        } else {
            rgb(LINE_STRONG)
        })
        .bg(if selected {
            rgba(0xa5d56f18)
        } else {
            rgb(PANEL_SOFT)
        })
        .text_size(px(9.))
        .text_color(if selected { rgb(GREEN) } else { rgb(MUTED) })
        .cursor_pointer()
        .hover(|style| style.border_color(rgb(GREEN)))
        .on_click(listener)
        .child(label)
}

fn branch_chip_element_id(group_id: &str) -> SharedString {
    SharedString::from(format!("branch-chip-{group_id}"))
}

fn metadata_row(label: &'static str, value: String) -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .justify_between()
        .gap(px(8.))
        .child(div().text_size(px(9.)).text_color(rgb(FAINT)).child(label))
        .child(
            div()
                .max_w(px(160.))
                .truncate()
                .text_size(px(9.))
                .text_color(rgb(MUTED))
                .child(value),
        )
}

fn metadata_action_row(
    element_id: String,
    label: &'static str,
    value: String,
    listener: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    div()
        .id(SharedString::from(element_id))
        .mx(px(-7.))
        .px(px(7.))
        .h(px(25.))
        .flex()
        .items_center()
        .justify_between()
        .gap(px(8.))
        .rounded(px(5.))
        .cursor_pointer()
        .hover(|style| style.bg(rgb(PANEL_RAISED)))
        .on_click(listener)
        .child(div().text_size(px(9.)).text_color(rgb(FAINT)).child(label))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(5.))
                .text_size(px(9.))
                .text_color(rgb(BLUE))
                .child(value)
                .child("→"),
        )
}

fn render_conversation_message(message: ConversationMessage, assistant_label: String) -> Div {
    let is_user = message.role == ConversationRole::User;
    let timestamp = message
        .timestamp
        .as_deref()
        .map(relative_time)
        .unwrap_or_else(|| "Unknown time".into());
    div()
        .mb(px(13.))
        .flex()
        .when(is_user, |row| row.justify_end())
        .child(
            div()
                .max_w(px(455.))
                .p(px(12.))
                .rounded(px(9.))
                .border_1()
                .border_color(if is_user { rgb(BLUE) } else { rgb(LINE) })
                .bg(if is_user {
                    rgba(0x173039ee)
                } else {
                    rgba(0x141d18f5)
                })
                .child(
                    div()
                        .flex()
                        .items_center()
                        .child(
                            div()
                                .text_size(px(8.))
                                .font_weight(FontWeight::BOLD)
                                .text_color(if is_user { rgb(BLUE) } else { rgb(GREEN) })
                                .child(if is_user {
                                    "YOU".into()
                                } else {
                                    assistant_label
                                }),
                        )
                        .child(div().flex_1())
                        .child(
                            div()
                                .text_size(px(8.))
                                .text_color(rgb(FAINT))
                                .child(timestamp),
                        ),
                )
                .child(
                    div()
                        .mt(px(7.))
                        .whitespace_normal()
                        .text_size(px(10.))
                        .line_height(px(16.))
                        .text_color(rgb(TEXT))
                        .child(message.text),
                ),
        )
}

fn eyebrow(label: &'static str) -> Div {
    div()
        .text_size(px(8.))
        .font_weight(FontWeight::BOLD)
        .text_color(rgb(FAINT))
        .child(label)
}

fn status_label(status: SessionStatus) -> &'static str {
    match status {
        SessionStatus::Active => "Working",
        SessionStatus::Waiting => "Waiting",
        SessionStatus::Idle => "Idle",
    }
}

fn session_matches_status_filter(session: &CodingSession, filter: StatusFilter) -> bool {
    match filter {
        StatusFilter::All => true,
        StatusFilter::Active => session.status == SessionStatus::Active,
        StatusFilter::Waiting => session.status == SessionStatus::Waiting,
        StatusFilter::Idle => session.status == SessionStatus::Idle,
    }
}

fn session_matches_provider_filter(session: &CodingSession, filter: ProviderFilter) -> bool {
    match filter {
        ProviderFilter::All => true,
        ProviderFilter::ClaudeCode => session.provider == CodingAgent::ClaudeCode,
        ProviderFilter::Codex => session.provider == CodingAgent::Codex,
    }
}

fn status_color(status: SessionStatus) -> Hsla {
    match status {
        SessionStatus::Active => rgb(GREEN).into(),
        SessionStatus::Waiting => rgb(AMBER).into(),
        SessionStatus::Idle => rgb(FAINT).into(),
    }
}

fn relative_time(timestamp: &str) -> String {
    let Ok(timestamp) = DateTime::parse_from_rfc3339(timestamp) else {
        return "unknown".into();
    };
    let seconds = (Utc::now() - timestamp.with_timezone(&Utc))
        .num_seconds()
        .max(0);
    match seconds {
        0..=9 => "just now".into(),
        10..=59 => format!("{seconds}s ago"),
        60..=3_599 => format!("{}m ago", seconds / 60),
        3_600..=86_399 => format!("{}h ago", seconds / 3_600),
        _ => format!("{}d ago", seconds / 86_400),
    }
}

fn short_path(path: &str) -> String {
    let components: Vec<_> = Path::new(path).components().collect();
    if components.len() >= 3
        && components[0].as_os_str() == "/"
        && components[1].as_os_str() == "Users"
    {
        let remainder = components
            .iter()
            .skip(3)
            .map(|part| part.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/");
        if remainder.is_empty() {
            "~".into()
        } else {
            format!("~/{remainder}")
        }
    } else {
        path.into()
    }
}

pub fn claude_root() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".claude")
        .join("projects")
}

pub fn codex_root() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".codex")
}

pub fn session_roots() -> scanner::SessionRoots {
    scanner::SessionRoots {
        claude: claude_root(),
        codex: codex_root(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_and_time_helpers_are_stable() {
        assert_eq!(
            short_path("/Users/mai/workspace/grove"),
            "~/workspace/grove"
        );
        assert_eq!(short_path("/tmp/grove"), "/tmp/grove");
    }

    #[test]
    fn filter_matches_project_metadata() {
        let now = Utc::now().to_rfc3339();
        let scan = SessionScan {
            sessions: vec![CodingSession {
                id: "one".into(),
                provider: CodingAgent::ClaudeCode,
                title: "Investigate tests".into(),
                project_name: "looper".into(),
                cwd: "/Users/mai/looper".into(),
                git_branch: Some("fix/retries".into()),
                slug: None,
                status: SessionStatus::Waiting,
                updated_at: now,
                started_at: None,
                message_count: 1,
                last_prompt: None,
                last_tool: None,
                activities: vec![],
                subagents: vec![],
            }],
            scanned_at: Utc::now().to_rfc3339(),
            source_roots: vec!["/tmp".into()],
            skipped_files: 0,
            warnings: vec![],
        };

        let matches = |query: &str| {
            let query = query.to_lowercase();
            scan.sessions.iter().any(|session| {
                [
                    session.title.as_str(),
                    session.project_name.as_str(),
                    session.cwd.as_str(),
                    session.git_branch.as_deref().unwrap_or(""),
                ]
                .join(" ")
                .to_lowercase()
                .contains(&query)
            })
        };
        assert!(matches("looper"));
        assert!(matches("retries"));
        assert!(!matches("missing"));
    }

    #[test]
    fn status_filters_include_only_matching_sessions() {
        let session = |id: &str, status: SessionStatus| CodingSession {
            id: id.into(),
            provider: CodingAgent::ClaudeCode,
            title: id.into(),
            project_name: "grove".into(),
            cwd: "/Users/mai/grove".into(),
            git_branch: None,
            slug: None,
            status,
            updated_at: Utc::now().to_rfc3339(),
            started_at: None,
            message_count: 1,
            last_prompt: None,
            last_tool: None,
            activities: vec![],
            subagents: vec![],
        };
        let sessions = [
            session("working", SessionStatus::Active),
            session("waiting", SessionStatus::Waiting),
            session("idle", SessionStatus::Idle),
        ];
        let matching_ids = |filter| {
            sessions
                .iter()
                .filter(|session| session_matches_status_filter(session, filter))
                .map(|session| session.id.as_str())
                .collect::<Vec<_>>()
        };

        assert_eq!(
            matching_ids(StatusFilter::All),
            vec!["working", "waiting", "idle"]
        );
        assert_eq!(matching_ids(StatusFilter::Active), vec!["working"]);
        assert_eq!(matching_ids(StatusFilter::Waiting), vec!["waiting"]);
        assert_eq!(matching_ids(StatusFilter::Idle), vec!["idle"]);
    }

    #[test]
    fn provider_filters_keep_claude_and_codex_sessions_separate() {
        let session = |id: &str, provider: CodingAgent| CodingSession {
            id: id.into(),
            provider,
            title: id.into(),
            project_name: "grove".into(),
            cwd: "/Users/mai/grove".into(),
            git_branch: None,
            slug: None,
            status: SessionStatus::Idle,
            updated_at: Utc::now().to_rfc3339(),
            started_at: None,
            message_count: 1,
            last_prompt: None,
            last_tool: None,
            activities: vec![],
            subagents: vec![],
        };
        let sessions = [
            session("claude-session", CodingAgent::ClaudeCode),
            session("codex-session", CodingAgent::Codex),
        ];
        let matching_ids = |filter| {
            sessions
                .iter()
                .filter(|session| session_matches_provider_filter(session, filter))
                .map(|session| session.id.as_str())
                .collect::<Vec<_>>()
        };

        assert_eq!(
            matching_ids(ProviderFilter::All),
            vec!["claude-session", "codex-session"]
        );
        assert_eq!(
            matching_ids(ProviderFilter::ClaudeCode),
            vec!["claude-session"]
        );
        assert_eq!(matching_ids(ProviderFilter::Codex), vec!["codex-session"]);
    }

    #[test]
    fn duplicate_group_labels_can_have_unique_chip_ids() {
        assert_ne!(
            branch_chip_element_id("release-train"),
            branch_chip_element_id("release-train-2")
        );
    }

    #[test]
    fn mind_map_places_sessions_and_nested_agents_around_grove() {
        let timestamp = Utc::now().to_rfc3339();
        let subagent = |id: &str, parent: Option<&str>, depth: usize| CodingSubagent {
            id: id.into(),
            parent_agent_id: parent.map(str::to_owned),
            agent_type: "Explore".into(),
            description: format!("Agent {id}"),
            status: SessionStatus::Waiting,
            updated_at: timestamp.clone(),
            message_count: 2,
            last_tool: Some("Read".into()),
            spawn_depth: depth,
        };
        let session = CodingSession {
            id: "session-map".into(),
            provider: CodingAgent::ClaudeCode,
            title: "Build the map".into(),
            project_name: "grove".into(),
            cwd: "/Users/mai/grove".into(),
            git_branch: Some("main".into()),
            slug: None,
            status: SessionStatus::Active,
            updated_at: timestamp.clone(),
            started_at: None,
            message_count: 3,
            last_prompt: None,
            last_tool: Some("Agent".into()),
            activities: vec![],
            subagents: vec![
                subagent("parent", None, 1),
                subagent("child", Some("parent"), 2),
            ],
        };

        let layout = build_mind_map_layout(&[session]);

        assert_eq!(layout.nodes.len(), 4);
        assert_eq!(layout.edges.len(), 3);
        assert!(layout.width > 2_000.);
        let grove = layout
            .nodes
            .iter()
            .find(|node| matches!(node.node, MapNode::Grove))
            .unwrap();
        let root_center_x = grove.x + grove.width / 2.;
        let root_center_y = grove.y + grove.height / 2.;
        assert_eq!(root_center_x, layout.width / 2.);
        assert_eq!(root_center_y, layout.height / 2.);
        let min_x = layout
            .nodes
            .iter()
            .map(|node| node.x)
            .fold(root_center_x, f32::min);
        let max_x = layout
            .nodes
            .iter()
            .map(|node| node.x + node.width)
            .fold(root_center_x, f32::max);
        let min_y = layout
            .nodes
            .iter()
            .map(|node| node.y)
            .fold(root_center_y, f32::min);
        let max_y = layout
            .nodes
            .iter()
            .map(|node| node.y + node.height)
            .fold(root_center_y, f32::max);
        assert!(min_x >= root_center_x - min_x);
        assert!(layout.width - max_x >= max_x - root_center_x);
        assert!(min_y >= root_center_y - min_y);
        assert!(layout.height - max_y >= max_y - root_center_y);
        let (offset_x, offset_y) = centered_map_offset(layout.width, layout.height, 1_280., 714.);
        assert_eq!(offset_x + layout.width / 2., 640.);
        assert_eq!(offset_y + layout.height / 2., 357.);
    }

    #[test]
    fn map_node_offsets_move_nodes_and_keep_edges_attached() {
        let timestamp = Utc::now().to_rfc3339();
        let session = CodingSession {
            id: "movable-session".into(),
            provider: CodingAgent::ClaudeCode,
            title: "Move this node".into(),
            project_name: "grove".into(),
            cwd: "/Users/mai/grove".into(),
            git_branch: Some("main".into()),
            slug: None,
            status: SessionStatus::Waiting,
            updated_at: timestamp,
            started_at: None,
            message_count: 1,
            last_prompt: None,
            last_tool: None,
            activities: vec![],
            subagents: vec![],
        };
        let base = build_mind_map_layout(std::slice::from_ref(&session));
        let base_session = base
            .nodes
            .iter()
            .find(|node| node.id == "session:movable-session")
            .unwrap();
        let mut offsets = HashMap::new();
        offsets.insert(
            "session:movable-session".into(),
            MapNodeOffset { x: 75, y: -30 },
        );

        let moved = build_mind_map_layout_with_offsets(&[session], &offsets);
        let moved_session = moved
            .nodes
            .iter()
            .find(|node| node.id == "session:movable-session")
            .unwrap();
        let edge = moved
            .edges
            .iter()
            .find(|edge| edge.to_id == "session:movable-session")
            .unwrap();

        assert_eq!(moved_session.x, base_session.x + 75.);
        assert_eq!(moved_session.y, base_session.y - 30.);
        assert_eq!(edge.to_y, moved_session.y + moved_session.height / 2.);
        assert!(edge.to_x == moved_session.x || edge.to_x == moved_session.x + moved_session.width);
    }

    #[test]
    fn dragged_nodes_stay_inside_the_map_bounds() {
        let layout = build_mind_map_layout(&[]);
        let grove = layout.nodes.iter().find(|node| node.id == "grove").unwrap();

        let offset = clamped_node_offset(
            grove,
            &layout,
            MapNodeOffset {
                x: -100_000,
                y: 100_000,
            },
        );

        assert_eq!(grove.x + offset.x as f32, 20.);
        assert_eq!(
            grove.y + offset.y as f32,
            layout.height - grove.height - 20.
        );
    }

    #[test]
    fn map_zoom_is_bounded_and_preserves_the_viewport_center() {
        assert_eq!(clamp_map_zoom(0.1), MAP_ZOOM_MIN);
        assert_eq!(clamp_map_zoom(2.0), MAP_ZOOM_MAX);

        let offset = zoomed_map_offset((-500., -60.), 1.0, 1.2, (2_000., 720.), (1_000., 600.));
        assert!((offset.0 + 700.).abs() < 0.001);
        assert!((offset.1 + 132.).abs() < 0.001);

        let anchored = zoomed_map_offset_around(
            (-500., -60.),
            1.0,
            1.2,
            (2_000., 720.),
            (1_000., 600.),
            (250., 150.),
        );
        assert!((anchored.0 + 650.).abs() < 0.001);
        assert!((anchored.1 + 102.).abs() < 0.001);

        let zoomed_out = zoomed_map_offset((-500., -60.), 1.0, 0.5, (2_000., 720.), (1_000., 600.));
        assert_eq!(zoomed_out, (0., 0.));

        assert_eq!(
            clamped_map_offset((100., -1_000.), (2_000., 800.), (1_000., 600.)),
            (0., -200.)
        );
        assert_eq!(
            clamped_map_offset((-20., -20.), (800., 500.), (1_000., 600.)),
            (0., 0.)
        );
    }

    #[test]
    fn large_agent_fans_collapse_into_one_cluster_node() {
        let timestamp = Utc::now().to_rfc3339();
        let subagents = (0..13)
            .map(|index| CodingSubagent {
                id: format!("agent-{index}"),
                parent_agent_id: None,
                agent_type: if index < 9 { "fork" } else { "general-purpose" }.into(),
                description: format!("Agent {index}"),
                status: SessionStatus::Idle,
                updated_at: timestamp.clone(),
                message_count: 1,
                last_tool: Some("Read".into()),
                spawn_depth: 1,
            })
            .collect::<Vec<_>>();
        let session = CodingSession {
            id: "clustered-session".into(),
            provider: CodingAgent::ClaudeCode,
            title: "Many agents".into(),
            project_name: "grove".into(),
            cwd: "/Users/mai/grove".into(),
            git_branch: Some("main".into()),
            slug: None,
            status: SessionStatus::Idle,
            updated_at: timestamp,
            started_at: None,
            message_count: 13,
            last_prompt: None,
            last_tool: Some("Agent".into()),
            activities: vec![],
            subagents: subagents.clone(),
        };

        let layout = build_mind_map_layout(&[session]);

        assert_eq!(layout.nodes.len(), 3);
        assert_eq!(layout.edges.len(), 2);
        assert!(
            layout
                .nodes
                .iter()
                .any(|node| matches!(node.node, MapNode::AgentCluster { .. }))
        );
        assert_eq!(
            subagent_type_summary(&subagents),
            "fork × 9  ·  general-purpose × 4"
        );
    }

    #[test]
    fn expanded_agent_cluster_keeps_one_node_and_adds_group_content_below_toggle() {
        let timestamp = Utc::now().to_rfc3339();
        let subagents = (0..19)
            .map(|index| CodingSubagent {
                id: format!("agent-{index}"),
                parent_agent_id: None,
                agent_type: if index < 16 {
                    "general-purpose"
                } else if index < 18 {
                    "Explore"
                } else {
                    "Plan"
                }
                .into(),
                description: format!("Agent {index}"),
                status: SessionStatus::Idle,
                updated_at: timestamp.clone(),
                message_count: 1,
                last_tool: Some("Read".into()),
                spawn_depth: 1,
            })
            .collect::<Vec<_>>();
        let session = CodingSession {
            id: "expanded-session".into(),
            provider: CodingAgent::ClaudeCode,
            title: "Many grouped agents".into(),
            project_name: "grove".into(),
            cwd: "/Users/mai/grove".into(),
            git_branch: None,
            slug: None,
            status: SessionStatus::Idle,
            updated_at: timestamp,
            started_at: None,
            message_count: 19,
            last_prompt: None,
            last_tool: Some("Agent".into()),
            activities: vec![],
            subagents: subagents.clone(),
        };
        let mut expanded = HashSet::new();
        expanded.insert(session.id.clone());

        let layout = build_mind_map_layout_with_expanded(&[session], &expanded);
        let groups = grouped_subagents(&subagents);

        assert_eq!(layout.nodes.len(), 3);
        assert_eq!(layout.edges.len(), 2);
        assert!(layout.width > 3_100.);
        let cluster = layout
            .nodes
            .iter()
            .find(|node| matches!(node.node, MapNode::AgentCluster { .. }))
            .unwrap();
        assert_eq!(
            (cluster.width, cluster.height),
            expanded_agent_cluster_size(&subagents)
        );
        let cluster_edge = layout
            .edges
            .iter()
            .find(|edge| edge.to_id == cluster.id)
            .unwrap();
        assert_eq!(cluster_edge.to_y, cluster.y + MAP_AGENT_CLUSTER_HEIGHT / 2.);
        assert_eq!(agent_tray_column_count(subagents.len()), 2);
        assert!(cluster.height > 800.);
        assert_eq!(groups[0].0, "general-purpose");
        assert_eq!(groups[0].1.len(), 16);
        assert_eq!(groups[1].0, "Explore");
        assert_eq!(groups[2].0, "Plan");
    }

    #[test]
    fn very_large_inline_agent_groups_use_four_columns_and_show_all_rows() {
        let subagents = (0..81)
            .map(|index| CodingSubagent {
                id: format!("agent-{index}"),
                parent_agent_id: None,
                agent_type: if index < 48 {
                    "fork"
                } else {
                    "general-purpose"
                }
                .into(),
                description: format!("Agent {index}"),
                status: SessionStatus::Idle,
                updated_at: Utc::now().to_rfc3339(),
                message_count: 1,
                last_tool: Some("Bash".into()),
                spawn_depth: 1,
            })
            .collect::<Vec<_>>();

        let (width, height) = expanded_agent_cluster_size(&subagents);

        assert_eq!(agent_tray_column_count(subagents.len()), 4);
        assert!(width > 900.);
        assert!(height > 1_400.);
    }
}
