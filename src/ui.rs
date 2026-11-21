use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, TryRecvError};
use std::time::Duration;

use gtk::prelude::*;
use gtk::{gdk, gio, glib};

use crate::error::LoadError;
use crate::model::{Document, LinkTarget};
use crate::render::{self, RenderOutput};
use crate::theme::{self, AppTheme};
use crate::zoom_view::ZoomView;

pub fn run(files: Vec<String>) -> glib::ExitCode {
    let mut flags = gio::ApplicationFlags::HANDLES_OPEN;
    if std::env::var_os("DOCXLITE_NON_UNIQUE").is_some() {
        flags |= gio::ApplicationFlags::NON_UNIQUE;
    }
    let app = gtk::Application::builder()
        .application_id("io.github.docxlite.DocxLite")
        .flags(flags)
        .build();
    app.connect_startup(|_| {
        force_light_theme();
        install_css();
    });
    let has_files = !files.is_empty();
    app.connect_activate(move |app| {
        if !has_files && app.active_window().is_none() {
            open_window(app, None);
        }
    });
    app.connect_open(|app, files, _| {
        if files.is_empty() {
            open_window(app, None);
        } else {
            for file in files {
                open_window(app, file.path());
            }
        }
    });
    app.set_accels_for_action("win.open", &["<Control>o"]);
    app.set_accels_for_action("win.search", &["<Control>f"]);
    app.set_accels_for_action("win.zoom-in", &["<Control>equal", "<Control>plus"]);
    app.set_accels_for_action("win.zoom-out", &["<Control>minus"]);
    app.set_accels_for_action("win.zoom-reset", &["<Control>0"]);
    app.set_accels_for_action("win.reload", &["F5"]);
    app.set_accels_for_action("win.close", &["<Control>w"]);
    app.set_accels_for_action("win.quit", &["<Control>q"]);
    let mut args = vec!["docxlite".to_string()];
    args.extend(files);
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    app.run_with_args(&refs)
}

fn force_light_theme() {
    if let Some(settings) = gtk::Settings::default() {
        settings.set_gtk_application_prefer_dark_theme(false);
        settings.set_gtk_theme_name(Some("Adwaita"));
    }
}

fn install_css() {
    let Some(display) = gdk::Display::default() else {
        return;
    };
    let provider = gtk::CssProvider::new();
    provider.load_from_data(
        ".docx-banner { padding: 6px 8px; background-color: alpha(@theme_selected_bg_color, 0.18); }\n\
         .docx-status { padding: 4px 10px; font-size: 0.9em; opacity: 0.75; }\n\
         .docx-empty-title { font-size: 1.2em; }\n\
         .docx-body { background-color: #2c2c2c; color: #f2f2f2; }\n\
         .docx-page { background-color: #ffffff; }\n\
         .docx-text { background-color: transparent; color: #1a1a1a; }\n\
         .docx-toolbar { background-color: #2c2c2c; padding: 4px 8px; }\n\
         .docx-toolbar button, .docx-toolbar menubutton > button { background-color: #3a3a3a; background-image: none; color: #f2f2f2; border-color: #4a4a4a; box-shadow: none; }\n\
         .docx-toolbar button:hover, .docx-toolbar menubutton > button:hover { background-color: #484848; }\n\
         .docx-toolbar button:checked, .docx-toolbar menubutton > button:checked { background-color: #555555; }\n\
         .docx-scroll { background-color: #2c2c2c; }\n\
         .docx-sidebar-list { background-color: #242424; }\n\
         .docx-sidebar-list button { background: none; background-image: none; border: none; box-shadow: none; color: #f2f2f2; padding: 1px 6px; min-height: 0; font-size: 0.9em; }\n\
         .docx-sidebar-list button:hover { background-color: #333333; }\n\
         .docx-outline-item { color: #f2f2f2; }\n\
         .docx-outline-level-2 { padding-left: 10px; }\n\
         .docx-outline-level-3 { padding-left: 18px; }\n\
         .docx-outline-level-4 { padding-left: 26px; }\n\
         .docx-outline-empty { color: #d0d0d0; padding: 8px; }\n\
         .docx-statusbar { background-color: #2c2c2c; padding: 4px 8px; }\n\
         .docx-statusbar .docx-status { padding: 0; }\n\
         .docx-status { color: #d0d0d0; }\n\
         .docx-banner { color: #f2f2f2; }\n\
         window { background-color: #2c2c2c; color: #f2f2f2; }\n\
         headerbar { background-color: #2c2c2c; color: #f2f2f2; }\n",
    );
    gtk::style_context_add_provider_for_display(
        &display,
        &provider,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
}

const PAGE_WIDTH: i32 = 816;
const SIDEBAR_WIDTH: i32 = 160;
const PAGE_HEIGHT: i32 = 1056;
const ZOOM_LEVELS: [f64; 10] = [0.5, 0.7, 0.85, 1.0, 1.25, 1.5, 1.75, 2.0, 3.0, 4.0];

#[derive(Clone, Copy, PartialEq)]
enum ZoomMode {
    FitPage,
    FitWidth,
    Automatic,
    Level(f64),
}

fn build_zoom_menu(container: &gtk::Box) -> Rc<Vec<(ZoomMode, gtk::CheckButton)>> {
    let mut items: Vec<(ZoomMode, gtk::CheckButton)> = Vec::new();
    let mut add = |container: &gtk::Box, mode: ZoomMode, text: &str| {
        let button = gtk::CheckButton::with_label(text);
        if let Some((_, first)) = items.first() {
            button.set_group(Some(first));
        }
        container.append(&button);
        items.push((mode, button));
    };
    add(container, ZoomMode::FitPage, "Fit Page");
    add(container, ZoomMode::FitWidth, "Fit Width");
    add(container, ZoomMode::Automatic, "Automatic");
    container.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
    for level in ZOOM_LEVELS {
        add(container, ZoomMode::Level(level), &format_zoom(level));
    }
    for (mode, button) in &items {
        if *mode == ZoomMode::Automatic {
            button.set_active(true);
        }
    }
    Rc::new(items)
}

fn format_zoom(scale: f64) -> String {
    let percent = scale * 100.0;
    if (percent - percent.round()).abs() < 0.05 {
        format!("{}%", percent.round() as i32)
    } else {
        format!("{percent:.1}%")
    }
}

fn zoom_scale_for(state: &WindowState, mode: ZoomMode) -> f64 {
    let width = f64::from(state.view_scroll.width().max(1));
    let height = f64::from(state.view_scroll.height().max(1));
    let width_scale = width / f64::from(PAGE_WIDTH);
    let page_scale = (width / f64::from(PAGE_WIDTH)).min(height / f64::from(PAGE_HEIGHT));
    let scale = match mode {
        ZoomMode::Level(level) => level,
        ZoomMode::FitWidth => width_scale,
        ZoomMode::FitPage => page_scale,
        ZoomMode::Automatic => {
            if width_scale < 1.0 {
                width_scale
            } else {
                1.0
            }
        }
    };
    scale.clamp(0.25, 4.0)
}

fn apply_zoom(state: &Rc<RefCell<WindowState>>, mode: ZoomMode) {
    let Ok(mut state) = state.try_borrow_mut() else {
        return;
    };
    if state.zoom_mode == mode
        && state.zoom_items.iter().any(|(item, _)| *item == mode)
        && (state.zoom - zoom_scale_for(&state, mode)).abs() < 1e-6
    {
        return;
    }
    let scale = zoom_scale_for(&state, mode);
    let ratio = scale / state.zoom;
    state.zoom = scale;
    state.zoom_mode = mode;
    state.zoom_label.set_label(&format_zoom(scale));
    state.zoom_view.set_scale(scale);
    for (item_mode, button) in state.zoom_items.iter() {
        button.set_active(*item_mode == mode);
    }
    if (ratio - 1.0).abs() > f64::EPSILON {
        scale_scroll(&state.view_scroll.hadjustment(), ratio);
        scale_scroll(&state.view_scroll.vadjustment(), ratio);
    }
}

fn refresh_zoom(state: &Rc<RefCell<WindowState>>) {
    let mode = {
        let Ok(state) = state.try_borrow() else {
            return;
        };
        state.zoom_mode
    };
    apply_zoom(state, mode);
}

struct WindowState {
    window: gtk::ApplicationWindow,
    stack: gtk::Stack,
    sidebar: gtk::ScrolledWindow,
    outline_box: gtk::Box,
    empty_label: gtk::Label,
    text_view: gtk::TextView,
    view_scroll: gtk::ScrolledWindow,
    zoom_view: ZoomView,
    search_bar: gtk::SearchBar,
    search_entry: gtk::SearchEntry,
    search_button: gtk::ToggleButton,
    banner: gtk::Revealer,
    banner_label: gtk::Label,
    status: gtk::Label,
    page_label: gtk::Label,
    zoom_label: gtk::MenuButton,
    zoom_mode: ZoomMode,
    zoom_items: Rc<Vec<(ZoomMode, gtk::CheckButton)>>,
    path: Option<PathBuf>,
    document: Option<Arc<Document>>,
    zoom: f64,
    generation: u64,
    cancel: Option<Arc<AtomicBool>>,
    search_matches: Vec<(i32, i32)>,
    search_index: usize,
    render_output: Rc<RefCell<RenderOutput>>,
    open_dialog: Option<gtk::FileChooserDialog>,
    theme: AppTheme,
}

fn default_window_size() -> (i32, i32) {
    let monitor = gdk::Display::default().and_then(|display| {
        display
            .monitors()
            .item(0)
            .and_then(|obj| obj.downcast::<gdk::Monitor>().ok())
    });
    let Some(geometry) = monitor.map(|monitor| monitor.geometry()) else {
        return (620, 520);
    };
    let factor = 0.2_f64.sqrt();
    let width = (f64::from(geometry.width()) * factor).clamp(420.0, 1000.0);
    let height = (f64::from(geometry.height()) * factor).clamp(420.0, 900.0);
    (width as i32, height as i32)
}

fn open_window(app: &gtk::Application, path: Option<PathBuf>) -> Rc<RefCell<WindowState>> {
    let (default_width, default_height) = default_window_size();
    let window = gtk::ApplicationWindow::builder()
        .application(app)
        .default_width(default_width)
        .default_height(default_height)
        .build();
    window.set_title(Some("DocxLite"));

    let body = gtk::Box::new(gtk::Orientation::Vertical, 0);
    body.add_css_class("docx-body");
    window.set_child(Some(&body));

    let viewer_bar = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    viewer_bar.add_css_class("docx-toolbar");
    let sidebar_button = gtk::ToggleButton::new();
    sidebar_button.set_icon_name("view-list-symbolic");
    sidebar_button.set_tooltip_text(Some("Document outline"));
    let viewer_spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    viewer_spacer.set_hexpand(true);
    let search_button = gtk::ToggleButton::new();
    search_button.set_icon_name("edit-find-symbolic");
    search_button.set_tooltip_text(Some("Search (Ctrl+F)"));
    let zoom_button = gtk::MenuButton::new();
    zoom_button.set_label("100%");
    zoom_button.set_tooltip_text(Some("Zoom"));
    let zoom_popover = gtk::Popover::new();
    let zoom_box = gtk::Box::new(gtk::Orientation::Vertical, 2);
    zoom_box.set_margin_top(6);
    zoom_box.set_margin_bottom(6);
    zoom_box.set_margin_start(6);
    zoom_box.set_margin_end(6);
    let zoom_items = build_zoom_menu(&zoom_box);
    zoom_popover.set_child(Some(&zoom_box));
    zoom_button.set_popover(Some(&zoom_popover));
    let menu_button = gtk::MenuButton::new();
    menu_button.set_icon_name("open-menu-symbolic");
    menu_button.set_tooltip_text(Some("Menu"));
    let menu_popover = gtk::Popover::new();
    let menu_box = gtk::Box::new(gtk::Orientation::Vertical, 4);
    menu_box.set_margin_top(6);
    menu_box.set_margin_bottom(6);
    menu_box.set_margin_start(6);
    menu_box.set_margin_end(6);
    let menu_open = gtk::Button::with_label("Open…");
    let menu_reload = gtk::Button::with_label("Reload");
    let menu_fullscreen = gtk::Button::with_label("Fullscreen");
    menu_box.append(&menu_open);
    menu_box.append(&menu_reload);
    menu_box.append(&menu_fullscreen);
    menu_popover.set_child(Some(&menu_box));
    menu_button.set_popover(Some(&menu_popover));
    let fullscreen_button = gtk::Button::from_icon_name("view-fullscreen-symbolic");
    fullscreen_button.set_tooltip_text(Some("Fullscreen"));
    viewer_bar.append(&sidebar_button);
    viewer_bar.append(&viewer_spacer);
    viewer_bar.append(&zoom_button);
    viewer_bar.append(&search_button);
    viewer_bar.append(&menu_button);
    viewer_bar.append(&fullscreen_button);
    body.append(&viewer_bar);

    let banner = gtk::Revealer::new();
    banner.set_transition_type(gtk::RevealerTransitionType::SlideDown);
    let banner_box = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    banner_box.add_css_class("docx-banner");
    let banner_label = gtk::Label::new(None);
    banner_label.set_wrap(true);
    banner_label.set_xalign(0.0);
    banner_label.set_hexpand(true);
    let banner_close = gtk::Button::from_icon_name("window-close-symbolic");
    banner_close.set_tooltip_text(Some("Dismiss"));
    banner_box.append(&banner_label);
    banner_box.append(&banner_close);
    banner.set_child(Some(&banner_box));
    body.append(&banner);

    let search_bar = gtk::SearchBar::new();
    let search_entry = gtk::SearchEntry::new();
    search_entry.set_placeholder_text(Some("Search document"));
    search_entry.set_hexpand(true);
    search_bar.set_child(Some(&search_entry));
    search_bar.connect_entry(&search_entry);
    search_bar.set_show_close_button(true);
    body.append(&search_bar);

    let stack = gtk::Stack::new();
    stack.set_vexpand(true);
    stack.set_hexpand(true);

    let sidebar = gtk::ScrolledWindow::new();
    sidebar.set_size_request(SIDEBAR_WIDTH, -1);
    sidebar.set_propagate_natural_width(false);
    sidebar.set_max_content_width(SIDEBAR_WIDTH);
    sidebar.set_min_content_width(SIDEBAR_WIDTH);
    sidebar.set_policy(gtk::PolicyType::Automatic, gtk::PolicyType::Automatic);
    sidebar.set_vexpand(true);
    sidebar.set_visible(false);
    let outline_box = gtk::Box::new(gtk::Orientation::Vertical, 0);
    outline_box.add_css_class("docx-sidebar-list");
    outline_box.set_halign(gtk::Align::Fill);
    sidebar.set_child(Some(&outline_box));

    let content_box = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    content_box.set_vexpand(true);
    sidebar.set_hexpand(false);
    content_box.append(&sidebar);
    content_box.append(&stack);
    body.append(&content_box);

    let empty_box = gtk::Box::new(gtk::Orientation::Vertical, 12);
    empty_box.set_valign(gtk::Align::Center);
    empty_box.set_halign(gtk::Align::Center);
    let empty_label = gtk::Label::new(Some("Open a .docx file or drop one here"));
    empty_label.add_css_class("docx-empty-title");
    empty_label.set_wrap(true);
    empty_label.set_justify(gtk::Justification::Center);
    let empty_open = gtk::Button::with_label("Open…");
    empty_box.append(&empty_label);
    empty_box.append(&empty_open);
    stack.add_named(&empty_box, Some("empty"));

    let text_box = gtk::Box::new(gtk::Orientation::Vertical, 0);
    text_box.set_vexpand(true);
    let page_box = gtk::Box::new(gtk::Orientation::Vertical, 0);
    page_box.add_css_class("docx-page");
    page_box.set_vexpand(true);
    let text_view = gtk::TextView::new();
    text_view.set_editable(false);
    text_view.set_cursor_visible(false);
    text_view.set_wrap_mode(gtk::WrapMode::WordChar);
    text_view.set_left_margin(64);
    text_view.set_right_margin(64);
    text_view.set_top_margin(48);
    text_view.set_bottom_margin(48);
    text_view.set_vexpand(true);
    text_view.add_css_class("docx-text");
    page_box.append(&text_view);
    let zoom_view = ZoomView::new(&page_box, PAGE_WIDTH);
    let viewport = gtk::Viewport::new(None::<&gtk::Adjustment>, None::<&gtk::Adjustment>);
    viewport.set_hscroll_policy(gtk::ScrollablePolicy::Natural);
    viewport.set_vscroll_policy(gtk::ScrollablePolicy::Natural);
    viewport.set_child(Some(&zoom_view));
    let view_scroll = gtk::ScrolledWindow::new();
    view_scroll.set_vexpand(true);
    view_scroll.set_hexpand(true);
    view_scroll.set_policy(gtk::PolicyType::Automatic, gtk::PolicyType::Automatic);
    view_scroll.add_css_class("docx-scroll");
    view_scroll.set_child(Some(&viewport));
    text_box.append(&view_scroll);
    stack.add_named(&text_box, Some("text"));
    stack.set_visible_child_name("empty");

    let status_bar = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    status_bar.add_css_class("docx-statusbar");
    let status = gtk::Label::new(None);
    status.add_css_class("docx-status");
    status.set_xalign(0.0);
    status.set_hexpand(true);
    status.set_ellipsize(gtk::pango::EllipsizeMode::End);
    status_bar.append(&status);
    let page_label = gtk::Label::new(None);
    page_label.add_css_class("docx-status");
    page_label.set_ellipsize(gtk::pango::EllipsizeMode::End);
    status_bar.append(&page_label);
    body.append(&status_bar);

    let theme = theme::detect(&window);
    let state = Rc::new(RefCell::new(WindowState {
        window: window.clone(),
        stack,
        sidebar: sidebar.clone(),
        outline_box: outline_box.clone(),
        empty_label,
        text_view: text_view.clone(),
        view_scroll: view_scroll.clone(),
        zoom_view: zoom_view.clone(),
        search_bar: search_bar.clone(),
        search_entry: search_entry.clone(),
        search_button: search_button.clone(),
        banner,
        banner_label,
        status,
        page_label: page_label.clone(),
        zoom_label: zoom_button.clone(),
        zoom_mode: ZoomMode::Automatic,
        zoom_items,
        path: None,
        document: None,
        zoom: 1.0,
        generation: 0,
        cancel: None,
        search_matches: Vec::new(),
        search_index: 0,
        render_output: Rc::new(RefCell::new(RenderOutput::default())),
        open_dialog: None,
        theme,
    }));

    {
        let scroll_state = state.clone();
        let scroll = gtk::EventControllerScroll::new(gtk::EventControllerScrollFlags::VERTICAL);
        scroll.set_propagation_phase(gtk::PropagationPhase::Capture);
        let wheel_state = scroll_state.clone();
        let pending_zoom: Rc<Cell<f64>> = Rc::new(Cell::new(0.0));
        let zoom_scheduled: Rc<Cell<bool>> = Rc::new(Cell::new(false));
        scroll.connect_scroll(move |controller, _dx, dy| {
            if !controller
                .current_event_state()
                .contains(gdk::ModifierType::CONTROL_MASK)
            {
                return glib::Propagation::Proceed;
            }
            if dy != 0.0 {
                queue_wheel_zoom(
                    &wheel_state,
                    &pending_zoom,
                    &zoom_scheduled,
                    if dy < 0.0 { 0.1 } else { -0.1 },
                );
            }
            glib::Propagation::Stop
        });
        text_view.add_controller(scroll);
    }
    {
        let scroll_state = state.clone();
        let adjustment = state.borrow().view_scroll.vadjustment();
        adjustment.connect_value_changed(move |_| update_page_status(&scroll_state));
    }
    {
        let resize_state = state.clone();
        glib::timeout_add_local(Duration::from_millis(200), move || {
            match resize_state.try_borrow() {
                Ok(state) if !state.window.is_visible() => return glib::ControlFlow::Break,
                Ok(_) => {}
                Err(_) => return glib::ControlFlow::Continue,
            }
            refresh_zoom(&resize_state);
            update_page_status(&resize_state);
            glib::ControlFlow::Continue
        });
    }
    {
        let state = state.clone();
        banner_close.connect_clicked(move |_| {
            if let Ok(state) = state.try_borrow() {
                state.banner.set_reveal_child(false);
            }
        });
    }
    {
        let state = state.clone();
        empty_open.connect_clicked(move |_| show_open_dialog(&state));
    }
    {
        let state = state.clone();
        search_button.connect_toggled(move |button| {
            if let Ok(state) = state.try_borrow() {
                state.search_bar.set_search_mode(button.is_active());
                if button.is_active() {
                    state.search_entry.grab_focus();
                }
            }
        });
    }
    {
        let state = state.clone();
        search_entry.connect_search_changed(move |_| perform_search(&state));
    }
    {
        let state = state.clone();
        search_entry.connect_activate(move |_| next_match(&state, 1));
    }
    {
        let state = state.clone();
        let key = gtk::EventControllerKey::new();
        key.connect_key_pressed(move |_, keyval, _, modifiers| {
            if keyval == gdk::Key::Return || keyval == gdk::Key::KP_Enter {
                if modifiers.contains(gdk::ModifierType::SHIFT_MASK) {
                    next_match(&state, -1);
                } else {
                    next_match(&state, 1);
                }
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        });
        search_entry.add_controller(key);
    }
    let zoom_items = state.borrow().zoom_items.clone();
    for (mode, button) in zoom_items.iter().map(|(m, b)| (*m, b.clone())) {
        let state = state.clone();
        button.connect_toggled(move |button| {
            if button.is_active() {
                apply_zoom(&state, mode);
            }
        });
    }
    {
        let sidebar = sidebar.clone();
        sidebar_button.connect_toggled(move |button| {
            sidebar.set_visible(button.is_active());
        });
    }
    {
        let state = state.clone();
        menu_open.connect_clicked(move |_| show_open_dialog(&state));
    }
    {
        let state = state.clone();
        menu_reload.connect_clicked(move |_| reload(&state));
    }
    {
        let state = state.clone();
        menu_fullscreen.connect_clicked(move |_| toggle_fullscreen(&state));
    }
    {
        let state = state.clone();
        fullscreen_button.connect_clicked(move |_| toggle_fullscreen(&state));
    }

    install_actions(app, &window, &state);
    install_link_handlers(&state);
    install_drop_target(&window, &state);
    {
        let state = state.clone();
        window.connect_close_request(move |_| {
            if let Ok(mut state) = state.try_borrow_mut()
                && let Some(cancel) = state.cancel.take()
            {
                cancel.store(true, Ordering::Relaxed);
            }
            glib::Propagation::Proceed
        });
    }

    window.present();
    if let Some(path) = path {
        start_load(&state, path);
    }
    state
}

fn install_actions(
    app: &gtk::Application,
    window: &gtk::ApplicationWindow,
    state: &Rc<RefCell<WindowState>>,
) {
    let open_state = state.clone();
    let open_action = gio::SimpleAction::new("open", None);
    open_action.connect_activate(move |_, _| show_open_dialog(&open_state));
    window.add_action(&open_action);

    let search_state = state.clone();
    let search_action = gio::SimpleAction::new("search", None);
    search_action.connect_activate(move |_, _| {
        if let Ok(state) = search_state.try_borrow() {
            let active = !state.search_button.is_active();
            state.search_button.set_active(active);
            if active {
                state.search_entry.grab_focus();
            }
        }
    });
    window.add_action(&search_action);

    let zoom_in_state = state.clone();
    let zoom_in_action = gio::SimpleAction::new("zoom-in", None);
    zoom_in_action.connect_activate(move |_, _| set_zoom(&zoom_in_state, 0.25));
    window.add_action(&zoom_in_action);

    let zoom_out_state = state.clone();
    let zoom_out_action = gio::SimpleAction::new("zoom-out", None);
    zoom_out_action.connect_activate(move |_, _| set_zoom(&zoom_out_state, -0.25));
    window.add_action(&zoom_out_action);

    let zoom_reset_state = state.clone();
    let zoom_reset_action = gio::SimpleAction::new("zoom-reset", None);
    zoom_reset_action.connect_activate(move |_, _| set_zoom(&zoom_reset_state, 0.0));
    window.add_action(&zoom_reset_action);

    let reload_state = state.clone();
    let reload_action = gio::SimpleAction::new("reload", None);
    reload_action.connect_activate(move |_, _| reload(&reload_state));
    window.add_action(&reload_action);

    let close_window = window.clone();
    let close_action = gio::SimpleAction::new("close", None);
    close_action.connect_activate(move |_, _| close_window.close());
    window.add_action(&close_action);

    let app_clone = app.clone();
    let quit_action = gio::SimpleAction::new("quit", None);
    quit_action.connect_activate(move |_, _| app_clone.quit());
    window.add_action(&quit_action);
}

fn install_link_handlers(state: &Rc<RefCell<WindowState>>) {
    let motion_state = state.clone();
    let motion = gtk::EventControllerMotion::new();
    motion.connect_motion(move |_, x, y| {
        let Ok(state) = motion_state.try_borrow() else {
            return;
        };
        let Some(iter) = iter_at_widget_point(&state.text_view, x, y) else {
            state.status.set_text("");
            state.text_view.set_cursor_from_name(None);
            return;
        };
        if let Some(index) = link_index_at(&iter)
            && let Some(target) = state.render_output.borrow().link_targets.get(index)
        {
            state.status.set_text(&link_text(target));
            state.text_view.set_cursor_from_name(Some("pointer"));
            return;
        }
        state.status.set_text("");
        state.text_view.set_cursor_from_name(None);
    });
    let leave_state = state.clone();
    motion.connect_leave(move |_| {
        if let Ok(state) = leave_state.try_borrow() {
            state.status.set_text("");
            state.text_view.set_cursor_from_name(None);
        }
    });
    state.borrow().text_view.add_controller(motion);

    let click_state = state.clone();
    let click = gtk::GestureClick::new();
    click.set_button(1);
    click.connect_pressed(move |_, _, x, y| {
        let Ok(state) = click_state.try_borrow() else {
            return;
        };
        let Some(iter) = iter_at_widget_point(&state.text_view, x, y) else {
            return;
        };
        let Some(index) = link_index_at(&iter) else {
            return;
        };
        let target = state
            .render_output
            .borrow()
            .link_targets
            .get(index)
            .cloned();
        let Some(target) = target else {
            return;
        };
        match target {
            LinkTarget::External(url) => {
                if allowed_scheme(&url) {
                    if let Err(error) =
                        gio::AppInfo::launch_default_for_uri(&url, None::<&gio::AppLaunchContext>)
                    {
                        state
                            .status
                            .set_text(&format!("Could not open link: {error}"));
                    }
                } else {
                    state.status.set_text("Blocked unsafe link scheme");
                }
            }
            LinkTarget::Anchor(anchor) => {
                let mark = state.render_output.borrow().bookmarks.get(&anchor).cloned();
                if let Some(mark) = mark {
                    state.text_view.scroll_to_mark(&mark, 0.1, true, 0.0, 0.5);
                }
            }
        }
    });
    state.borrow().text_view.add_controller(click);
}

fn install_drop_target(window: &gtk::ApplicationWindow, state: &Rc<RefCell<WindowState>>) {
    let drop_state = state.clone();
    let drop = gtk::DropTarget::new(gdk::FileList::static_type(), gdk::DragAction::COPY);
    drop.connect_drop(move |_, value, _, _| {
        let Ok(list) = value.get::<gdk::FileList>() else {
            return false;
        };
        let files = list.files();
        let Some(file) = files.first() else {
            return false;
        };
        let Some(path) = file.path() else {
            return false;
        };
        start_load(&drop_state, path);
        true
    });
    window.add_controller(drop);
}

#[allow(deprecated)]
fn show_open_dialog(state: &Rc<RefCell<WindowState>>) {
    let window = {
        let Ok(state_ref) = state.try_borrow() else {
            return;
        };
        if let Some(dialog) = &state_ref.open_dialog {
            dialog.present();
            return;
        }
        state_ref.window.clone()
    };
    let dialog = gtk::FileChooserDialog::new(
        Some("Open Document"),
        Some(&window),
        gtk::FileChooserAction::Open,
        &[
            ("Cancel", gtk::ResponseType::Cancel),
            ("Open", gtk::ResponseType::Accept),
        ],
    );
    dialog.set_modal(true);
    dialog.set_default_response(gtk::ResponseType::Accept);
    let filter = gtk::FileFilter::new();
    filter.set_name(Some("Word documents"));
    filter.add_pattern("*.docx");
    filter.add_pattern("*.docm");
    filter.add_pattern("*.dotx");
    dialog.add_filter(&filter);
    let state_clone = state.clone();
    dialog.connect_response(move |dialog, response| {
        if response == gtk::ResponseType::Accept
            && let Some(path) = dialog.file().and_then(|file| file.path())
        {
            start_load(&state_clone, path);
        }
        if let Ok(mut state) = state_clone.try_borrow_mut() {
            state.open_dialog = None;
        }
        dialog.close();
    });
    if let Ok(mut state) = state.try_borrow_mut() {
        state.open_dialog = Some(dialog.clone());
    }
    dialog.present();
}

fn start_load(state: &Rc<RefCell<WindowState>>, path: PathBuf) {
    let generation = {
        let Ok(mut state) = state.try_borrow_mut() else {
            return;
        };
        state.generation = state.generation.wrapping_add(1);
        if let Some(cancel) = state.cancel.take() {
            cancel.store(true, Ordering::Relaxed);
        }
        state.path = Some(path.clone());
        state.document = None;
        state.search_matches.clear();
        state.search_index = 0;
        state.sidebar.set_visible(false);
        clear_outline_list(&state.outline_box);
        state.render_output.borrow_mut().link_targets.clear();
        state.render_output.borrow_mut().bookmarks.clear();
        state.empty_label.set_text("Loading…");
        state.stack.set_visible_child_name("empty");
        state.banner.set_reveal_child(false);
        state.status.set_text("");
        let name = file_name(&path);
        state.window.set_title(Some(&format!("{name} — DocxLite")));
        state.generation
    };

    let cancel = Arc::new(AtomicBool::new(false));
    if let Ok(mut state) = state.try_borrow_mut() {
        state.cancel = Some(cancel.clone());
    }
    let (sender, receiver) = mpsc::channel();
    let thread_path = path.clone();
    std::thread::spawn(move || {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            crate::load_document(&thread_path, cancel)
        }));
        let _ = sender.send(result);
    });
    let state = state.clone();
    glib::timeout_add_local(Duration::from_millis(16), move || {
        match receiver.try_recv() {
            Ok(Ok(Ok(document))) => {
                show_document(&state, generation, document);
                glib::ControlFlow::Break
            }
            Ok(Ok(Err(error))) => {
                show_error(&state, generation, error);
                glib::ControlFlow::Break
            }
            Ok(Err(_)) => {
                show_error(&state, generation, LoadError::Internal);
                glib::ControlFlow::Break
            }
            Err(TryRecvError::Empty) => glib::ControlFlow::Continue,
            Err(TryRecvError::Disconnected) => {
                show_error(&state, generation, LoadError::Internal);
                glib::ControlFlow::Break
            }
        }
    });
}

fn show_document(state: &Rc<RefCell<WindowState>>, generation: u64, document: Document) {
    let warnings = document.warning_count();
    let truncated = document.truncated;
    let document = Arc::new(document);
    let (view, theme) = {
        let Ok(mut state) = state.try_borrow_mut() else {
            return;
        };
        if state.generation != generation {
            return;
        }
        if let Some(cancel) = state.cancel.take() {
            cancel.store(true, Ordering::Relaxed);
        }
        state.document = Some(document.clone());
        state.stack.set_visible_child_name("text");
        state.status.set_text("Rendering…");
        state.banner.set_reveal_child(false);
        state.search_matches.clear();
        state.search_index = 0;
        let name = state
            .path
            .as_deref()
            .map(file_name)
            .unwrap_or_else(|| "Document".to_string());
        let title = if truncated {
            format!("{name} (partial) — DocxLite")
        } else {
            format!("{name} — DocxLite")
        };
        state.window.set_title(Some(&title));
        (state.text_view.clone(), state.theme.clone())
    };
    if warnings > 0 {
        show_banner(
            state,
            &format!(
                "Showing a partial view: {warnings} elements aren't supported or the file is very large."
            ),
        );
    }
    let done_state = state.clone();
    let cancel = render::render(view, document, theme, move |fresh| {
        if std::env::var_os("DOCXLITE_SMOKE_LOG").is_some() {
            eprintln!("smoke on_done");
        }
        if let Ok(state) = done_state.try_borrow_mut()
            && state.generation == generation
        {
            publish_output(&state.render_output, fresh);
            state.status.set_text("");
        }
        populate_outline(&done_state);
        update_page_status(&done_state);
        {
            static SPAM_STARTED: AtomicBool = AtomicBool::new(false);
            if !SPAM_STARTED.swap(true, Ordering::SeqCst) {
                start_wheel_stress(&done_state);
                start_zoom_spam(&done_state);
            }
        }
        if let Some(level) = std::env::var("DOCXLITE_SMOKE_ZOOM_LEVEL")
            .ok()
            .and_then(|scale| scale.parse::<f64>().ok())
        {
            let mode = match level {
                -1.0 => ZoomMode::FitPage,
                -2.0 => ZoomMode::FitWidth,
                -3.0 => ZoomMode::Automatic,
                value => ZoomMode::Level(value),
            };
            apply_zoom(&done_state, mode);
        }
        if std::env::var_os("DOCXLITE_SMOKE_ZOOM_MENU").is_some()
            && let Ok(state) = done_state.try_borrow()
        {
            state.zoom_label.popup();
        }
        if std::env::var_os("DOCXLITE_SMOKE_OUTLINE").is_some()
            && let Ok(state) = done_state.try_borrow()
        {
            state.sidebar.set_visible(true);
        }
        static ZOOMED: AtomicBool = AtomicBool::new(false);
        if std::env::var_os("DOCXLITE_SMOKE_ZOOM").is_some() && !ZOOMED.swap(true, Ordering::SeqCst)
        {
            set_zoom(&done_state, 0.25);
            return;
        }
        if start_zoom_steps(&done_state) {
            return;
        }
        if std::env::var_os("DOCXLITE_SMOKE_OPEN").is_some() {
            show_open_dialog(&done_state);
        }
        if let Ok(query) = std::env::var("DOCXLITE_SMOKE_SEARCH") {
            if let Ok(state) = done_state.try_borrow_mut() {
                state.search_entry.set_text(&query);
                state.search_button.set_active(true);
            }
            perform_search(&done_state);
            if std::env::var_os("DOCXLITE_SMOKE_LOG").is_some()
                && let Ok(state) = done_state.try_borrow()
            {
                eprintln!("smoke search status: {}", state.status.text());
            }
        }
        if std::env::var_os("DOCXLITE_SMOKE_EXIT").is_some() {
            std::process::exit(0);
        }
    });
    if let Ok(mut state) = state.try_borrow_mut() {
        state.cancel = Some(cancel);
        state.status.set_text("Rendering…");
    }
}

fn queue_wheel_zoom(
    state: &Rc<RefCell<WindowState>>,
    pending_zoom: &Rc<Cell<f64>>,
    zoom_scheduled: &Rc<Cell<bool>>,
    step: f64,
) {
    pending_zoom.set(pending_zoom.get() + step);
    if zoom_scheduled.replace(true) {
        return;
    }
    let state = state.clone();
    let pending = pending_zoom.clone();
    let scheduled = zoom_scheduled.clone();
    glib::timeout_add_local_once(Duration::from_millis(40), move || {
        scheduled.set(false);
        let delta = pending.replace(0.0);
        if delta != 0.0 {
            set_zoom(&state, delta);
        }
    });
}

fn start_wheel_stress(state: &Rc<RefCell<WindowState>>) {
    let Ok(count) = std::env::var("DOCXLITE_SMOKE_WHEEL_ZOOM") else {
        return;
    };
    let count = count.parse::<usize>().unwrap_or(0);
    if count == 0 {
        return;
    }
    let pending: Rc<Cell<f64>> = Rc::new(Cell::new(0.0));
    let scheduled: Rc<Cell<bool>> = Rc::new(Cell::new(false));
    let wheel_state = state.clone();
    let mut fired = 0usize;
    glib::timeout_add_local(Duration::from_millis(4), move || {
        if fired >= count {
            if std::env::var_os("DOCXLITE_SMOKE_EXIT").is_some() {
                std::process::exit(0);
            }
            return glib::ControlFlow::Break;
        }
        let step = if fired.is_multiple_of(2) { 0.1 } else { -0.1 };
        fired = fired.saturating_add(1);
        queue_wheel_zoom(&wheel_state, &pending, &scheduled, step);
        glib::ControlFlow::Continue
    });
}

fn start_zoom_steps(state: &Rc<RefCell<WindowState>>) -> bool {
    let Ok(steps) = std::env::var("DOCXLITE_SMOKE_ZOOM_STEPS") else {
        return false;
    };
    let limit = steps.parse::<usize>().unwrap_or(0);
    let pattern = std::env::var("DOCXLITE_SMOKE_ZOOM_PATTERN").unwrap_or_default();
    let steps_state = state.clone();
    let mut step = 0usize;
    glib::timeout_add_local(Duration::from_millis(40), move || {
        if step >= limit {
            if std::env::var_os("DOCXLITE_SMOKE_EXIT").is_some() {
                std::process::exit(0);
            }
            return glib::ControlFlow::Break;
        }
        let delta = match pattern.as_str() {
            "up" => 0.25,
            "down" => -0.25,
            "jitter" => match step % 3 {
                0 => 0.5,
                1 => -0.5,
                _ => 0.25,
            },
            _ => {
                if step.is_multiple_of(2) {
                    0.2
                } else {
                    -0.2
                }
            }
        };
        if std::env::var_os("DOCXLITE_SMOKE_LOG").is_some() {
            eprintln!("zoom step {step} delta {delta}");
        }
        set_zoom(&steps_state, delta);
        step = step.saturating_add(1);
        glib::ControlFlow::Continue
    });
    true
}

fn start_zoom_spam(state: &Rc<RefCell<WindowState>>) {
    let Ok(count) = std::env::var("DOCXLITE_SMOKE_ZOOM_SPAM") else {
        return;
    };
    let count = count.parse::<usize>().unwrap_or(0);
    if count == 0 {
        return;
    }
    let spam_state = state.clone();
    let mut fired = 0usize;
    glib::timeout_add_local(Duration::from_millis(15), move || {
        if fired >= count {
            if std::env::var_os("DOCXLITE_SMOKE_EXIT").is_some() {
                std::process::exit(0);
            }
            return glib::ControlFlow::Break;
        }
        let delta = if fired.is_multiple_of(2) { 0.25 } else { -0.25 };
        fired = fired.saturating_add(1);
        if fired.is_multiple_of(7) && std::env::var_os("DOCXLITE_SMOKE_SPAM_RELOAD").is_some() {
            reload(&spam_state);
        }
        if fired.is_multiple_of(5) && std::env::var_os("DOCXLITE_SMOKE_SPAM_SEARCH").is_some() {
            perform_search(&spam_state);
        }
        set_zoom(&spam_state, delta);
        glib::ControlFlow::Continue
    });
}

fn publish_output(target: &Rc<RefCell<RenderOutput>>, fresh: RenderOutput) {
    if let Ok(mut target) = target.try_borrow_mut() {
        *target = fresh;
    }
}

fn show_error(state: &Rc<RefCell<WindowState>>, generation: u64, error: LoadError) {
    let Ok(mut state) = state.try_borrow_mut() else {
        return;
    };
    if state.generation != generation {
        return;
    }
    state.document = None;
    state.stack.set_visible_child_name("empty");
    state.empty_label.set_text(error.message());
    state.status.set_text("");
    state.banner.set_reveal_child(false);
}

fn show_banner(state: &Rc<RefCell<WindowState>>, text: &str) {
    if let Ok(state) = state.try_borrow() {
        state.banner_label.set_text(text);
        state.banner.set_reveal_child(true);
    }
}

fn toggle_fullscreen(state: &Rc<RefCell<WindowState>>) {
    if let Ok(state) = state.try_borrow() {
        if state.window.is_fullscreen() {
            state.window.unfullscreen();
        } else {
            state.window.fullscreen();
        }
    }
}

fn clear_outline_list(list: &gtk::Box) {
    while let Some(child) = list.first_child() {
        list.remove(&child);
    }
}

fn populate_outline(state: &Rc<RefCell<WindowState>>) {
    let entries = {
        let Ok(state) = state.try_borrow() else {
            return;
        };
        let output = state.render_output.borrow();
        output
            .outline
            .iter()
            .map(|entry| (entry.title.clone(), entry.level, entry.offset))
            .collect::<Vec<_>>()
    };
    let Ok(state) = state.try_borrow() else {
        return;
    };
    clear_outline_list(&state.outline_box);
    if entries.is_empty() {
        let label = gtk::Label::new(Some("No headings"));
        label.add_css_class("docx-outline-empty");
        label.set_xalign(0.0);
        state.outline_box.append(&label);
        return;
    }
    for (title, level, offset) in entries {
        let label = gtk::Label::new(Some(&title));
        label.set_xalign(0.0);
        label.set_ellipsize(gtk::pango::EllipsizeMode::End);
        label.set_max_width_chars(16);
        let button = gtk::Button::new();
        button.set_child(Some(&label));
        button.set_halign(gtk::Align::Start);
        button.set_hexpand(true);
        button.add_css_class("docx-outline-item");
        button.add_css_class(&format!("docx-outline-level-{level}"));
        let view = state.text_view.clone();
        button.connect_clicked(move |_| {
            let buffer = view.buffer();
            let max = buffer.char_count().max(0);
            let iter = buffer.iter_at_offset(offset.clamp(0, max));
            view.scroll_to_iter(&mut iter.clone(), 0.1, true, 0.0, 0.0);
        });
        state.outline_box.append(&button);
    }
}

fn set_zoom(state: &Rc<RefCell<WindowState>>, delta: f64) {
    let Ok(state_ref) = state.try_borrow() else {
        return;
    };
    let current = state_ref.zoom;
    let level = if delta == 0.0 {
        1.0
    } else if delta > 0.0 {
        ZOOM_LEVELS
            .iter()
            .copied()
            .find(|level| *level > current + 1e-6)
            .or_else(|| ZOOM_LEVELS.last().copied())
            .unwrap_or(1.0)
    } else {
        ZOOM_LEVELS
            .iter()
            .copied()
            .rev()
            .find(|level| *level < current - 1e-6)
            .unwrap_or(0.5)
    };
    drop(state_ref);
    apply_zoom(state, ZoomMode::Level(level));
}

fn scale_scroll(adjustment: &gtk::Adjustment, ratio: f64) {
    if ratio <= 0.0 {
        return;
    }
    let max = (adjustment.upper() - adjustment.page_size()).max(adjustment.lower());
    let value = (adjustment.value() * ratio).clamp(adjustment.lower(), max);
    adjustment.set_value(value);
}

fn update_page_status(state: &Rc<RefCell<WindowState>>) {
    let (content_height, scale, offset, page_size) = {
        let Ok(state) = state.try_borrow() else {
            return;
        };
        let (_, content_height, _, _) = state
            .text_view
            .measure(gtk::Orientation::Vertical, PAGE_WIDTH - 128);
        let adjustment = state.view_scroll.vadjustment();
        (
            content_height.max(1),
            state.zoom,
            adjustment.value(),
            adjustment.page_size(),
        )
    };
    let page_px = f64::from(PAGE_HEIGHT);
    let total = (f64::from(content_height) / page_px).ceil().max(1.0) as i32;
    let scale = if scale > 0.0 { scale } else { 1.0 };
    let top = offset.max(0.0) / scale;
    let visible = page_size.max(0.0) / scale;
    let first = (top / page_px).floor() as i32 + 1;
    let last = ((top + visible) / page_px).ceil() as i32;
    let text = if !visible.is_finite() || visible <= 0.0 {
        format!("{total} pages")
    } else if first == last {
        format!("Page {first} of {total}")
    } else {
        format!("Pages {first}–{last} of {total}")
    };
    if let Ok(state) = state.try_borrow() {
        state.page_label.set_text(&text);
    }
}

fn reload(state: &Rc<RefCell<WindowState>>) {
    let path = state.try_borrow().ok().and_then(|state| state.path.clone());
    if let Some(path) = path {
        start_load(state, path);
    }
}

fn perform_search(state: &Rc<RefCell<WindowState>>) {
    let Ok(mut state) = state.try_borrow_mut() else {
        return;
    };
    let query = state.search_entry.text().to_string();
    let buffer = state.text_view.buffer();
    let match_tag = search_tag(
        &buffer,
        "search-match",
        gdk::RGBA::new(1.0, 0.86, 0.30, 0.65),
    );
    let current_tag = search_tag(
        &buffer,
        "search-current",
        gdk::RGBA::new(1.0, 0.55, 0.10, 0.90),
    );
    buffer.remove_tag(&match_tag, &buffer.start_iter(), &buffer.end_iter());
    buffer.remove_tag(&current_tag, &buffer.start_iter(), &buffer.end_iter());
    state.search_matches.clear();
    state.search_index = 0;
    if query.is_empty() {
        state.status.set_text("");
        return;
    }
    let flags = gtk::TextSearchFlags::CASE_INSENSITIVE | gtk::TextSearchFlags::VISIBLE_ONLY;
    let mut iter = buffer.start_iter();
    let limit = buffer.end_iter();
    while let Some((start, end)) = iter.forward_search(&query, flags, Some(&limit)) {
        if state.search_matches.len() >= 5_000 {
            break;
        }
        let start_offset = start.offset();
        let end_offset = end.offset();
        buffer.apply_tag(&match_tag, &start, &end);
        state.search_matches.push((start_offset, end_offset));
        iter = end;
    }
    if state.search_matches.is_empty() {
        state.status.set_text("No matches");
    } else {
        apply_current_match(&mut state);
    }
}

fn next_match(state: &Rc<RefCell<WindowState>>, direction: i32) {
    let Ok(mut state) = state.try_borrow_mut() else {
        return;
    };
    let count = state.search_matches.len();
    if count == 0 {
        return;
    }
    let current = state.search_index;
    let next = if direction >= 0 {
        current.saturating_add(1) % count
    } else if current == 0 {
        count.saturating_sub(1)
    } else {
        current.saturating_sub(1)
    };
    state.search_index = next;
    apply_current_match(&mut state);
}

fn apply_current_match(state: &mut WindowState) {
    let buffer = state.text_view.buffer();
    let Some((start_offset, end_offset)) = state.search_matches.get(state.search_index).copied()
    else {
        return;
    };
    let current_tag = search_tag(
        &buffer,
        "search-current",
        gdk::RGBA::new(1.0, 0.55, 0.10, 0.90),
    );
    buffer.remove_tag(&current_tag, &buffer.start_iter(), &buffer.end_iter());
    let start = buffer.iter_at_offset(start_offset);
    let end = buffer.iter_at_offset(end_offset);
    buffer.apply_tag(&current_tag, &start, &end);
    let mut scroll_start = buffer.iter_at_offset(start_offset);
    state
        .text_view
        .scroll_to_iter(&mut scroll_start, 0.1, true, 0.0, 0.5);
    state.status.set_text(&format!(
        "{} of {}",
        state.search_index.saturating_add(1),
        state.search_matches.len()
    ));
}

fn search_tag(buffer: &gtk::TextBuffer, name: &str, color: gdk::RGBA) -> gtk::TextTag {
    if let Some(tag) = buffer.tag_table().lookup(name) {
        return tag;
    }
    let tag = gtk::TextTag::new(Some(name));
    tag.set_background_rgba(Some(&color));
    buffer.tag_table().add(&tag);
    tag
}

fn iter_at_widget_point(view: &gtk::TextView, x: f64, y: f64) -> Option<gtk::TextIter> {
    let (x, y) = view.window_to_buffer_coords(
        gtk::TextWindowType::Widget,
        x.round() as i32,
        y.round() as i32,
    );
    view.iter_at_location(x, y)
}

fn link_index_at(iter: &gtk::TextIter) -> Option<usize> {
    iter.tags().iter().find_map(|tag| {
        tag.name().and_then(|name| {
            name.strip_prefix("link:")
                .and_then(|value| value.parse::<usize>().ok())
        })
    })
}

fn link_text(target: &LinkTarget) -> String {
    match target {
        LinkTarget::External(url) => url.clone(),
        LinkTarget::Anchor(anchor) => format!("#{anchor}"),
    }
}

fn allowed_scheme(url: &str) -> bool {
    url.split_once(':').is_some_and(|(scheme, _)| {
        matches!(
            scheme.to_ascii_lowercase().as_str(),
            "http" | "https" | "mailto"
        )
    })
}

fn file_name(path: &std::path::Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Document".to_string())
}
