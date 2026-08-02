mod app;
mod models;
mod preferences;
mod scanner;
mod text_input;

use app::{Grove, session_roots};
use gpui::{
    App, AppContext, Application, Bounds, Menu, MenuItem, SystemMenuType, TitlebarOptions,
    WindowBounds, WindowOptions, actions, px, size,
};
use std::time::SystemTime;

actions!(grove, [Quit]);

fn initial_scan() -> models::SessionScan {
    let roots = session_roots();
    scanner::scan_sessions_at(&roots, SystemTime::now()).unwrap_or_else(|error| {
        models::SessionScan {
            sessions: vec![],
            scanned_at: chrono::Utc::now().to_rfc3339(),
            source_roots: vec![
                roots.claude.to_string_lossy().into_owned(),
                roots.codex.to_string_lossy().into_owned(),
            ],
            skipped_files: 1,
            warnings: vec![error.to_string()],
        }
    })
}

fn open_grove_window(scan: models::SessionScan, cx: &mut App) {
    let bounds = Bounds::centered(None, size(px(1280.), px(820.)), cx);
    let window = cx
        .open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                window_min_size: Some(size(px(880.), px(620.))),
                titlebar: Some(TitlebarOptions {
                    title: Some("Grove".into()),
                    appears_transparent: true,
                    traffic_light_position: Some(gpui::point(px(16.), px(17.))),
                }),
                app_id: Some("app.grove.sessions".into()),
                ..Default::default()
            },
            |_, cx| cx.new(|cx| Grove::new(scan, cx)),
        )
        .expect("open Grove window");

    window
        .update(cx, |_, window, _| {
            window.set_window_title("Grove");
        })
        .ok();
    cx.activate(true);
}

fn main() {
    let application = Application::new();
    application.on_reopen(|cx| {
        if cx.windows().is_empty() {
            open_grove_window(initial_scan(), cx);
        }
    });

    let scan = initial_scan();
    application.run(move |cx: &mut App| {
        text_input::register_key_bindings(cx);
        cx.on_action(|_: &Quit, cx| cx.quit());
        cx.set_menus(vec![Menu {
            name: "Grove".into(),
            items: vec![
                MenuItem::os_submenu("Services", SystemMenuType::Services),
                MenuItem::separator(),
                MenuItem::action("Quit Grove", Quit),
            ],
        }]);
        open_grove_window(scan, cx);
    });
}
