use std::cell::RefCell;
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
         .docx-text { background-color: #ffffff; color: #1a1a1a; }\n\
         .docx-toolbar { background-color: #2c2c2c; padding: 4px 8px; }\n\
         .docx-toolbar button, .docx-toolbar menubutton > button { background-color: #3a3a3a; background-image: none; color: #f2f2f2; border-color: #4a4a4a; box-shadow: none; }\n\
         .docx-toolbar button:hover, .docx-toolbar menubutton > button:hover { background-color: #484848; }\n\
         .docx-toolbar button:checked, .docx-toolbar menubutton > button:checked { background-color: #555555; }\n\
         .docx-scroll { background-color: #2c2c2c; }\n\
         .docx-sidebar-list { background-color: #242424; }\n\
         .docx-sidebar-list button { background: transparent; color: #f2f2f2; padding: 2px 4px; }\n\
         .docx-outline-item { color: #f2f2f2; }\n\
         .docx-outline-level-2 { padding-left: 12px; }\n\
         .docx-outline-level-3 { padding-left: 24px; }\n\
         .docx-outline-level-4 { padding-left: 36px; }\n\
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

struct WindowState {
    window: gtk::ApplicationWindow,
    stack: gtk::Stack,
    sidebar: gtk::ScrolledWindow,
    outline_box: gtk::Box,
    empty_label: gtk::Label,
    text_view: gtk::TextView,
    page_box: gtk::Box,
    search_bar: gtk::SearchBar,
    search_entry: gtk::SearchEntry,
    search_button: gtk::ToggleButton,
    banner: gtk::Revealer,
    banner_label: gtk::Label,
    status: gtk::Label,
    zoom_label: gtk::MenuButton,
    path: Option<PathBuf>,
    document: Option<Arc<Document>>,
    zoom: f64,
    generation: u64,
    cancel: Option<Arc<AtomicBool>>,
    pending_scroll: Option<i32>,
    search_matches: Vec<(i32, i32)>,
    search_index: usize,
    render_output: Rc<RefCell<RenderOutput>>,
    open_dialog: Option<gtk::FileChooserDialog>,
    theme: AppTheme,
}

fn open_window(app: &gtk::Application, path: Option<PathBuf>) -> Rc<RefCell<WindowState>> {
    let window = gtk::ApplicationWindow::builder()
        .application(app)
        .default_width(864)
        .default_height(1000)
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
    let zoom_box = gtk::Box::new(gtk::Orientation::Vertical, 4);
    zoom_box.set_margin_top(6);
    zoom_box.set_margin_bottom(6);
    zoom_box.set_margin_start(6);
    zoom_box.set_margin_end(6);
    let zoom_in = gtk::Button::with_label("Zoom in");
    let zoom_out = gtk::Button::with_label("Zoom out");
    let zoom_reset = gtk::Button::with_label("Reset zoom");
    zoom_box.append(&zoom_in);
    zoom_box.append(&zoom_out);
    zoom_box.append(&zoom_reset);
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
    sidebar.set_size_request(220, -1);
    sidebar.set_vexpand(true);
    sidebar.set_visible(false);
    let outline_box = gtk::Box::new(gtk::Orientation::Vertical, 0);
    outline_box.add_css_class("docx-sidebar-list");
    sidebar.set_child(Some(&outline_box));

    let content_box = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    content_box.set_vexpand(true);
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
    page_box.set_halign(gtk::Align::Center);
    page_box.set_vexpand(true);
    page_box.set_hexpand(false);
    page_box.set_width_request(816);
    let view_scroll = gtk::ScrolledWindow::new();
    view_scroll.set_vexpand(true);
    view_scroll.set_hexpand(true);
    view_scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    view_scroll.add_css_class("docx-scroll");
    let page_area = gtk::Box::new(gtk::Orientation::Vertical, 0);
    page_area.set_halign(gtk::Align::Fill);
    page_area.set_vexpand(true);
    page_area.set_hexpand(true);
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
    page_box.set_vexpand(true);
    page_box.append(&text_view);
    page_area.append(&page_box);
    view_scroll.set_child(Some(&page_area));
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
    body.append(&status_bar);

    let theme = theme::detect(&window);
    let state = Rc::new(RefCell::new(WindowState {
        window: window.clone(),
        stack,
        sidebar: sidebar.clone(),
        outline_box: outline_box.clone(),
        empty_label,
        text_view: text_view.clone(),
        page_box: page_box.clone(),
        search_bar: search_bar.clone(),
        search_entry: search_entry.clone(),
        search_button: search_button.clone(),
        banner,
        banner_label,
        status,
        zoom_label: zoom_button.clone(),
        path: None,
        document: None,
        zoom: 1.0,
        generation: 0,
        cancel: None,
        pending_scroll: None,
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
        scroll.connect_scroll(move |controller, _dx, dy| {
            if controller
                .current_event_state()
                .contains(gdk::ModifierType::CONTROL_MASK)
            {
                if dy < 0.0 {
                    set_zoom(&scroll_state, 0.1);
                } else if dy > 0.0 {
                    set_zoom(&scroll_state, -0.1);
                }
                glib::Propagation::Stop
            } else {
                glib::Propagation::Proceed
            }
        });
        text_view.add_controller(scroll);
    }
    {
        let resize_state = state.clone();
        glib::timeout_add_local(Duration::from_millis(200), move || {
            if let Ok(state) = resize_state.try_borrow() {
                if !state.window.is_visible() {
                    return glib::ControlFlow::Break;
                }
                update_page_width(&state);
            }
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
    {
        let state = state.clone();
        zoom_in.connect_clicked(move |_| set_zoom(&state, 0.25));
    }
    {
        let state = state.clone();
        zoom_out.connect_clicked(move |_| set_zoom(&state, -0.25));
    }
    {
        let state = state.clone();
        zoom_reset.connect_clicked(move |_| set_zoom(&state, 0.0));
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
        let Some(iter) = state.text_view.iter_at_location(x as i32, y as i32) else {
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
        let Some(iter) = state.text_view.iter_at_location(x as i32, y as i32) else {
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
        state.pending_scroll = None;
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
    let (view, zoom, theme, output) = {
        let Ok(mut state) = state.try_borrow_mut() else {
            return;
        };
        if state.generation != generation {
            return;
        }
        state.document = Some(document.clone());
        state.stack.set_visible_child_name("text");
        state.status.set_text("Rendering…");
        state.banner.set_reveal_child(false);
        state.search_matches.clear();
        state.search_index = 0;
        state
            .text_view
            .set_buffer(Some(&gtk::TextBuffer::new(None)));
        output_reset(&state.render_output);
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
        (
            state.text_view.clone(),
            state.zoom,
            state.theme.clone(),
            state.render_output.clone(),
        )
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
    let cancel = render::render(view, document, zoom, theme, output, move || {
        if std::env::var_os("DOCXLITE_SMOKE_LOG").is_some() {
            eprintln!("smoke on_done");
        }
        if let Ok(state) = done_state.try_borrow_mut()
            && state.generation == generation
        {
            state.status.set_text("");
        }
        populate_outline(&done_state);
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

fn start_render(state: &Rc<RefCell<WindowState>>) {
    let (document, view, zoom, theme, output, generation) = {
        let Ok(mut state) = state.try_borrow_mut() else {
            return;
        };
        let Some(document) = state.document.clone() else {
            return;
        };
        if let Some(cancel) = state.cancel.take() {
            cancel.store(true, Ordering::Relaxed);
        }
        state.search_matches.clear();
        state.search_index = 0;
        state
            .text_view
            .set_buffer(Some(&gtk::TextBuffer::new(None)));
        output_reset(&state.render_output);
        state.status.set_text("Rendering…");
        (
            document,
            state.text_view.clone(),
            state.zoom,
            state.theme.clone(),
            state.render_output.clone(),
            state.generation,
        )
    };
    let done_state = state.clone();
    let cancel = render::render(view, document, zoom, theme, output, move || {
        if std::env::var_os("DOCXLITE_SMOKE_LOG").is_some() {
            eprintln!("smoke on_done");
        }
        if let Ok(mut state) = done_state.try_borrow_mut()
            && state.generation == generation
        {
            state.status.set_text("");
            restore_pending_scroll(&mut state);
        }
        populate_outline(&done_state);
        if std::env::var_os("DOCXLITE_SMOKE_OUTLINE").is_some()
            && let Ok(state) = done_state.try_borrow()
        {
            state.sidebar.set_visible(true);
        }
    });
    if let Ok(mut state) = state.try_borrow_mut() {
        state.cancel = Some(cancel);
    }
}

fn output_reset(output: &Rc<RefCell<RenderOutput>>) {
    if let Ok(mut output) = output.try_borrow_mut() {
        output.link_targets.clear();
        output.bookmarks.clear();
        output.outline.clear();
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
            .map(|entry| (entry.title.clone(), entry.level, entry.mark.clone()))
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
    for (title, level, mark) in entries {
        let label = gtk::Label::new(Some(&title));
        label.set_xalign(0.0);
        label.set_wrap(true);
        label.set_max_width_chars(28);
        let button = gtk::Button::new();
        button.set_child(Some(&label));
        button.set_halign(gtk::Align::Start);
        button.set_hexpand(true);
        button.add_css_class("docx-outline-item");
        button.add_css_class(&format!("docx-outline-level-{level}"));
        let view = state.text_view.clone();
        button.connect_clicked(move |_| {
            view.scroll_to_mark(&mark, 0.1, true, 0.0, 0.0);
        });
        state.outline_box.append(&button);
    }
}

fn restore_pending_scroll(state: &mut WindowState) {
    let Some(offset) = state.pending_scroll.take() else {
        return;
    };
    let buffer = state.text_view.buffer();
    let mut iter = buffer.iter_at_offset(offset);
    state
        .text_view
        .scroll_to_iter(&mut iter, 0.1, true, 0.0, 0.35);
}

fn set_zoom(state: &Rc<RefCell<WindowState>>, delta: f64) {
    let changed = {
        let Ok(mut state) = state.try_borrow_mut() else {
            return;
        };
        let next = if delta == 0.0 {
            1.0
        } else {
            (state.zoom + delta).clamp(0.5, 3.0)
        };
        if (next - state.zoom).abs() < f64::EPSILON {
            return;
        }
        state.zoom = next;
        state
            .zoom_label
            .set_label(&format!("{}%", (next * 100.0).round() as i32));
        state.document.is_some()
    };
    if changed {
        start_render(state);
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

fn update_page_width(state: &WindowState) {
    let window_width = state.window.width();
    let content_width = if state.stack.width() > 100 {
        state.stack.width()
    } else {
        window_width
    };
    if content_width <= 0 {
        return;
    }
    let available = content_width.saturating_sub(48).max(200);
    let desired = ((available as f64).min(816.0 * state.zoom)).round() as i32;
    let desired_height = if state.stack.height() > 100 {
        state.stack.height()
    } else {
        -1
    };
    if state.page_box.width_request() != desired
        || (desired_height > 0 && state.page_box.height_request() != desired_height)
    {
        state.page_box.set_size_request(desired, desired_height);
    }
}

fn file_name(path: &std::path::Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Document".to_string())
}
