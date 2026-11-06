use gtk::gdk;
use gtk::prelude::*;

#[derive(Clone, Debug)]
pub struct AppTheme {
    pub dark: bool,
    pub foreground: gdk::RGBA,
    pub muted: gdk::RGBA,
    pub link: gdk::RGBA,
}

pub fn detect(widget: &impl IsA<gtk::Widget>) -> AppTheme {
    let settings = gtk::Settings::default();
    let prefer_dark = settings
        .as_ref()
        .is_some_and(|settings| settings.is_gtk_application_prefer_dark_theme());
    let theme_name_dark = settings
        .as_ref()
        .and_then(|settings| settings.gtk_theme_name())
        .is_some_and(|name| name.to_ascii_lowercase().contains("dark"));
    let env_dark = std::env::var("GTK_THEME")
        .map(|value| value.to_ascii_lowercase().contains("dark"))
        .unwrap_or(false);
    let dark = prefer_dark || theme_name_dark || env_dark;
    let _ = widget.style_context().color();
    let foreground = if dark {
        gdk::RGBA::new(0.92, 0.92, 0.92, 1.0)
    } else {
        gdk::RGBA::new(0.08, 0.08, 0.08, 1.0)
    };
    let muted = if dark {
        gdk::RGBA::new(0.68, 0.68, 0.68, 1.0)
    } else {
        gdk::RGBA::new(0.42, 0.42, 0.42, 1.0)
    };
    let link = if dark {
        gdk::RGBA::new(0.45, 0.68, 1.0, 1.0)
    } else {
        gdk::RGBA::new(0.02, 0.38, 0.74, 1.0)
    };
    AppTheme {
        dark,
        foreground,
        muted,
        link,
    }
}
