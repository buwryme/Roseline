// SPDX-License-Identifier: GPL-3.0-or-later

//! the settings window: pages slide in over a column of destinations.

use std::cell::RefCell;
use std::rc::Rc;

use adw::prelude::*;
use gtk::{gio, glib};

use super::{content_page, extras_page, flags_page, graphics_page, runner_page, settings_page};
use crate::config::{self, DISPLAY_NAME};

thread_local! {
    static WINDOW: RefCell<Option<glib::WeakRef<adw::ApplicationWindow>>> = const { RefCell::new(None) };
    static NAV: RefCell<Option<glib::WeakRef<adw::NavigationView>>> = const { RefCell::new(None) };
}

/// show the settings window, reusing it when open.
pub fn present(app: &adw::Application) {
    if let Some(window) =
        WINDOW.with(|slot| slot.borrow().as_ref().and_then(glib::WeakRef::upgrade))
    {
        window.present();
        return;
    }
    let window = build(app);
    WINDOW.with(|slot| *slot.borrow_mut() = Some(window.downgrade()));
    window.present();
}

/// show the window; slide a page in when one is named.
pub fn request_settings(app: &adw::Application, page: Option<&str>) {
    present(app);
    let Some(page) = page else {
        return;
    };
    NAV.with(|slot| {
        if let Some(nav) = slot.borrow().as_ref().and_then(glib::WeakRef::upgrade) {
            push_settings(&nav, page);
        }
    });
}

pub fn toast(window: &adw::ApplicationWindow, text: &str) {
    let Some(overlay) = window.content().and_downcast::<adw::ToastOverlay>() else {
        return;
    };
    overlay.add_toast(adw::Toast::new(text));
}

pub fn about(app: &adw::Application) {
    about_dialog().present(app.active_window().as_ref());
}

fn build(app: &adw::Application) -> adw::ApplicationWindow {
    let nav = adw::NavigationView::new();
    NAV.with(|slot| *slot.borrow_mut() = Some(nav.downgrade()));

    let menu = gio::Menu::new();
    menu.append(Some("About Roséline"), Some("app.about"));
    menu.append(Some("Quit"), Some("app.quit"));
    let menu_button = gtk::MenuButton::builder()
        .icon_name("open-menu-symbolic")
        .menu_model(&menu)
        .tooltip_text("Menu")
        .build();

    let open_page: Rc<dyn Fn(&str)> = {
        let nav = nav.clone();
        Rc::new(move |kind: &str| push_settings(&nav, kind))
    };
    let root_content = settings_page::build(open_page);
    root_content.set_widget_name("settings");

    let root_header = adw::HeaderBar::builder()
        .title_widget(&adw::WindowTitle::new("Roséline Settings", ""))
        .build();
    root_header.pack_end(&menu_button);
    let root_view = adw::ToolbarView::new();
    root_view.add_top_bar(&root_header);
    root_view.set_content(Some(&root_content));
    let root_page = adw::NavigationPage::builder()
        .title("Roséline Settings")
        .tag("settings")
        .child(&root_view)
        .build();
    nav.push(&root_page);

    let overlay = adw::ToastOverlay::new();
    overlay.set_child(Some(&nav));

    adw::ApplicationWindow::builder()
        .application(app)
        .title("Roséline Settings")
        .default_width(576)
        .default_height(621)
        .content(&overlay)
        .build()
}

/// slide a settings page in over the column, whole-window wide.
fn push_settings(nav: &adw::NavigationView, kind: &str) {
    if let Some(visible) = nav.visible_page()
        && visible.tag().as_deref() == Some(kind)
    {
        return;
    }
    if nav.find_page(kind).is_some() {
        nav.pop_to_tag(kind);
        return;
    }
    let (content, title): (gtk::Widget, &str) = match kind {
        "graphics" => (graphics_page::build().upcast(), "Graphics"),
        "content" => (content_page::build().upcast(), "Content"),
        "fflags" => (flags_page::build().upcast(), "FFlags"),
        "extras" => (extras_page::build().upcast(), "Extras"),
        "runner" => (runner_page::build().upcast(), "Advanced"),
        _ => return,
    };
    let header = adw::HeaderBar::builder()
        .title_widget(&adw::WindowTitle::new(title, ""))
        .build();
    let view = adw::ToolbarView::new();
    view.add_top_bar(&header);
    view.set_content(Some(&content));
    let page = adw::NavigationPage::builder()
        .title(title)
        .tag(kind)
        .child(&view)
        .build();
    nav.push(&page);
}

fn about_dialog() -> adw::AboutDialog {
    let dialog = adw::AboutDialog::builder()
        .application_name(DISPLAY_NAME)
        .application_icon(config::APP_ID)
        .version(config::VERSION)
        .developer_name("buwryme")
        .website(config::WEBSITE)
        .issue_url(format!("{}/issues", config::WEBSITE))
        .copyright("© 2026 buwryme")
        .license_type(gtk::License::Gpl30)
        .comments("Run Roblox with Rosemary.")
        .build();
    dialog.add_credit_section(
        Some("Rosemary and Butter"),
        &["Microlift contributors https://github.com/microlift"],
    );
    dialog
}
