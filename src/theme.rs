use gtk::gdk;
use gtk::prelude::*;

#[derive(Clone, Debug)]
pub struct AppTheme {
    pub dark: bool,
    pub foreground: gdk::RGBA,
    pub muted: gdk::RGBA,
    pub link: gdk::RGBA,
}

pub fn detect(_widget: &impl IsA<gtk::Widget>) -> AppTheme {
    AppTheme {
        dark: false,
        foreground: gdk::RGBA::new(0.08, 0.08, 0.08, 1.0),
        muted: gdk::RGBA::new(0.42, 0.42, 0.42, 1.0),
        link: gdk::RGBA::new(0.02, 0.38, 0.74, 1.0),
    }
}
