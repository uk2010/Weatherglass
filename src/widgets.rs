use adw::prelude::*;
use gtk::{Align, Orientation, pango};

pub fn section(title: &str, subtitle: Option<&str>) -> (gtk::Box, gtk::Box) {
    let outer = gtk::Box::new(Orientation::Vertical, 4);
    outer.add_css_class("forecast-card");
    let heading = gtk::Box::new(Orientation::Vertical, 1);
    let label = gtk::Label::new(Some(title));
    label.set_xalign(0.0);
    label.add_css_class("section-title");
    heading.append(&label);
    if let Some(text) = subtitle {
        let sub = gtk::Label::new(Some(text));
        sub.set_xalign(0.0);
        sub.set_wrap(true);
        sub.add_css_class("dim-label");
        heading.append(&sub);
    }
    outer.append(&heading);
    let content = gtk::Box::new(Orientation::Vertical, 4);
    outer.append(&content);
    (outer, content)
}
pub fn metric_card(icon: &str, title: &str, value: &str, detail: &str) -> gtk::Box {
    let card = gtk::Box::new(Orientation::Vertical, 2);
    card.add_css_class("metric-card");
    let heading = gtk::Box::new(Orientation::Horizontal, 6);
    let i = gtk::Label::new(Some(icon));
    i.add_css_class("metric-icon");
    let t = gtk::Label::new(Some(title));
    t.add_css_class("metric-title");
    t.set_xalign(0.0);
    t.set_hexpand(true);
    t.set_ellipsize(pango::EllipsizeMode::End);
    heading.append(&i);
    heading.append(&t);
    card.append(&heading);
    let v = gtk::Label::new(Some(value));
    v.add_css_class("metric-value");
    v.set_xalign(0.0);
    v.set_ellipsize(pango::EllipsizeMode::End);
    card.append(&v);
    let d = gtk::Label::new(Some(detail));
    d.add_css_class("dim-label");
    d.set_xalign(0.0);
    d.set_ellipsize(pango::EllipsizeMode::End);
    card.append(&d);
    card.set_hexpand(true);
    card.set_halign(Align::Fill);
    card
}
pub fn icon_button(icon: &str, tooltip: &str) -> gtk::Button {
    let b = gtk::Button::from_icon_name(icon);
    b.set_tooltip_text(Some(tooltip));
    b.add_css_class("flat");
    b
}
