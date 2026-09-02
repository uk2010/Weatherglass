use crate::{
    RUNTIME,
    auth::{GnomeSecretStore, LocalJwtProvider, SecretStore},
    cache::ForecastCache,
    conditions,
    geocode::{Geocoder, OpenMeteoGeocoder},
    map_layers::{MapLayerClient, MapLayerData},
    models::{Attribution, SavedLocation, WeatherResponse},
    openmeteo::OpenMeteoClient,
    radar::RadarClient,
    settings::Settings,
    state::{ForecastResult, RefreshCoordinator},
    storage::LocationStore,
    units::{self, TemperatureUnit},
    weatherkit::{WeatherKitClient, WeatherProvider},
    widgets,
};
use adw::prelude::*;
use chrono::Utc;
use chrono_tz::Tz;
use gtk::{Align, Orientation, gdk, gio, glib};
use std::{cell::RefCell, collections::HashMap, io::Cursor, rc::Rc, sync::Arc};

#[derive(Clone)]
struct ViewRefs {
    list: gtk::ListBox,
    forecast: gtk::Box,
    forecast_scroll: gtk::ScrolledWindow,
    toast: adw::ToastOverlay,
    split: adw::OverlaySplitView,
    window: adw::ApplicationWindow,
}
struct UiState {
    locations: Vec<SavedLocation>,
    selected: Option<String>,
    weather: HashMap<String, WeatherResponse>,
    settings: Settings,
    demo: bool,
    attribution: Option<Attribution>,
    attribution_mark: Option<std::path::PathBuf>,
}

fn demo_weather() -> WeatherResponse {
    serde_json::from_str(include_str!("../tests/fixtures/demo_weather.json"))
        .expect("valid bundled demo fixture")
}
fn demo_locations() -> Vec<SavedLocation> {
    let mut rows = vec![
        SavedLocation::new("Chicago", "US", "America/Chicago", 41.8781, -87.6298),
        SavedLocation::new("London", "GB", "Europe/London", 51.5072, -0.1276),
        SavedLocation::new("Tokyo", "JP", "Asia/Tokyo", 35.6762, 139.6503),
    ];
    for (i, x) in rows.iter_mut().enumerate() {
        x.sort_order = i as i64;
        x.last_selected = i == 0;
    }
    rows
}

pub fn install_css() {
    let css = gtk::CssProvider::new();
    css.load_from_string(CSS);
    gtk::style_context_add_provider_for_display(
        &gdk::Display::default().expect("display"),
        &css,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
}

pub fn build_window(app: &adw::Application) {
    if let Some(w) = app.active_window() {
        w.present();
        return;
    }
    let window = adw::ApplicationWindow::builder()
        .application(app)
        .title("Weatherglass")
        .default_width(1280)
        .default_height(820)
        .icon_name("io.github.weatherglass.Weatherglass")
        .build();
    let toast = adw::ToastOverlay::new();
    let split = adw::OverlaySplitView::new();
    split.set_sidebar_width_fraction(0.235);
    split.set_min_sidebar_width(285.0);
    split.set_max_sidebar_width(360.0);
    toast.set_child(Some(&split));
    window.set_content(Some(&toast));
    let sidebar = gtk::Box::new(Orientation::Vertical, 0);
    sidebar.add_css_class("sidebar");
    let side_header = adw::HeaderBar::new();
    side_header.add_css_class("sidebar-header");
    let title = gtk::Label::new(Some("Locations"));
    title.add_css_class("title");
    side_header.set_title_widget(Some(&title));
    let search = widgets::icon_button(
        "system-search-symbolic",
        "Search or enter coordinates (Ctrl+L)",
    );
    search.set_action_name(Some("win.search"));
    side_header.pack_start(&search);
    let settings = widgets::icon_button("preferences-system-symbolic", "Settings (Ctrl+,)");
    settings.set_action_name(Some("win.settings"));
    side_header.pack_end(&settings);
    sidebar.append(&side_header);
    let sidebar_search = gtk::Button::with_label("Search locations");
    let search_content = gtk::Box::new(Orientation::Horizontal, 10);
    let search_icon = gtk::Image::from_icon_name("system-search-symbolic");
    let search_label = gtk::Label::new(Some("Search locations"));
    search_label.set_xalign(0.0);
    search_label.set_hexpand(true);
    search_content.append(&search_icon);
    search_content.append(&search_label);
    sidebar_search.set_child(Some(&search_content));
    sidebar_search.set_action_name(Some("win.search"));
    sidebar_search.add_css_class("sidebar-search");
    sidebar_search.set_tooltip_text(Some("Search or enter coordinates (Ctrl+L)"));
    sidebar.append(&sidebar_search);
    let list = gtk::ListBox::new();
    list.set_selection_mode(gtk::SelectionMode::None);
    list.add_css_class("location-list");
    let side_scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::External)
        .vscrollbar_policy(gtk::PolicyType::External)
        .vexpand(true)
        .child(&list)
        .build();
    sidebar.append(&side_scroll);
    let location_actions = gtk::Box::new(Orientation::Horizontal, 4);
    location_actions.set_margin_start(8);
    location_actions.set_margin_end(8);
    location_actions.set_margin_top(4);
    let add = gtk::Button::from_icon_name("list-add-symbolic");
    add.set_tooltip_text(Some("Add Location"));
    add.set_action_name(Some("win.search"));
    add.set_hexpand(false);
    add.set_width_request(40);
    add.set_height_request(32);
    add.add_css_class("add-location");
    let rename = widgets::icon_button("document-edit-symbolic", "Rename selected location");
    rename.set_action_name(Some("win.rename"));
    let delete = widgets::icon_button("user-trash-symbolic", "Remove selected location (Delete)");
    delete.set_action_name(Some("win.remove"));
    location_actions.append(&add);
    location_actions.append(&rename);
    location_actions.append(&delete);
    sidebar.append(&location_actions);
    let osm = gtk::LinkButton::with_label(
        "https://open-meteo.com/en/license",
        "Forecast & search data © Open-Meteo",
    );
    osm.add_css_class("attribution-small");
    sidebar.append(&osm);
    split.set_sidebar(Some(&sidebar));
    let content = gtk::Box::new(Orientation::Vertical, 0);
    let header = adw::HeaderBar::new();
    header.add_css_class("main-header");
    let reveal = widgets::icon_button("sidebar-show-symbolic", "Show saved locations");
    {
        let split = split.clone();
        reveal.connect_clicked(move |_| split.set_show_sidebar(!split.shows_sidebar()));
    }
    header.pack_start(&reveal);
    let brand = gtk::Box::new(Orientation::Horizontal, 8);
    let icon_bytes = glib::Bytes::from_static(include_bytes!(
        "../data/icons/hicolor/128x128/apps/io.github.weatherglass.Weatherglass.png"
    ));
    let icon_texture = gdk::Texture::from_bytes(&icon_bytes).expect("embedded application icon");
    let mark = gtk::Image::from_paintable(Some(&icon_texture));
    mark.set_pixel_size(28);
    let brand_name = gtk::Label::new(Some("Weatherglass"));
    brand_name.add_css_class("title");
    brand.append(&mark);
    brand.append(&brand_name);
    header.set_title_widget(Some(&brand));
    let refresh = widgets::icon_button("view-refresh-symbolic", "Refresh weather (Ctrl+R)");
    refresh.set_action_name(Some("win.refresh"));
    header.pack_end(&refresh);
    content.append(&header);
    let forecast_scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vscrollbar_policy(gtk::PolicyType::External)
        .vexpand(true)
        .build();
    let forecast = gtk::Box::new(Orientation::Vertical, 0);
    forecast_scroll.set_child(Some(&forecast));
    content.append(&forecast_scroll);
    split.set_content(Some(&content));
    let breakpoint = adw::Breakpoint::new(
        adw::BreakpointCondition::parse("max-width: 700sp").expect("valid breakpoint"),
    );
    let collapsed = true.to_value();
    breakpoint.add_setter(&split, "collapsed", Some(&collapsed));
    window.add_breakpoint(breakpoint);
    let state = Rc::new(RefCell::new(UiState {
        locations: demo_locations(),
        selected: None,
        weather: HashMap::new(),
        settings: Settings::default(),
        demo: true,
        attribution: None,
        attribution_mark: None,
    }));
    {
        let mut s = state.borrow_mut();
        s.selected = s.locations.first().map(|x| x.id.clone());
        for l in &s.locations.clone() {
            s.weather.insert(l.id.clone(), demo_weather());
        }
    }
    let store = LocationStore::xdg().expect("XDG location store");
    let views = ViewRefs {
        list,
        forecast,
        forecast_scroll,
        toast,
        split,
        window: window.clone(),
    };
    render_all(&state, &views, &store);
    install_actions(&state, &views, &store);
    load_persisted(&state, &views, &store);
    window.present();
    let sized_window = window.clone();
    glib::timeout_add_local_once(std::time::Duration::from_millis(150), move || {
        sized_window.unmaximize();
        sized_window.set_default_size(1280, 820);
    });
}

fn install_actions(state: &Rc<RefCell<UiState>>, views: &ViewRefs, store: &LocationStore) {
    let refresh = gio::SimpleAction::new("refresh", None);
    {
        let s = state.clone();
        let v = views.clone();
        let st = store.clone();
        refresh.connect_activate(move |_, _| refresh_selected(&s, &v, &st, true));
    }
    views.window.add_action(&refresh);
    let search = gio::SimpleAction::new("search", None);
    {
        let s = state.clone();
        let v = views.clone();
        let st = store.clone();
        search.connect_activate(move |_, _| show_search(&s, &v, &st));
    }
    views.window.add_action(&search);
    let settings = gio::SimpleAction::new("settings", None);
    {
        let s = state.clone();
        let v = views.clone();
        let st = store.clone();
        settings.connect_activate(move |_, _| show_settings(&s, &v, &st));
    }
    views.window.add_action(&settings);
    let remove = gio::SimpleAction::new("remove", None);
    {
        let s = state.clone();
        let v = views.clone();
        let st = store.clone();
        remove.connect_activate(move |_, _| remove_selected(&s, &v, &st));
    }
    views.window.add_action(&remove);
    let rename = gio::SimpleAction::new("rename", None);
    {
        let s = state.clone();
        let v = views.clone();
        let st = store.clone();
        rename.connect_activate(move |_, _| show_rename(&s, &v, &st));
    }
    views.window.add_action(&rename);
}

fn load_persisted(state: &Rc<RefCell<UiState>>, views: &ViewRefs, store: &LocationStore) {
    let (tx, rx) = async_channel::bounded(1);
    let st = store.clone();
    RUNTIME.spawn(async move {
        let value = async {
            st.migrate().await?;
            let mut rows = st.list().await?;
            if rows.is_empty() {
                for l in demo_locations() {
                    st.upsert(l).await?;
                }
                rows = st.list().await?;
            }
            let settings = Settings::load(Settings::xdg_path()?).await?;
            Ok::<_, anyhow::Error>((rows, settings))
        }
        .await;
        let _ = tx.send(value).await;
    });
    let s = state.clone();
    let v = views.clone();
    let st = store.clone();
    glib::spawn_future_local(async move {
        match rx.recv().await {
            Ok(Ok((rows, settings))) => {
                let mut state = s.borrow_mut();
                state.locations = rows;
                state.settings = settings;
                apply_theme(&state.settings.theme, &v.window);
                state.selected = state
                    .locations
                    .iter()
                    .find(|x| x.last_selected)
                    .or(state.locations.first())
                    .map(|x| x.id.clone());
                let selected_country = state
                    .selected
                    .as_ref()
                    .and_then(|id| state.locations.iter().find(|location| &location.id == id))
                    .map(|location| location.country_code.clone());
                if let Some(country) = selected_country {
                    state.settings.apply_country_defaults(&country);
                }
                for l in state.locations.clone() {
                    state.weather.entry(l.id).or_insert_with(demo_weather);
                }
                drop(state);
                render_all(&s, &v, &st);
                refresh_selected(&s, &v, &st, false)
            }
            Ok(Err(e)) => toast(&v, &format!("Could not open saved locations: {e}")),
            Err(_) => {}
        }
    });
}

fn render_all(state: &Rc<RefCell<UiState>>, views: &ViewRefs, store: &LocationStore) {
    render_sidebar(state, views, store);
    render_forecast(state, views);
}
fn clear_box(b: &gtk::Box) {
    while let Some(c) = b.first_child() {
        b.remove(&c)
    }
}
fn clear_list(b: &gtk::ListBox) {
    while let Some(c) = b.first_child() {
        b.remove(&c)
    }
}

fn render_sidebar(state: &Rc<RefCell<UiState>>, views: &ViewRefs, store: &LocationStore) {
    clear_list(&views.list);
    let snapshot = state.borrow();
    for l in &snapshot.locations {
        let row = gtk::ListBoxRow::new();
        row.add_css_class("location-card");
        row.set_activatable(true);
        row.set_tooltip_text(Some("Select location; drag to reorder"));
        let line = gtk::Box::new(Orientation::Horizontal, 10);
        line.set_margin_start(14);
        line.set_margin_end(12);
        line.set_margin_top(12);
        line.set_margin_bottom(12);
        let text = gtk::Box::new(Orientation::Vertical, 4);
        text.set_hexpand(true);
        let top = gtk::Box::new(Orientation::Horizontal, 8);
        let name = gtk::Label::new(Some(&l.display_name));
        name.set_xalign(0.0);
        name.add_css_class("location-name");
        name.set_hexpand(true);
        top.append(&name);
        let tz: Tz = l.timezone.parse().unwrap_or(chrono_tz::UTC);
        let time = Utc::now().with_timezone(&tz);
        let local = gtk::Label::new(Some(&time.format("%-I:%M %p").to_string()));
        local.add_css_class("location-time");
        top.append(&local);
        text.append(&top);
        let weather = snapshot.weather.get(&l.id);
        let summary = weather
            .and_then(|w| w.current_weather.as_ref())
            .map(|c| conditions::present(&c.condition_code, c.daylight).description)
            .unwrap_or("Not updated");
        let cond = gtk::Label::new(Some(summary));
        cond.set_xalign(0.0);
        cond.add_css_class("location-condition");
        text.append(&cond);
        let range_text = weather
            .and_then(|weather| weather.forecast_daily.as_ref())
            .and_then(|daily| daily.days.first())
            .map(|day| {
                format!(
                    "H:{}  L:{}",
                    format_temp(day.temperature_max, snapshot.settings.temperature),
                    format_temp(day.temperature_min, snapshot.settings.temperature)
                )
            })
            .unwrap_or_default();
        let range = gtk::Label::new(Some(&range_text));
        range.set_xalign(0.0);
        range.add_css_class("sidebar-range");
        text.append(&range);
        line.append(&text);
        let weather_side = gtk::Box::new(Orientation::Horizontal, 8);
        weather_side.set_valign(Align::Center);
        weather_side.set_halign(Align::End);
        let temp = weather
            .and_then(|w| w.current_weather.as_ref())
            .map(|c| format_temp(c.temperature, snapshot.settings.temperature))
            .unwrap_or_else(|| "—".into());
        let current_temp = gtk::Label::new(Some(&temp));
        current_temp.add_css_class("sidebar-temp");
        current_temp.set_halign(Align::End);
        let icon = gtk::Label::new(Some(
            weather
                .and_then(|w| w.current_weather.as_ref())
                .map(|c| conditions::present(&c.condition_code, c.daylight).symbol)
                .unwrap_or("◌"),
        ));
        icon.add_css_class("sidebar-condition-icon");
        icon.set_halign(Align::End);
        weather_side.append(&current_temp);
        weather_side.append(&icon);
        line.append(&weather_side);
        row.set_child(Some(&line));
        if snapshot.selected.as_deref() == Some(&l.id) {
            row.add_css_class("selected-location")
        }
        let id = l.id.clone();
        {
            let s = state.clone();
            let v = views.clone();
            let st = store.clone();
            let id = id.clone();
            let click = gtk::GestureClick::new();
            click.connect_released(move |_, _, _, _| select_location(&id, &s, &v, &st));
            row.add_controller(click);
        }
        let drag = gtk::DragSource::builder()
            .actions(gdk::DragAction::MOVE)
            .build();
        {
            let id = id.clone();
            drag.connect_prepare(move |_, _, _| {
                Some(gdk::ContentProvider::for_value(&id.to_value()))
            });
        }
        row.add_controller(drag);
        let drop = gtk::DropTarget::new(String::static_type(), gdk::DragAction::MOVE);
        {
            let target = id.clone();
            let s = state.clone();
            let v = views.clone();
            let st = store.clone();
            drop.connect_drop(move |_, value, _, _| {
                if let Ok(source) = value.get::<String>() {
                    drop_before(&source, &target, &s, &v, &st);
                    true
                } else {
                    false
                }
            });
        }
        row.add_controller(drop);
        views.list.append(&row);
    }
    drop(snapshot);
}

fn select_location(
    id: &str,
    state: &Rc<RefCell<UiState>>,
    views: &ViewRefs,
    store: &LocationStore,
) {
    {
        let mut s = state.borrow_mut();
        s.selected = Some(id.to_string());
        for l in &mut s.locations {
            l.last_selected = l.id == id;
        }
        let selected_country = s
            .locations
            .iter()
            .find(|location| location.id == id)
            .map(|location| location.country_code.clone());
        if let Some(country) = selected_country {
            s.settings.apply_country_defaults(&country);
        }
    }
    let st = store.clone();
    let id = id.to_string();
    RUNTIME.spawn(async move {
        let _ = st.select(id).await;
    });
    render_all(state, views, store);
    if views.split.is_collapsed() {
        views.split.set_show_sidebar(false)
    }
}
fn drop_before(
    source: &str,
    target: &str,
    state: &Rc<RefCell<UiState>>,
    views: &ViewRefs,
    store: &LocationStore,
) {
    if source == target {
        return;
    }
    let mut s = state.borrow_mut();
    if let (Some(from), Some(mut to)) = (
        s.locations.iter().position(|x| x.id == source),
        s.locations.iter().position(|x| x.id == target),
    ) {
        let item = s.locations.remove(from);
        if from < to {
            to -= 1
        }
        s.locations.insert(to, item);
        for (i, l) in s.locations.iter_mut().enumerate() {
            l.sort_order = i as i64;
        }
    }
    let ids = s.locations.iter().map(|x| x.id.clone()).collect();
    drop(s);
    let st = store.clone();
    RUNTIME.spawn(async move {
        let _ = st.reorder(ids).await;
    });
    render_sidebar(state, views, store)
}

fn render_forecast(state: &Rc<RefCell<UiState>>, views: &ViewRefs) {
    clear_box(&views.forecast);
    let s = state.borrow();
    let Some(id) = s.selected.as_ref() else {
        let status = adw::StatusPage::builder()
            .icon_name("weather-clear-symbolic")
            .title("Add a location")
            .description("Search for a city or enter latitude, longitude.")
            .build();
        views.forecast.append(&status);
        return;
    };
    let Some(l) = s.locations.iter().find(|l| &l.id == id) else {
        return;
    };
    let Some(w) = s.weather.get(id) else { return };
    let page = forecast_page(l, w, &s, &views.window);
    views.forecast.append(&page);
    let scroll = views.forecast_scroll.clone();
    glib::idle_add_local_once(move || scroll.vadjustment().set_value(0.0));
}

fn forecast_page(
    l: &SavedLocation,
    w: &WeatherResponse,
    state: &UiState,
    parent: &adw::ApplicationWindow,
) -> gtk::Box {
    let page = gtk::Box::new(Orientation::Vertical, 8);
    page.add_css_class("forecast-page");
    page.set_margin_start(14);
    page.set_margin_end(14);
    page.set_margin_top(10);
    page.set_margin_bottom(14);
    page.set_halign(Align::Fill);
    if state.demo {
        let banner = adw::Banner::new("Demo data — refreshing from Open-Meteo…");
        banner.set_revealed(true);
        page.append(&banner)
    }
    if let Some(c) = &w.current_weather {
        let hero = gtk::Box::new(Orientation::Vertical, 8);
        hero.add_css_class("hero");
        let hero_top = gtk::Box::new(Orientation::Horizontal, 8);
        hero_top.add_css_class("hero-top");
        let hero_left = gtk::Box::new(Orientation::Vertical, 5);
        hero_left.set_hexpand(true);
        let location = gtk::Label::new(Some(&l.display_name));
        location.add_css_class("hero-location");
        location.set_xalign(0.0);
        let tz: Tz = l.timezone.parse().unwrap_or(chrono_tz::UTC);
        let local = Utc::now().with_timezone(&tz);
        let date = gtk::Label::new(Some(&format!(
            "{} · {}",
            local.format("%A, %b %-d"),
            local.format("%-I:%M %p"),
        )));
        date.add_css_class("hero-date");
        date.set_xalign(0.0);
        let updated = gtk::Label::new(Some(&if state.demo {
            "Demo data".to_string()
        } else {
            format!("Updated {}", c.as_of.with_timezone(&tz).format("%-I:%M %p"))
        }));
        updated.add_css_class("hero-updated");
        updated.set_xalign(0.0);
        hero_left.append(&location);
        hero_left.append(&date);
        hero_left.append(&updated);
        let hero_right = gtk::Box::new(Orientation::Vertical, 1);
        hero_right.set_halign(Align::End);
        hero_right.set_valign(Align::Center);
        let temperature_line = gtk::Box::new(Orientation::Horizontal, 8);
        temperature_line.set_halign(Align::End);
        let temp = gtk::Label::new(Some(&format_temp(
            c.temperature,
            state.settings.temperature,
        )));
        temp.add_css_class("hero-temp");
        let icon = gtk::Label::new(Some(
            conditions::present(&c.condition_code, c.daylight).symbol,
        ));
        icon.add_css_class("hero-icon");
        temperature_line.append(&temp);
        temperature_line.append(&icon);
        let p = conditions::present(&c.condition_code, c.daylight);
        let condition = gtk::Label::new(Some(p.description));
        condition.add_css_class("hero-condition");
        condition.set_halign(Align::End);
        let hi_lo = w
            .forecast_daily
            .as_ref()
            .and_then(|x| x.days.first())
            .map(|d| {
                format!(
                    "High {}   Low {}",
                    format_temp(d.temperature_max, state.settings.temperature),
                    format_temp(d.temperature_min, state.settings.temperature)
                )
            })
            .unwrap_or_default();
        let range = gtk::Label::new(Some(
            &hi_lo.replace("High", "High").replace("   Low", "   Low"),
        ));
        range.add_css_class("hero-range");
        range.set_halign(Align::End);
        hero_right.append(&temperature_line);
        hero_right.append(&condition);
        hero_right.append(&range);
        hero_top.append(&hero_left);
        hero_top.append(&hero_right);
        hero.append(&hero_top);
        page.append(&hero)
    }
    let dashboard = gtk::Grid::new();
    dashboard.add_css_class("weather-dashboard");
    dashboard.set_column_spacing(14);
    dashboard.set_row_spacing(14);
    dashboard.set_hexpand(true);
    dashboard.set_column_homogeneous(false);
    let lower_right = gtk::Box::new(Orientation::Vertical, 10);
    lower_right.add_css_class("radar-column");
    lower_right.set_width_request(390);
    lower_right.set_hexpand(true);
    lower_right.set_valign(Align::Start);
    dashboard.attach(&lower_right, 1, 2, 1, 1);
    if let Some(alerts) = &w.weather_alerts {
        for alert in &alerts.details {
            let card = gtk::Box::new(Orientation::Vertical, 7);
            card.add_css_class("alert-card");
            let severity = gtk::Label::new(Some(&format!(
                "⚠  {} WEATHER ALERT",
                alert.severity.as_deref().unwrap_or("ACTIVE").to_uppercase()
            )));
            severity.add_css_class("alert-severity");
            severity.set_xalign(0.0);
            let title = gtk::Label::new(Some(
                alert.description.as_deref().unwrap_or("Weather alert"),
            ));
            title.add_css_class("alert-title");
            title.set_xalign(0.0);
            title.set_wrap(true);
            card.append(&severity);
            card.append(&title);
            if let Some(source) = &alert.source {
                let agency = gtk::Label::new(Some(source));
                agency.set_xalign(0.0);
                agency.add_css_class("dim-label");
                card.append(&agency)
            }
            if let Some(url) = &alert.details_url {
                let link = gtk::LinkButton::with_label(url, "View unmodified alert details");
                link.set_halign(Align::Start);
                card.append(&link)
            }
            page.append(&card)
        }
    }
    if let Some(hourly) = &w.forecast_hourly {
        let sec = gtk::Box::new(Orientation::Vertical, 0);
        sec.add_css_class("hourly-panel");
        let body = gtk::Box::new(Orientation::Vertical, 8);
        let scroll = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vscrollbar_policy(gtk::PolicyType::Never)
            .min_content_height(112)
            .build();
        let row = gtk::Box::new(Orientation::Horizontal, 5);
        row.set_homogeneous(true);
        row.set_hexpand(true);
        let tz: Tz = l.timezone.parse().unwrap_or(chrono_tz::UTC);
        let first_hour = current_hour_index(&hourly.hours, Utc::now());
        for (visible_index, h) in hourly.hours.iter().skip(first_hour).take(12).enumerate() {
            let item = gtk::Box::new(Orientation::Vertical, 3);
            item.add_css_class("hour-tile");
            let dt = h.forecast_start.with_timezone(&tz);
            let time = gtk::Label::new(Some(&if visible_index == 0 {
                "Now".to_string()
            } else {
                dt.format("%-I %p").to_string()
            }));
            let icon = gtk::Label::new(Some(
                conditions::present(&h.condition_code, h.daylight.unwrap_or(true)).symbol,
            ));
            icon.add_css_class("hour-icon");
            let temp = gtk::Label::new(Some(&format_temp(
                h.temperature,
                state.settings.temperature,
            )));
            temp.add_css_class("hour-temp");
            let rain = gtk::Label::new(Some(&format!(
                "{}%",
                (h.precipitation_chance.unwrap_or(0.0) * 100.0).round()
            )));
            rain.add_css_class("precip");
            item.append(&time);
            item.append(&icon);
            item.append(&temp);
            item.append(&rain);
            row.append(&item)
        }
        scroll.set_child(Some(&row));
        body.append(&scroll);
        let metrics = gtk::Box::new(Orientation::Horizontal, 4);
        metrics.add_css_class("metric-tabs");
        let chart = gtk::Box::new(Orientation::Vertical, 6);
        chart.add_css_class("chart");
        let chart_summary = gtk::Label::new(None);
        chart_summary.set_xalign(0.0);
        chart_summary.set_wrap(true);
        chart_summary.add_css_class("dim-label");
        chart.set_visible(false);
        chart_summary.set_visible(false);
        let metric_choices = [
            HourlyMetric::Temperature,
            HourlyMetric::Precipitation,
            HourlyMetric::Wind,
            HourlyMetric::Humidity,
            HourlyMetric::Pressure,
            HourlyMetric::FeelsLike,
        ];
        let mut first_button: Option<gtk::ToggleButton> = None;
        let shown_metric = Rc::new(RefCell::new(None::<HourlyMetric>));
        for metric in metric_choices {
            let b = gtk::ToggleButton::with_label(metric.label());
            b.add_css_class("pill");
            b.add_css_class("metric-tab");
            b.set_tooltip_text(Some(&format!(
                "Show the hourly {} chart",
                metric.label().to_lowercase()
            )));
            if let Some(first) = &first_button {
                b.set_group(Some(first))
            } else {
                first_button = Some(b.clone());
                b.set_active(true)
            }
            b.set_sensitive(metric_series(hourly, metric, &state.settings).is_some());
            let chart = chart.clone();
            let summary = chart_summary.clone();
            let hourly = hourly.clone();
            let settings = state.settings.clone();
            let shown_metric = shown_metric.clone();
            b.connect_clicked(move |button| {
                if *shown_metric.borrow() == Some(metric) {
                    *shown_metric.borrow_mut() = None;
                    button.set_active(false);
                    chart.set_visible(false);
                    summary.set_visible(false);
                } else {
                    *shown_metric.borrow_mut() = Some(metric);
                    button.set_active(true);
                    chart.set_visible(true);
                    summary.set_visible(true);
                    render_hourly_chart(&chart, &summary, &hourly, metric, &settings, tz);
                }
            });
            metrics.append(&b)
        }
        let metric_scroll = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vscrollbar_policy(gtk::PolicyType::Never)
            .min_content_height(38)
            .max_content_height(44)
            .css_classes(["metric-tabs-scroll"])
            .child(&metrics)
            .build();
        sec.append(&body);
        dashboard.attach(&sec, 0, 0, 2, 1);
        let metric_area = gtk::Box::new(Orientation::Vertical, 6);
        metric_area.add_css_class("metric-area");
        metric_area.append(&metric_scroll);
        render_hourly_chart(
            &chart,
            &chart_summary,
            hourly,
            HourlyMetric::Temperature,
            &state.settings,
            tz,
        );
        metric_area.append(&chart);
        metric_area.append(&chart_summary);
        dashboard.attach(&metric_area, 0, 1, 2, 1)
    }
    if let Some(daily) = &w.forecast_daily {
        let (sec, body) = widgets::section("10-DAY FORECAST", None);
        sec.add_css_class("ten-day-card");
        let global_min = daily
            .days
            .iter()
            .map(|d| d.temperature_min)
            .fold(f64::INFINITY, f64::min);
        let global_max = daily
            .days
            .iter()
            .map(|d| d.temperature_max)
            .fold(f64::NEG_INFINITY, f64::max);
        let tz: Tz = l.timezone.parse().unwrap_or(chrono_tz::UTC);
        for (i, d) in daily.days.iter().take(10).enumerate() {
            let row = gtk::Button::new();
            row.add_css_class("day-row");
            let line = gtk::Box::new(Orientation::Horizontal, 5);
            let day_text = if i == 0 {
                "Today".to_string()
            } else {
                d.forecast_start.with_timezone(&tz).format("%a").to_string()
            };
            let day = gtk::Label::new(Some(&day_text));
            day.set_width_chars(5);
            day.set_xalign(0.0);
            let icon = gtk::Label::new(Some(conditions::present(&d.condition_code, true).symbol));
            icon.add_css_class("day-icon");
            let rain = gtk::Label::new(Some(&format!(
                "{}%",
                (d.precipitation_chance.unwrap_or(0.0) * 100.0).round()
            )));
            rain.add_css_class("precip");
            rain.set_width_chars(4);
            let low = gtk::Label::new(Some(&format_temp(
                d.temperature_min,
                state.settings.temperature,
            )));
            let bar = gtk::LevelBar::for_interval(global_min, global_max);
            bar.set_value((d.temperature_min + d.temperature_max) / 2.0);
            bar.set_hexpand(true);
            bar.set_tooltip_text(Some(&format!(
                "Range {} to {}",
                format_temp(d.temperature_min, state.settings.temperature),
                format_temp(d.temperature_max, state.settings.temperature)
            )));
            let high = gtk::Label::new(Some(&format_temp(
                d.temperature_max,
                state.settings.temperature,
            )));
            line.append(&day);
            line.append(&icon);
            line.append(&rain);
            line.append(&low);
            line.append(&bar);
            line.append(&high);
            row.set_child(Some(&line));
            body.append(&row)
        }
        sec.set_hexpand(true);
        sec.set_valign(Align::Start);
        dashboard.attach(&sec, 0, 2, 1, 1)
    }
    page.append(&dashboard);
    if let Some(c) = &w.current_weather {
        let grid = gtk::FlowBox::new();
        grid.add_css_class("conditions-grid");
        grid.set_selection_mode(gtk::SelectionMode::None);
        grid.set_max_children_per_line(5);
        grid.set_min_children_per_line(2);
        grid.set_column_spacing(10);
        grid.set_row_spacing(10);
        let day = w.forecast_daily.as_ref().and_then(|x| x.days.first());
        let air_quality = w
            .extra
            .get("airQuality")
            .and_then(|air| {
                if l.country_code == "US" {
                    air.get("us_aqi")
                } else {
                    air.get("european_aqi")
                }
            })
            .and_then(serde_json::Value::as_f64)
            .map(|value| value.round() as i32);
        lower_right.prepend(&radar_card(l, parent));
        let details_heading = gtk::Label::new(Some("CURRENT CONDITIONS"));
        details_heading.add_css_class("section-title");
        details_heading.set_xalign(0.0);
        page.append(&details_heading);
        let items = vec![
            widgets::metric_card(
                "◎",
                "AIR QUALITY",
                &air_quality
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| "—".into()),
                air_quality_description(air_quality, &l.country_code),
            ),
            widgets::metric_card(
                "➤",
                "WIND",
                &format_wind(c.wind_speed.unwrap_or(0.0), state.settings.wind),
                &format!(
                    "{}° · Gusts {}",
                    c.wind_direction.unwrap_or(0),
                    format_wind(c.wind_gust.unwrap_or(0.0), state.settings.wind)
                ),
            ),
            widgets::metric_card(
                "☾",
                "MOON",
                &day.and_then(|d| {
                    d.extra
                        .get("moonPhaseFraction")
                        .and_then(serde_json::Value::as_f64)
                        .map(format_moon_illumination)
                        .or_else(|| d.moon_phase.as_deref().map(format_moon_phase))
                })
                .unwrap_or_else(|| "Unavailable".into()),
                &day.and_then(|d| d.moonset)
                    .map(|time| {
                        format!(
                            "Moonset {}",
                            time.with_timezone(&l.timezone.parse::<Tz>().unwrap_or(chrono_tz::UTC))
                                .format("%-I:%M %p")
                        )
                    })
                    .unwrap_or_else(|| "Moon times unavailable".into()),
            ),
            widgets::metric_card(
                "☀",
                "SUNRISE",
                &day.and_then(|d| d.sunrise)
                    .map(|time| {
                        time.with_timezone(&l.timezone.parse::<Tz>().unwrap_or(chrono_tz::UTC))
                            .format("%-I:%M %p")
                            .to_string()
                    })
                    .unwrap_or_else(|| "—".into()),
                "First light",
            ),
            widgets::metric_card(
                "☀",
                "SUNSET",
                &day.and_then(|d| d.sunset)
                    .map(|time| {
                        time.with_timezone(&l.timezone.parse::<Tz>().unwrap_or(chrono_tz::UTC))
                            .format("%-I:%M %p")
                            .to_string()
                    })
                    .unwrap_or_else(|| "—".into()),
                "Last light",
            ),
            widgets::metric_card(
                "☀",
                "UV INDEX",
                &day.and_then(|d| d.max_uv_index)
                    .or(c.uv_index)
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| "—".into()),
                uv_risk(day.and_then(|d| d.max_uv_index).or(c.uv_index)),
            ),
            widgets::metric_card(
                "●",
                "PRECIPITATION",
                &format_precip(
                    day.and_then(|d| d.precipitation_amount).unwrap_or(0.0),
                    state.settings.precipitation,
                ),
                &format!(
                    "{}% chance today",
                    (day.and_then(|d| d.precipitation_chance).unwrap_or(0.0) * 100.0).round()
                ),
            ),
            widgets::metric_card(
                "≈",
                "FEELS LIKE",
                &format_temp(
                    c.temperature_apparent.unwrap_or(c.temperature),
                    state.settings.temperature,
                ),
                "Humidity and wind-adjusted",
            ),
            widgets::metric_card(
                "◉",
                "HUMIDITY",
                &format!("{}%", (c.humidity.unwrap_or(0.0) * 100.0).round()),
                &format!(
                    "Dew point {}",
                    format_temp(
                        c.temperature_dew_point.unwrap_or(0.0),
                        state.settings.temperature
                    )
                ),
            ),
            widgets::metric_card(
                "◫",
                "VISIBILITY",
                &format_distance(c.visibility.unwrap_or(0.0), state.settings.distance),
                "Clear viewing distance",
            ),
            widgets::metric_card(
                "⌁",
                "PRESSURE",
                &format_pressure(c.pressure.unwrap_or(0.0), state.settings.pressure),
                c.pressure_trend.as_deref().unwrap_or("steady"),
            ),
            widgets::metric_card(
                "☁",
                "CLOUD COVER",
                &format!("{}%", (c.cloud_cover.unwrap_or(0.0) * 100.0).round()),
                "Current sky coverage",
            ),
        ];
        for item in items {
            grid.insert(&item, -1)
        }
        if day.and_then(|d| d.snowfall_amount).unwrap_or(0.0) > 0.0 {
            grid.insert(
                &widgets::metric_card(
                    "❄",
                    "SNOWFALL",
                    &format!(
                        "{:.1} mm",
                        day.and_then(|d| d.snowfall_amount).unwrap_or(0.0)
                    ),
                    "Expected today",
                ),
                -1,
            )
        }
        page.append(&grid)
    }
    if let Some(path) = &state.attribution_mark {
        let mark = gtk::Picture::for_filename(path);
        mark.set_can_shrink(true);
        mark.set_height_request(32);
        mark.set_halign(Align::Center);
        mark.set_tooltip_text(Some("Official Apple Weather attribution mark"));
        page.append(&mark)
    }
    let source = state
        .attribution
        .as_ref()
        .map(|a| a.service_name.as_str())
        .unwrap_or("Apple Weather");
    let legal = w
        .current_weather
        .as_ref()
        .and_then(|x| x.metadata.attribution_url.as_deref())
        .or_else(|| {
            state
                .attribution
                .as_ref()
                .and_then(|a| a.legal_page_url.as_deref())
        })
        .unwrap_or("https://weatherkit.apple.com/legal-attribution.html");
    let attribution = gtk::LinkButton::with_label(
        legal,
        &format!("Weather data: {source} · Legal attribution"),
    );
    attribution.set_halign(Align::Center);
    attribution.add_css_class("weather-attribution");
    page.append(&attribution);
    page
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HourlyMetric {
    Temperature,
    Precipitation,
    Wind,
    Humidity,
    Pressure,
    FeelsLike,
}
impl HourlyMetric {
    fn label(self) -> &'static str {
        match self {
            Self::Temperature => "Temperature",
            Self::Precipitation => "Precipitation",
            Self::Wind => "Wind",
            Self::Humidity => "Humidity",
            Self::Pressure => "Pressure",
            Self::FeelsLike => "Feels like",
        }
    }
}
struct MetricSeries {
    values: Vec<Option<f64>>,
    unit: &'static str,
    decimals: usize,
    start_index: usize,
}

fn current_hour_index(hours: &[crate::models::HourCondition], now: chrono::DateTime<Utc>) -> usize {
    hours
        .iter()
        .position(|hour| hour.forecast_start + chrono::Duration::hours(1) > now)
        .unwrap_or(0)
}

fn metric_series(
    hourly: &crate::models::HourlyForecast,
    metric: HourlyMetric,
    settings: &Settings,
) -> Option<MetricSeries> {
    let start_index = current_hour_index(&hourly.hours, Utc::now());
    let values = hourly
        .hours
        .iter()
        .skip(start_index)
        .take(12)
        .map(|h| match metric {
            HourlyMetric::Temperature => {
                Some(units::temperature(h.temperature, settings.temperature))
            }
            HourlyMetric::Precipitation => h.precipitation_chance.map(|v| v * 100.0),
            HourlyMetric::Wind => h.wind_speed.map(|v| units::wind(v, settings.wind)),
            HourlyMetric::Humidity => h.humidity.map(|v| v * 100.0),
            HourlyMetric::Pressure => h.pressure.map(|v| units::pressure(v, settings.pressure)),
            HourlyMetric::FeelsLike => h
                .temperature_apparent
                .map(|v| units::temperature(v, settings.temperature)),
        })
        .collect::<Vec<_>>();
    if !values.iter().any(Option::is_some) {
        return None;
    }
    let (unit, decimals) = match metric {
        HourlyMetric::Temperature | HourlyMetric::FeelsLike => (
            if settings.temperature == TemperatureUnit::Celsius {
                "°C"
            } else {
                "°F"
            },
            0,
        ),
        HourlyMetric::Precipitation | HourlyMetric::Humidity => ("%", 0),
        HourlyMetric::Wind => (
            match settings.wind {
                crate::units::WindUnit::KilometresPerHour => "km/h",
                crate::units::WindUnit::MilesPerHour => "mph",
                crate::units::WindUnit::MetresPerSecond => "m/s",
                crate::units::WindUnit::Knots => "kn",
            },
            if settings.wind == crate::units::WindUnit::MetresPerSecond {
                1
            } else {
                0
            },
        ),
        HourlyMetric::Pressure => (
            if settings.pressure == crate::units::PressureUnit::Hectopascals {
                "hPa"
            } else {
                "inHg"
            },
            if settings.pressure == crate::units::PressureUnit::Hectopascals {
                0
            } else {
                2
            },
        ),
    };
    Some(MetricSeries {
        values,
        unit,
        decimals,
        start_index,
    })
}
fn render_hourly_chart(
    container: &gtk::Box,
    summary: &gtk::Label,
    hourly: &crate::models::HourlyForecast,
    metric: HourlyMetric,
    settings: &Settings,
    tz: Tz,
) {
    clear_box(container);
    let Some(series) = metric_series(hourly, metric, settings) else {
        summary.set_text(&format!(
            "{} data is unavailable for this forecast.",
            metric.label()
        ));
        return;
    };
    let present = series.values.iter().flatten().copied().collect::<Vec<_>>();
    let min = present.iter().copied().fold(f64::INFINITY, f64::min);
    let max = present.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let graph = gtk::Box::new(Orientation::Horizontal, 4);
    graph.set_height_request(96);
    for (index, (value, hour)) in series
        .values
        .iter()
        .zip(hourly.hours.iter().skip(series.start_index))
        .enumerate()
    {
        let column = gtk::Box::new(Orientation::Vertical, 3);
        column.set_hexpand(true);
        let value_label = gtk::Label::new(Some(
            &value
                .map(|v| format!("{:.*}", series.decimals, v))
                .unwrap_or_else(|| "—".into()),
        ));
        value_label.add_css_class("chart-value");
        let bar = gtk::ProgressBar::new();
        bar.set_orientation(Orientation::Vertical);
        bar.set_inverted(true);
        bar.set_vexpand(true);
        bar.set_fraction(
            value
                .map(|v| {
                    if (max - min).abs() < 0.001 {
                        0.55
                    } else {
                        0.12 + 0.82 * (v - min) / (max - min)
                    }
                })
                .unwrap_or(0.0),
        );
        let time = if index == 0 {
            "Now".to_string()
        } else {
            hour.forecast_start
                .with_timezone(&tz)
                .format("%-I %p")
                .to_string()
        };
        bar.set_tooltip_text(Some(
            &value
                .map(|v| {
                    format!(
                        "{} at {}: {:.*} {}",
                        metric.label(),
                        &time,
                        series.decimals,
                        v,
                        series.unit
                    )
                })
                .unwrap_or_else(|| format!("{} at {}: unavailable", metric.label(), &time)),
        ));
        let time_label = gtk::Label::new(Some(&time));
        time_label.add_css_class("chart-time");
        column.append(&value_label);
        column.append(&bar);
        column.append(&time_label);
        graph.append(&column)
    }
    container.append(&graph);
    let first = present.first().copied().unwrap_or(0.0);
    let last = present.last().copied().unwrap_or(first);
    let direction = if last > first + 0.05 {
        "rising"
    } else if last < first - 0.05 {
        "falling"
    } else {
        "steady"
    };
    let text = format!(
        "{} is {direction} overall, ranging from {:.*} {} to {:.*} {} over the next {} hours.",
        metric.label(),
        series.decimals,
        min,
        series.unit,
        series.decimals,
        max,
        series.unit,
        present.len()
    );
    summary.set_text(&text);
    container.set_tooltip_text(Some(&text));
}
fn uv_risk(v: Option<i32>) -> &'static str {
    match v.unwrap_or(-1) {
        0..=2 => "Low",
        3..=5 => "Moderate — protection recommended",
        6..=7 => "High — reduce midday exposure",
        8..=10 => "Very high",
        11.. => "Extreme",
        _ => "Unavailable",
    }
}
fn air_quality_description(value: Option<i32>, country: &str) -> &'static str {
    let Some(value) = value else {
        return "Air-quality forecast unavailable";
    };
    if country == "US" {
        match value {
            0..=50 => "Good",
            51..=100 => "Moderate",
            101..=150 => "Unhealthy for sensitive groups",
            151..=200 => "Unhealthy",
            201..=300 => "Very unhealthy",
            _ => "Hazardous",
        }
    } else {
        match value {
            0..=20 => "Good",
            21..=40 => "Fair",
            41..=60 => "Moderate",
            61..=80 => "Poor",
            81..=100 => "Very poor",
            _ => "Extremely poor",
        }
    }
}
fn format_moon_phase(value: &str) -> String {
    match value {
        "newMoon" => "New moon",
        "waxingCrescent" | "waningCrescent" => "1/4 full",
        "firstQuarter" | "lastQuarter" => "1/2 full",
        "waxingGibbous" | "waningGibbous" => "3/4 full",
        "fullMoon" => "Full moon",
        _ => "Illumination unavailable",
    }
    .into()
}

fn format_moon_illumination(phase: f64) -> String {
    let illumination = (1.0 - (std::f64::consts::TAU * phase).cos()) * 50.0;
    match (illumination.clamp(0.0, 100.0) / 25.0).round() as i32 {
        0 => "New moon",
        1 => "1/4 full",
        2 => "1/2 full",
        3 => "3/4 full",
        _ => "Full moon",
    }
    .into()
}

fn format_temp(c: f64, unit: TemperatureUnit) -> String {
    format!(
        "{:.0}°{}",
        units::temperature(c, unit),
        if unit == TemperatureUnit::Celsius {
            "C"
        } else {
            "F"
        }
    )
}
fn format_wind(kmh: f64, unit: crate::units::WindUnit) -> String {
    let suffix = match unit {
        crate::units::WindUnit::KilometresPerHour => "km/h",
        crate::units::WindUnit::MilesPerHour => "mph",
        crate::units::WindUnit::MetresPerSecond => "m/s",
        crate::units::WindUnit::Knots => "kn",
    };
    format!("{:.0} {suffix}", units::wind(kmh, unit))
}
fn format_precip(mm: f64, unit: crate::units::PrecipitationUnit) -> String {
    match unit {
        crate::units::PrecipitationUnit::Millimetres => format!("{:.1} mm", mm),
        crate::units::PrecipitationUnit::Inches => {
            format!("{:.2} in", units::precipitation(mm, unit))
        }
    }
}
fn format_pressure(hpa: f64, unit: crate::units::PressureUnit) -> String {
    match unit {
        crate::units::PressureUnit::Hectopascals => format!("{hpa:.0} hPa"),
        crate::units::PressureUnit::InchesMercury => {
            format!("{:.2} inHg", units::pressure(hpa, unit))
        }
    }
}
fn format_distance(km: f64, unit: crate::units::DistanceUnit) -> String {
    match unit {
        crate::units::DistanceUnit::Kilometres => format!("{km:.1} km"),
        crate::units::DistanceUnit::Miles => format!("{:.1} mi", units::distance(km, unit)),
    }
}

struct RadarFrameWidget {
    root: gtk::Fixed,
    base: gtk::Widget,
    precipitation: gtk::Widget,
    temperature: gtk::Widget,
    air_quality: gtk::Widget,
    wind: gtk::Widget,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum RadarLayer {
    Precipitation,
    Temperature,
    AirQuality,
    Wind,
}

enum RadarPlaybackFrame {
    Observed(chrono::DateTime<Utc>, gdk::Texture),
    Forecast(chrono::DateTime<Utc>, usize),
}

impl RadarPlaybackFrame {
    fn time(&self) -> chrono::DateTime<Utc> {
        match self {
            Self::Observed(time, _) | Self::Forecast(time, _) => *time,
        }
    }

    fn is_forecast(&self) -> bool {
        matches!(self, Self::Forecast(_, _))
    }
}

struct RadarLayerState {
    show_base: bool,
    active: RadarLayer,
    views: Vec<RadarFrameWidget>,
    legends: Vec<gtk::Label>,
}

impl Clone for RadarFrameWidget {
    fn clone(&self) -> Self {
        Self {
            root: self.root.clone(),
            base: self.base.clone(),
            precipitation: self.precipitation.clone(),
            temperature: self.temperature.clone(),
            air_quality: self.air_quality.clone(),
            wind: self.wind.clone(),
        }
    }
}

fn update_radar_layer_views(state: &RadarLayerState) {
    for view in &state.views {
        view.base.set_visible(state.show_base);
        view.precipitation
            .set_visible(state.active == RadarLayer::Precipitation);
        view.temperature
            .set_visible(state.active == RadarLayer::Temperature);
        view.air_quality
            .set_visible(state.active == RadarLayer::AirQuality);
        view.wind.set_visible(state.active == RadarLayer::Wind);
    }
    let (text, tooltip) = match state.active {
        RadarLayer::Precipitation => ("Light     Moderate     Heavy", "Precipitation intensity"),
        RadarLayer::Temperature => ("Cooler     Temperature     Warmer", "Temperature layer"),
        RadarLayer::AirQuality => ("Good     Moderate     Poor", "Air quality index"),
        RadarLayer::Wind => ("Light     Wind speed     Strong", "Wind speed layer"),
    };
    for legend in &state.legends {
        legend.set_text(text);
        legend.set_tooltip_text(Some(tooltip));
    }
}

fn radar_layers_button() -> (gtk::MenuButton, Rc<RefCell<RadarLayerState>>) {
    let state = Rc::new(RefCell::new(RadarLayerState {
        show_base: true,
        active: RadarLayer::Precipitation,
        views: Vec::new(),
        legends: Vec::new(),
    }));
    let button = gtk::MenuButton::builder()
        .icon_name("view-grid-symbolic")
        .tooltip_text("Choose radar layers")
        .build();
    button.add_css_class("layer-button");
    button.set_width_request(40);
    button.set_height_request(38);
    button.set_hexpand(false);
    button.set_halign(Align::Center);
    let popover = gtk::Popover::new();
    let content = gtk::Box::new(Orientation::Vertical, 10);
    content.add_css_class("layer-menu");
    content.set_margin_top(12);
    content.set_margin_bottom(12);
    content.set_margin_start(12);
    content.set_margin_end(12);
    let heading = gtk::Label::new(Some("MAP LAYERS"));
    heading.add_css_class("section-title");
    heading.set_xalign(0.0);
    content.append(&heading);
    let base_row = gtk::Box::new(Orientation::Horizontal, 16);
    let base_label = gtk::Label::new(Some("Base map"));
    base_label.set_xalign(0.0);
    base_label.set_hexpand(true);
    let base_toggle = gtk::Switch::builder()
        .active(true)
        .valign(Align::Center)
        .build();
    {
        let layer_state = state.clone();
        base_toggle.connect_active_notify(move |toggle| {
            let mut state = layer_state.borrow_mut();
            state.show_base = toggle.is_active();
            update_radar_layer_views(&state);
        });
    }
    base_row.append(&base_label);
    base_row.append(&base_toggle);
    content.append(&base_row);
    let mut first: Option<gtk::CheckButton> = None;
    for (name, kind) in [
        ("Precipitation", RadarLayer::Precipitation),
        ("Temperature", RadarLayer::Temperature),
        ("Air quality", RadarLayer::AirQuality),
        ("Wind", RadarLayer::Wind),
    ] {
        let row = gtk::Box::new(Orientation::Horizontal, 16);
        let label = gtk::Label::new(Some(name));
        label.set_xalign(0.0);
        label.set_hexpand(true);
        let toggle = gtk::CheckButton::new();
        if let Some(first) = &first {
            toggle.set_group(Some(first));
        } else {
            toggle.set_active(true);
            first = Some(toggle.clone());
        }
        let layer_state = state.clone();
        toggle.connect_toggled(move |toggle| {
            if toggle.is_active() {
                let mut state = layer_state.borrow_mut();
                state.active = kind;
                update_radar_layer_views(&state);
            }
        });
        row.append(&label);
        row.append(&toggle);
        content.append(&row);
    }
    let note = gtk::Label::new(Some("Radar: RainViewer · Forecast layers: Open-Meteo"));
    note.add_css_class("dim-label");
    note.set_wrap(true);
    note.set_max_width_chars(28);
    note.set_xalign(0.0);
    content.append(&note);
    popover.set_child(Some(&content));
    button.set_popover(Some(&popover));
    (button, state)
}

fn radar_card(location: &SavedLocation, parent: &adw::ApplicationWindow) -> gtk::Box {
    let card = gtk::Box::new(Orientation::Vertical, 4);
    card.add_css_class("radar-card");
    card.set_vexpand(true);
    let heading = gtk::Box::new(Orientation::Horizontal, 3);
    let title = gtk::Label::new(Some("◉  RADAR"));
    title.add_css_class("section-title");
    title.set_xalign(0.0);
    title.set_hexpand(true);
    let play = gtk::ToggleButton::with_label("▶");
    play.add_css_class("pill");
    play.set_tooltip_text(Some("Play recent radar frames"));
    let zoom_out = gtk::Button::with_label("−");
    zoom_out.add_css_class("pill");
    zoom_out.set_tooltip_text(Some("Zoom radar out"));
    let zoom_in = gtk::Button::with_label("+");
    zoom_in.add_css_class("pill");
    zoom_in.set_tooltip_text(Some("Zoom radar in"));
    let open = gtk::Button::with_label("⛶");
    open.add_css_class("pill");
    open.set_tooltip_text(Some("Open the full radar map"));
    let (layers, layer_state) = radar_layers_button();
    heading.append(&title);
    heading.append(&play);
    heading.append(&zoom_out);
    heading.append(&zoom_in);
    heading.append(&layers);
    heading.append(&open);
    card.append(&heading);
    let (map, map_stack) = radar_viewport();
    map.set_height_request(320);
    map.set_hexpand(true);
    map.set_vexpand(true);
    map.add_css_class("radar-preview");
    let status = gtk::Label::new(Some("Loading latest radar…"));
    status.add_css_class("radar-time");
    status.set_ellipsize(gtk::pango::EllipsizeMode::End);
    card.append(&status);
    card.append(&map);
    let legend = gtk::Label::new(Some("Light     Moderate     Heavy"));
    legend.add_css_class("radar-legend");
    legend.set_tooltip_text(Some("Radar precipitation intensity"));
    card.append(&legend);
    layer_state.borrow_mut().legends.push(legend);
    let state = Rc::new(RefCell::new((
        location.latitude,
        location.longitude,
        6_u8,
        0_u64,
    )));
    let radar_timezone: Tz = location.timezone.parse().unwrap_or(chrono_tz::UTC);
    let reload: Rc<dyn Fn()> = {
        let map = map.clone();
        let map_stack = map_stack.clone();
        let status = status.clone();
        let state = state.clone();
        let play = play.clone();
        let layer_state = layer_state.clone();
        Rc::new(move || {
            load_radar_map(
                &map,
                &map_stack,
                &status,
                &state,
                1,
                &play,
                radar_timezone,
                &layer_state,
            )
        })
    };
    {
        let state = state.clone();
        let reload = reload.clone();
        let zoom_in_control = zoom_in.clone();
        zoom_out.connect_clicked(move |button| {
            let zoom = radar_zoom_out(state.borrow().2);
            state.borrow_mut().2 = zoom;
            button.set_sensitive(zoom > 2);
            zoom_in_control.set_sensitive(true);
            reload();
        });
    }
    {
        let state = state.clone();
        let reload = reload.clone();
        let zoom_in_control = zoom_in.clone();
        let zoom_out_control = zoom_out.clone();
        zoom_in.connect_clicked(move |_| {
            let zoom = radar_zoom_in(state.borrow().2);
            state.borrow_mut().2 = zoom;
            zoom_in_control.set_sensitive(zoom < 7);
            zoom_out_control.set_sensitive(true);
            reload();
        });
    }
    {
        let play = play.clone();
        play.connect_toggled(move |button| {
            button.set_label(if button.is_active() { "Ⅱ" } else { "▶" });
        });
    }
    {
        let map = map.clone();
        let reload = reload.clone();
        glib::timeout_add_local_once(std::time::Duration::from_millis(350), move || {
            if map.root().is_some() {
                reload();
            }
        });
    }
    {
        let location = location.clone();
        let parent = parent.clone();
        open.connect_clicked(move |_| show_radar_map(&parent, &location));
    }
    card
}

fn show_radar_map(parent: &adw::ApplicationWindow, location: &SavedLocation) {
    let window = adw::Window::builder()
        .title(format!("{} Radar", location.display_name))
        .transient_for(parent)
        .modal(true)
        .default_width(860)
        .default_height(680)
        .build();
    let toolbar = adw::ToolbarView::new();
    let header = adw::HeaderBar::new();
    let title = gtk::Label::new(Some(&format!("{} · Radar", location.display_name)));
    title.add_css_class("title");
    header.set_title_widget(Some(&title));
    toolbar.add_top_bar(&header);
    let root = gtk::Box::new(Orientation::Vertical, 8);
    root.set_margin_start(10);
    root.set_margin_end(10);
    root.set_margin_bottom(10);
    let controls = gtk::Box::new(Orientation::Horizontal, 5);
    controls.set_halign(Align::Center);
    let left = widgets::icon_button("go-previous-symbolic", "Pan west");
    let up = widgets::icon_button("go-up-symbolic", "Pan north");
    let down = widgets::icon_button("go-down-symbolic", "Pan south");
    let right = widgets::icon_button("go-next-symbolic", "Pan east");
    let zoom_out = gtk::Button::with_label("−");
    zoom_out.set_tooltip_text(Some("Zoom out"));
    let zoom_in = gtk::Button::with_label("+");
    zoom_in.set_tooltip_text(Some("Zoom in"));
    let play = gtk::ToggleButton::with_label("▶ Play");
    play.set_tooltip_text(Some("Animate recent radar frames"));
    let (layers, layer_state) = radar_layers_button();
    for button in [&left, &up, &down, &right, &zoom_out, &zoom_in] {
        controls.append(button);
    }
    controls.append(&layers);
    controls.append(&play);
    let (map, map_stack) = radar_viewport();
    map.set_vexpand(true);
    map.add_css_class("radar-full");
    let status = gtk::Label::new(Some("Loading latest radar…"));
    status.add_css_class("dim-label");
    let credit = gtk::LinkButton::with_label(
        "https://www.rainviewer.com/",
        "Radar © RainViewer · Map © OpenStreetMap contributors",
    );
    credit.set_halign(Align::Center);
    credit.add_css_class("weather-attribution");
    root.append(&controls);
    root.append(&map);
    root.append(&status);
    root.append(&credit);
    toolbar.set_content(Some(&root));
    window.set_content(Some(&toolbar));
    let state = Rc::new(RefCell::new((
        location.latitude,
        location.longitude,
        6_u8,
        0_u64,
    )));
    let radar_timezone: Tz = location.timezone.parse().unwrap_or(chrono_tz::UTC);
    let reload: Rc<dyn Fn()> = {
        let map = map.clone();
        let map_stack = map_stack.clone();
        let status = status.clone();
        let state = state.clone();
        let play = play.clone();
        let layer_state = layer_state.clone();
        Rc::new(move || {
            load_radar_map(
                &map,
                &map_stack,
                &status,
                &state,
                1,
                &play,
                radar_timezone,
                &layer_state,
            )
        })
    };
    for (button, dx, dy) in [(&left, -1, 0), (&up, 0, -1), (&down, 0, 1), (&right, 1, 0)] {
        let state = state.clone();
        let reload = reload.clone();
        button.connect_clicked(move |_| {
            let (lat, lon, zoom, generation) = *state.borrow();
            let (lat, lon) = crate::radar::nudge(lat, lon, zoom, dx, dy);
            *state.borrow_mut() = (lat, lon, zoom, generation);
            reload();
        });
    }
    {
        let state = state.clone();
        let reload = reload.clone();
        let zoom_in_control = zoom_in.clone();
        zoom_out.connect_clicked(move |button| {
            let zoom = radar_zoom_out(state.borrow().2);
            state.borrow_mut().2 = zoom;
            button.set_sensitive(zoom > 2);
            zoom_in_control.set_sensitive(true);
            reload();
        });
    }
    {
        let state = state.clone();
        let reload_map = reload.clone();
        let zoom_in_control = zoom_in.clone();
        let zoom_out_control = zoom_out.clone();
        zoom_in.connect_clicked(move |_| {
            let zoom = radar_zoom_in(state.borrow().2);
            state.borrow_mut().2 = zoom;
            zoom_in_control.set_sensitive(zoom < 7);
            zoom_out_control.set_sensitive(true);
            reload_map();
        });
    }
    play.connect_toggled(move |button| {
        button.set_label(if button.is_active() {
            "Ⅱ Pause"
        } else {
            "▶ Play"
        });
    });
    reload();
    window.present();
}

fn load_radar_map(
    container: &gtk::ScrolledWindow,
    frame_stack: &gtk::Stack,
    status: &gtk::Label,
    state: &Rc<RefCell<(f64, f64, u8, u64)>>,
    radius: i32,
    play: &gtk::ToggleButton,
    timezone: Tz,
    layer_state: &Rc<RefCell<RadarLayerState>>,
) {
    clear_stack(frame_stack);
    let spinner = gtk::Spinner::new();
    spinner.start();
    spinner.set_halign(Align::Center);
    spinner.set_valign(Align::Center);
    spinner.set_vexpand(true);
    frame_stack.add_named(&spinner, Some("loading"));
    frame_stack.set_visible_child_name("loading");
    let (latitude, longitude, zoom, generation) = {
        let mut current = state.borrow_mut();
        current.3 = current.3.wrapping_add(1);
        *current
    };
    status.set_text("Loading radar…");
    let (tx, rx) = async_channel::bounded(1);
    RUNTIME.spawn(async move {
        let radar = async {
            match RadarClient::new() {
                Ok(client) => client.animation(latitude, longitude, zoom, radius, 3).await,
                Err(error) => Err(error),
            }
        };
        let weather_layers = async {
            match MapLayerClient::new() {
                Ok(client) => client.fetch(latitude, longitude, zoom, radius).await,
                Err(error) => Err(error),
            }
        };
        let (radar, weather_layers) = tokio::join!(radar, weather_layers);
        let response = (
            radar.map_err(|error| error.to_string()),
            weather_layers.ok(),
        );
        let _ = tx.send(response).await;
    });
    let container = container.clone();
    let frame_stack = frame_stack.clone();
    let status = status.clone();
    let play = play.clone();
    let request_state = state.clone();
    let layer_state = layer_state.clone();
    glib::spawn_future_local(async move {
        let response = rx.recv().await;
        if request_state.borrow().3 != generation {
            return;
        }
        match response {
            Ok((Ok(radars), weather_layers)) => {
                let weather_layers = weather_layers.map(Arc::new);
                let Some(first) = radars.first() else {
                    status.set_text("Radar returned no displayable frames");
                    return;
                };
                let focus_x = first.focus_x;
                let focus_y = first.focus_y;
                let Some(base_texture) = radar_base_texture(&first.tiles, radius) else {
                    status.set_text("Radar base map could not be decoded");
                    return;
                };
                let mut playback = radars
                    .into_iter()
                    .filter_map(|radar| {
                        radar_overlay_texture(radar.tiles, radius)
                            .map(|texture| RadarPlaybackFrame::Observed(radar.observed_at, texture))
                    })
                    .collect::<Vec<_>>();
                if playback.is_empty() {
                    status.set_text("Radar returned no displayable frames");
                    return;
                }
                let initial_index = playback.len() - 1;
                if let Some(data) = weather_layers.clone() {
                    for forecast_index in 0..data.precipitation_forecast.len() {
                        let at = data.precipitation_forecast[forecast_index].at;
                        playback.push(RadarPlaybackFrame::Forecast(at, forecast_index));
                    }
                }
                let playback = Rc::new(playback);
                let forecast_index = Rc::new(RefCell::new(0_usize));
                let latest_texture = match &playback[initial_index] {
                    RadarPlaybackFrame::Observed(_, texture) => texture,
                    RadarPlaybackFrame::Forecast(_, _) => unreachable!(),
                };
                let (display, radar_picture, forecast_picture) = radar_display(
                    &base_texture,
                    latest_texture,
                    weather_layers,
                    forecast_index.clone(),
                    radius,
                    focus_x,
                    focus_y,
                );
                let index = Rc::new(RefCell::new(initial_index));
                clear_stack(&frame_stack);
                {
                    let mut layers = layer_state.borrow_mut();
                    layers.views = vec![display.clone()];
                    update_radar_layer_views(&layers);
                }
                frame_stack.add_named(&display.root, Some("map"));
                frame_stack.set_visible_child_name("map");
                center_radar_view(&container, focus_x, focus_y);
                status.set_text(&radar_frame_time(
                    playback[*index.borrow()].time(),
                    timezone,
                    playback[*index.borrow()].is_forecast(),
                ));
                container.queue_allocate();
                container.queue_draw();
                status.queue_draw();
                if let Some(root) = container.root() {
                    root.queue_allocate();
                    root.queue_draw();
                }
                if let Some(display) = gdk::Display::default() {
                    display.flush();
                }
                let container_timer = container.clone();
                let status_timer = status.clone();
                let playback_timer = playback.clone();
                let index_timer = index.clone();
                let play_timer = play.clone();
                let state_timer = request_state.clone();
                let radar_picture_timer = radar_picture.clone();
                let forecast_picture_timer = forecast_picture.clone();
                let forecast_index_timer = forecast_index.clone();
                glib::timeout_add_local(std::time::Duration::from_millis(850), move || {
                    if container_timer.root().is_none() || state_timer.borrow().3 != generation {
                        return glib::ControlFlow::Break;
                    }
                    if !play_timer.is_active() {
                        return glib::ControlFlow::Continue;
                    }
                    let next = (*index_timer.borrow() + 1) % playback_timer.len();
                    *index_timer.borrow_mut() = next;
                    match &playback_timer[next] {
                        RadarPlaybackFrame::Observed(_, texture) => {
                            radar_picture_timer.set_paintable(Some(texture));
                            radar_picture_timer.set_visible(true);
                            forecast_picture_timer.set_visible(false);
                        }
                        RadarPlaybackFrame::Forecast(_, frame_index) => {
                            *forecast_index_timer.borrow_mut() = *frame_index;
                            forecast_picture_timer.queue_draw();
                            radar_picture_timer.set_visible(false);
                            forecast_picture_timer.set_visible(true);
                        }
                    }
                    status_timer.set_text(&radar_frame_time(
                        playback_timer[next].time(),
                        timezone,
                        playback_timer[next].is_forecast(),
                    ));
                    glib::ControlFlow::Continue
                });
            }
            Ok((Err(error), _)) => {
                clear_stack(&frame_stack);
                let message = gtk::Label::new(Some(&error));
                message.set_wrap(true);
                message.set_halign(Align::Center);
                message.set_valign(Align::Center);
                message.set_vexpand(true);
                frame_stack.add_named(&message, Some("error"));
                frame_stack.set_visible_child_name("error");
                status.set_text("Radar unavailable — retry by reopening the map");
            }
            Err(_) => {}
        }
    });
}

fn radar_zoom_in(zoom: u8) -> u8 {
    zoom.saturating_add(1).min(7)
}

fn radar_zoom_out(zoom: u8) -> u8 {
    zoom.saturating_sub(1).max(2)
}

fn radar_viewport() -> (gtk::ScrolledWindow, gtk::Stack) {
    let viewport = gtk::ScrolledWindow::builder()
        // External keeps the child scrollable without allocating visible bars.
        .hscrollbar_policy(gtk::PolicyType::External)
        .vscrollbar_policy(gtk::PolicyType::External)
        // Panning is an explicit primary-button drag.  In particular, a
        // trackpad scroll must not move the map underneath the pointer.
        .kinetic_scrolling(false)
        .propagate_natural_width(false)
        .propagate_natural_height(false)
        .build();
    let drag_origin = Rc::new(RefCell::new(None::<(f64, f64, f64, f64)>));
    let press = gtk::GestureClick::new();
    press.set_button(1);
    press.set_propagation_phase(gtk::PropagationPhase::Capture);
    press.set_exclusive(true);
    {
        let viewport = viewport.clone();
        let drag_origin = drag_origin.clone();
        press.connect_pressed(move |gesture, _, x, y| {
            gesture.set_state(gtk::EventSequenceState::Claimed);
            *drag_origin.borrow_mut() = Some((
                x,
                y,
                viewport.hadjustment().value(),
                viewport.vadjustment().value(),
            ));
            viewport.set_cursor_from_name(Some("grabbing"));
        });
    }
    {
        let viewport = viewport.clone();
        let drag_origin = drag_origin.clone();
        let motion = gtk::EventControllerMotion::new();
        motion.set_propagation_phase(gtk::PropagationPhase::Capture);
        let viewport_motion = viewport.clone();
        motion.connect_motion(move |_, x, y| {
            if let Some((pointer_x, pointer_y, scroll_x, scroll_y)) = *drag_origin.borrow() {
                viewport_motion
                    .hadjustment()
                    .set_value(scroll_x + pointer_x - x);
                viewport_motion
                    .vadjustment()
                    .set_value(scroll_y + pointer_y - y);
            }
        });
        viewport.add_controller(motion);
    }
    {
        let viewport = viewport.clone();
        press.connect_released(move |_, _, _, _| {
            *drag_origin.borrow_mut() = None;
            viewport.set_cursor_from_name(Some("grab"));
        });
    }
    viewport.add_controller(press);
    let wheel = gtk::EventControllerScroll::new(gtk::EventControllerScrollFlags::BOTH_AXES);
    wheel.set_propagation_phase(gtk::PropagationPhase::Capture);
    wheel.connect_scroll(|_, _, _| glib::Propagation::Stop);
    viewport.add_controller(wheel);
    viewport.set_cursor_from_name(Some("grab"));
    viewport.set_tooltip_text(Some("Drag to move around the radar map"));
    let stack = gtk::Stack::new();
    stack.set_transition_type(gtk::StackTransitionType::None);
    stack.set_hexpand(true);
    stack.set_vexpand(true);
    viewport.set_child(Some(&stack));
    (viewport, stack)
}

fn clear_stack(stack: &gtk::Stack) {
    while let Some(child) = stack.first_child() {
        stack.remove(&child);
    }
}

fn center_radar_view(viewport: &gtk::ScrolledWindow, focus_x: f64, focus_y: f64) {
    let viewport = viewport.clone();
    glib::timeout_add_local_once(std::time::Duration::from_millis(80), move || {
        let horizontal = viewport.hadjustment();
        let vertical = viewport.vadjustment();
        horizontal.set_value(focus_x - horizontal.page_size() / 2.0);
        vertical.set_value(focus_y - vertical.page_size() / 2.0);
    });
}

fn radar_base_texture(tiles: &[crate::radar::RadarTile], radius: i32) -> Option<gdk::Texture> {
    use gtk::gdk_pixbuf::{Colorspace, InterpType, Pixbuf};

    const TILE_SIZE: i32 = 512;
    let mosaic_size = (radius * 2 + 1) * TILE_SIZE;
    let mosaic = Pixbuf::new(Colorspace::Rgb, false, 8, mosaic_size, mosaic_size)?;
    mosaic.fill(0x131a2eff);
    for tile in tiles {
        let base = Pixbuf::from_read(Cursor::new(tile.base_png.clone())).ok()?;
        let base = base.scale_simple(TILE_SIZE, TILE_SIZE, InterpType::Bilinear)?;
        let x = (tile.column + radius) * TILE_SIZE;
        let y = (tile.row + radius) * TILE_SIZE;
        base.copy_area(0, 0, TILE_SIZE, TILE_SIZE, &mosaic, x, y);
    }
    Some(gdk::Texture::for_pixbuf(&mosaic))
}

fn radar_overlay_texture(tiles: Vec<crate::radar::RadarTile>, radius: i32) -> Option<gdk::Texture> {
    use gtk::gdk_pixbuf::{Colorspace, InterpType, Pixbuf};

    const TILE_SIZE: i32 = 512;
    let tile_count = radius * 2 + 1;
    let mosaic_size = tile_count * TILE_SIZE;
    let radar_mosaic = Pixbuf::new(Colorspace::Rgb, true, 8, mosaic_size, mosaic_size)?;
    radar_mosaic.fill(0x00000000);
    for tile in tiles {
        let radar = Pixbuf::from_read(Cursor::new(tile.radar_png)).ok()?;
        let radar = if radar.width() == TILE_SIZE && radar.height() == TILE_SIZE {
            radar
        } else {
            radar.scale_simple(TILE_SIZE, TILE_SIZE, InterpType::Bilinear)?
        };
        let x = (tile.column + radius) * TILE_SIZE;
        let y = (tile.row + radius) * TILE_SIZE;
        radar.copy_area(0, 0, TILE_SIZE, TILE_SIZE, &radar_mosaic, x, y);
    }
    Some(gdk::Texture::for_pixbuf(&radar_mosaic))
}

fn radar_display(
    base_texture: &gdk::Texture,
    radar_texture: &gdk::Texture,
    weather_layers: Option<Arc<MapLayerData>>,
    forecast_index: Rc<RefCell<usize>>,
    radius: i32,
    focus_x: f64,
    focus_y: f64,
) -> (RadarFrameWidget, gtk::Picture, gtk::DrawingArea) {
    const TILE_SIZE: i32 = 512;
    let mosaic_size = (radius * 2 + 1) * TILE_SIZE;
    let base_picture = gtk::Picture::for_paintable(base_texture);
    base_picture.set_content_fit(gtk::ContentFit::Fill);
    base_picture.set_can_shrink(false);
    base_picture.set_size_request(mosaic_size, mosaic_size);
    let radar_picture = gtk::Picture::for_paintable(radar_texture);
    radar_picture.set_content_fit(gtk::ContentFit::Fill);
    radar_picture.set_can_shrink(false);
    radar_picture.set_size_request(mosaic_size, mosaic_size);
    let forecast_picture =
        forecast_precipitation_widget(weather_layers.clone(), forecast_index, mosaic_size);
    forecast_picture.set_visible(false);
    let precipitation = gtk::Fixed::new();
    precipitation.set_size_request(mosaic_size, mosaic_size);
    precipitation.put(&radar_picture, 0.0, 0.0);
    precipitation.put(&forecast_picture, 0.0, 0.0);
    let temperature =
        weather_layer_widget(weather_layers.clone(), RadarLayer::Temperature, mosaic_size);
    let air_quality =
        weather_layer_widget(weather_layers.clone(), RadarLayer::AirQuality, mosaic_size);
    let wind = weather_layer_widget(weather_layers, RadarLayer::Wind, mosaic_size);
    let fixed = gtk::Fixed::new();
    fixed.set_size_request(mosaic_size, mosaic_size);
    fixed.put(&base_picture, 0.0, 0.0);
    fixed.put(&precipitation, 0.0, 0.0);
    fixed.put(&temperature, 0.0, 0.0);
    fixed.put(&air_quality, 0.0, 0.0);
    fixed.put(&wind, 0.0, 0.0);
    let marker = gtk::Label::new(Some("●"));
    marker.add_css_class("radar-marker");
    fixed.put(&marker, focus_x - 10.0, focus_y - 10.0);
    let display = RadarFrameWidget {
        root: fixed,
        base: base_picture.upcast(),
        precipitation: precipitation.upcast(),
        temperature: temperature.upcast(),
        air_quality: air_quality.upcast(),
        wind: wind.upcast(),
    };
    (display, radar_picture, forecast_picture)
}

fn forecast_precipitation_widget(
    data: Option<Arc<MapLayerData>>,
    frame_index: Rc<RefCell<usize>>,
    mosaic_size: i32,
) -> gtk::DrawingArea {
    let drawing = gtk::DrawingArea::new();
    drawing.set_size_request(mosaic_size, mosaic_size);
    drawing.set_draw_func(move |_, context, width, height| {
        let Some(data) = &data else { return };
        let Some(frame) = data.precipitation_forecast.get(*frame_index.borrow()) else {
            return;
        };
        const CELLS: usize = 32;
        let cell_width = width as f64 / CELLS as f64;
        let cell_height = height as f64 / CELLS as f64;
        for row in 0..CELLS {
            for column in 0..CELLS {
                let u = (column as f64 + 0.5) / CELLS as f64;
                let v = (row as f64 + 0.5) / CELLS as f64;
                let amount = interpolate_grid(&frame.millimetres, data.width, data.height, u, v)
                    .unwrap_or(0.0);
                let probability =
                    interpolate_grid(&frame.probability, data.width, data.height, u, v)
                        .unwrap_or(0.0);
                if amount <= 0.01 && probability < 5.0 {
                    continue;
                }
                let strength = (amount / 5.0).clamp(0.0, 1.0);
                let (red, green, blue) = if strength < 0.5 {
                    let mix = strength * 2.0;
                    (0.05 + mix * 0.90, 0.55 + mix * 0.35, 1.0 - mix * 0.75)
                } else {
                    let mix = (strength - 0.5) * 2.0;
                    (0.95, 0.90 - mix * 0.75, 0.25 - mix * 0.10)
                };
                let alpha = (0.18 + probability / 100.0 * 0.55).clamp(0.18, 0.73);
                context.set_source_rgba(red, green, blue, alpha);
                context.rectangle(
                    column as f64 * cell_width,
                    row as f64 * cell_height,
                    cell_width + 1.0,
                    cell_height + 1.0,
                );
                let _ = context.fill();
            }
        }
    });
    drawing
}

fn radar_frame_time(at: chrono::DateTime<Utc>, timezone: Tz, forecast: bool) -> String {
    format!(
        "{} · {}",
        at.with_timezone(&timezone).format("%-I:%M %p"),
        if forecast { "Forecast" } else { "Radar" }
    )
}

fn weather_layer_widget(
    data: Option<Arc<MapLayerData>>,
    layer: RadarLayer,
    mosaic_size: i32,
) -> gtk::DrawingArea {
    let drawing = gtk::DrawingArea::new();
    drawing.set_size_request(mosaic_size, mosaic_size);
    drawing.set_draw_func(move |_, context, width, height| {
        let Some(data) = &data else { return };
        let values = match layer {
            RadarLayer::Temperature => &data.temperature_c,
            RadarLayer::AirQuality => &data.air_quality,
            RadarLayer::Wind => &data.wind_kmh,
            RadarLayer::Precipitation => return,
        };
        const CELLS: usize = 32;
        let cell_width = width as f64 / CELLS as f64;
        let cell_height = height as f64 / CELLS as f64;
        for row in 0..CELLS {
            for column in 0..CELLS {
                let u = (column as f64 + 0.5) / CELLS as f64;
                let v = (row as f64 + 0.5) / CELLS as f64;
                let Some(value) = interpolate_grid(values, data.width, data.height, u, v) else {
                    continue;
                };
                let (red, green, blue) = weather_layer_color(layer, value);
                context.set_source_rgba(red, green, blue, 0.46);
                context.rectangle(
                    column as f64 * cell_width,
                    row as f64 * cell_height,
                    cell_width + 1.0,
                    cell_height + 1.0,
                );
                let _ = context.fill();
            }
        }
        context.select_font_face(
            "Ubuntu Sans",
            gtk::cairo::FontSlant::Normal,
            gtk::cairo::FontWeight::Bold,
        );
        context.set_font_size(18.0);
        for row in 0..data.height {
            for column in 0..data.width {
                let index = row * data.width + column;
                let Some(value) = values.get(index).copied().flatten() else {
                    continue;
                };
                let x = (column as f64 / (data.width - 1) as f64 * width as f64)
                    .clamp(28.0, width as f64 - 45.0);
                let y = (row as f64 / (data.height - 1) as f64 * height as f64)
                    .clamp(28.0, height as f64 - 28.0);
                context.set_source_rgba(0.02, 0.04, 0.10, 0.80);
                context.rectangle(x - 22.0, y - 17.0, 48.0, 27.0);
                let _ = context.fill();
                context.set_source_rgb(1.0, 1.0, 1.0);
                let label = match layer {
                    RadarLayer::Temperature => format!("{value:.0}°"),
                    RadarLayer::AirQuality => format!("{value:.0}"),
                    RadarLayer::Wind => format!("{value:.0}"),
                    RadarLayer::Precipitation => String::new(),
                };
                context.move_to(x - 17.0, y + 3.0);
                let _ = context.show_text(&label);
                if layer == RadarLayer::Wind {
                    let direction = data
                        .wind_direction
                        .get(index)
                        .copied()
                        .flatten()
                        .unwrap_or(0.0)
                        .to_radians();
                    let dx = direction.sin() * 24.0;
                    let dy = -direction.cos() * 24.0;
                    context.set_line_width(4.0);
                    context.move_to(x, y + 16.0);
                    context.line_to(x + dx, y + 16.0 + dy);
                    let _ = context.stroke();
                }
            }
        }
    });
    drawing
}

fn interpolate_grid(
    values: &[Option<f64>],
    width: usize,
    height: usize,
    u: f64,
    v: f64,
) -> Option<f64> {
    if width < 2 || height < 2 {
        return values.first().copied().flatten();
    }
    let x = u.clamp(0.0, 1.0) * (width - 1) as f64;
    let y = v.clamp(0.0, 1.0) * (height - 1) as f64;
    let x0 = x.floor() as usize;
    let y0 = y.floor() as usize;
    let x1 = (x0 + 1).min(width - 1);
    let y1 = (y0 + 1).min(height - 1);
    let candidates = [
        (
            values[y0 * width + x0],
            (1.0 - x.fract()) * (1.0 - y.fract()),
        ),
        (values[y0 * width + x1], x.fract() * (1.0 - y.fract())),
        (values[y1 * width + x0], (1.0 - x.fract()) * y.fract()),
        (values[y1 * width + x1], x.fract() * y.fract()),
    ];
    let (total, weight) = candidates
        .into_iter()
        .filter_map(|(value, weight)| value.map(|value| (value * weight, weight)))
        .fold((0.0, 0.0), |(total, weights), (value, weight)| {
            (total + value, weights + weight)
        });
    (weight > 0.0).then_some(total / weight)
}

fn weather_layer_color(layer: RadarLayer, value: f64) -> (f64, f64, f64) {
    let normalized = match layer {
        RadarLayer::Temperature => ((value + 30.0) / 75.0).clamp(0.0, 1.0),
        RadarLayer::AirQuality => (value / 250.0).clamp(0.0, 1.0),
        RadarLayer::Wind => (value / 80.0).clamp(0.0, 1.0),
        RadarLayer::Precipitation => 0.0,
    };
    if normalized < 0.5 {
        let amount = normalized * 2.0;
        match layer {
            RadarLayer::AirQuality => (amount, 0.78, 0.20),
            RadarLayer::Wind => (0.10, 0.72 - amount * 0.20, 0.95),
            _ => (
                0.10 + amount * 0.85,
                0.60 + amount * 0.25,
                1.0 - amount * 0.75,
            ),
        }
    } else {
        let amount = (normalized - 0.5) * 2.0;
        match layer {
            RadarLayer::AirQuality => (0.95, 0.78 - amount * 0.65, 0.12 + amount * 0.30),
            RadarLayer::Wind => (0.20 + amount * 0.65, 0.52 - amount * 0.32, 0.95),
            _ => (0.95, 0.85 - amount * 0.72, 0.25 - amount * 0.12),
        }
    }
}

fn refresh_selected(
    state: &Rc<RefCell<UiState>>,
    views: &ViewRefs,
    store: &LocationStore,
    force: bool,
) {
    let (s_location, settings) = {
        let s = state.borrow();
        let Some(id) = &s.selected else { return };
        let Some(l) = s.locations.iter().find(|l| &l.id == id) else {
            return;
        };
        (l.clone(), s.settings.clone())
    };
    if settings.weather_provider == "weatherkit" && settings.credentials.team_id.is_empty() {
        toast(
            views,
            "Demo data is active — add WeatherKit credentials in Settings",
        );
        return;
    }
    let id = s_location.id.clone();
    let (tx, rx) = async_channel::bounded(1);
    RUNTIME.spawn(async move {
        let provider_name = settings.weather_provider.clone();
        let (client, attribution, mark_path): (
            Arc<dyn WeatherProvider>,
            Option<Attribution>,
            Option<std::path::PathBuf>,
        ) = if settings.weather_provider == "weatherkit" {
            let token = Arc::new(LocalJwtProvider {
                metadata: settings.credentials,
                secrets: GnomeSecretStore,
            });
            let weatherkit = match WeatherKitClient::new(token) {
                Ok(c) => Arc::new(c),
                Err(e) => {
                    let _ = tx.send(Err(e.to_string())).await;
                    return;
                }
            };
            let attribution = weatherkit.attribution("en-US").await.ok();
            let mark_path = if let Some(a) = &attribution {
                if let Ok(bytes) = weatherkit.attribution_mark(a, false).await {
                    if let Some(d) =
                        directories::ProjectDirs::from("io", "Weatherglass", "Weatherglass")
                    {
                        let path = d.cache_dir().join("apple-weather-attribution.png");
                        let _ = tokio::fs::create_dir_all(d.cache_dir()).await;
                        if tokio::fs::write(&path, bytes).await.is_ok() {
                            Some(path)
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                } else {
                    None
                }
            } else {
                None
            };
            (weatherkit, attribution, mark_path)
        } else {
            let open_meteo = match OpenMeteoClient::new() {
                Ok(client) => Arc::new(client),
                Err(e) => {
                    let _ = tx.send(Err(e.to_string())).await;
                    return;
                }
            };
            let attribution = open_meteo.attribution("en-US").await.ok();
            (open_meteo, attribution, None)
        };
        let cache = match ForecastCache::xdg() {
            Ok(c) => c.scoped(&provider_name),
            Err(e) => {
                let _ = tx.send(Err(e.to_string())).await;
                return;
            }
        };
        let coordinator = RefreshCoordinator::new(client, cache);
        let result = coordinator
            .refresh(&s_location, force)
            .await
            .map(|r| (r, attribution, mark_path));
        let _ = tx.send(result.map_err(|e| e.to_string())).await;
    });
    let s = state.clone();
    let v = views.clone();
    let store_for_render = store.clone();
    glib::spawn_future_local(async move {
        match rx.recv().await {
            Ok(Ok((result, attribution, mark_path))) => {
                let mut state = s.borrow_mut();
                let (status, data) = match result {
                    ForecastResult::Fresh(w) => ("Live forecast updated", w),
                    ForecastResult::Cached {
                        entry,
                        stale: false,
                    } => ("Using valid cached forecast", entry.data),
                    ForecastResult::Cached { entry, stale: true } => {
                        ("Offline — showing expired cached forecast", entry.data)
                    }
                };
                state.weather.insert(id, data);
                state.demo = false;
                state.attribution = attribution;
                state.attribution_mark = mark_path;
                drop(state);
                render_all(&s, &v, &store_for_render);
                toast(&v, status)
            }
            Ok(Err(e)) => toast(&v, &e),
            Err(_) => {}
        }
    });
}

fn show_search(state: &Rc<RefCell<UiState>>, views: &ViewRefs, store: &LocationStore) {
    let dialog = adw::Window::builder()
        .title("Add Location")
        .transient_for(&views.window)
        .modal(true)
        .default_width(560)
        .default_height(560)
        .build();
    let root = gtk::Box::new(Orientation::Vertical, 12);
    root.set_margin_start(18);
    root.set_margin_end(18);
    root.set_margin_top(18);
    root.set_margin_bottom(18);
    let title = gtk::Label::new(Some("Find a city or enter coordinates"));
    title.add_css_class("title-2");
    title.set_xalign(0.0);
    let note = gtk::Label::new(Some(
        "Search is sent only when you press Search. Results are provided by Open-Meteo/GeoNames.",
    ));
    note.set_wrap(true);
    note.set_xalign(0.0);
    note.add_css_class("dim-label");
    let line = gtk::Box::new(Orientation::Horizontal, 8);
    let entry = gtk::SearchEntry::new();
    entry.set_placeholder_text(Some("Chicago or 41.878, -87.630"));
    entry.set_hexpand(true);
    let go = gtk::Button::with_label("Search");
    go.add_css_class("suggested-action");
    line.append(&entry);
    line.append(&go);
    let current = gtk::Button::with_label("Use Current Location…");
    current.set_icon_name("find-location-symbolic");
    current.set_tooltip_text(Some(
        "Ask for permission to get one location fix from GeoClue",
    ));
    let results = gtk::ListBox::new();
    results.add_css_class("boxed-list");
    let scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::External)
        .vscrollbar_policy(gtk::PolicyType::External)
        .vexpand(true)
        .child(&results)
        .build();
    let actions = gtk::Box::new(Orientation::Horizontal, 8);
    actions.set_halign(Align::End);
    let cancel = gtk::Button::with_label("Cancel");
    cancel.add_css_class("dialog-cancel");
    actions.append(&cancel);
    root.append(&title);
    root.append(&note);
    root.append(&line);
    root.append(&current);
    root.append(&scroll);
    root.append(&actions);
    dialog.set_content(Some(&root));
    {
        let dialog = dialog.clone();
        cancel.connect_clicked(move |_| dialog.close());
    }
    let escape = gtk::EventControllerKey::new();
    {
        let dialog = dialog.clone();
        escape.connect_key_pressed(move |_, key, _, _| {
            if key == gdk::Key::Escape {
                dialog.close();
                glib::Propagation::Stop
            } else {
                glib::Propagation::Proceed
            }
        });
    }
    root.add_controller(escape);
    let search_action = {
        let results = results.clone();
        let entry = entry.clone();
        let s = state.clone();
        let v = views.clone();
        let st = store.clone();
        let dialog = dialog.clone();
        move || {
            let query = entry.text().to_string();
            clear_list(&results);
            let spinner = gtk::Spinner::new();
            spinner.start();
            results.append(&spinner);
            let (tx, rx) = async_channel::bounded(1);
            RUNTIME.spawn(async move {
                let result = match OpenMeteoGeocoder::new() {
                    Ok(g) => g.search(&query).await.map_err(|e| e.to_string()),
                    Err(e) => Err(e.to_string()),
                };
                let _ = tx.send(result).await;
            });
            let results = results.clone();
            let s = s.clone();
            let v = v.clone();
            let st = st.clone();
            let dialog = dialog.clone();
            glib::spawn_future_local(async move {
                clear_list(&results);
                match rx.recv().await {
                    Ok(Ok(rows)) if rows.is_empty() => {
                        let p = gtk::Label::new(Some("No matching places"));
                        p.set_margin_top(24);
                        results.append(&p)
                    }
                    Ok(Ok(rows)) => {
                        for location in rows {
                            let row = gtk::ListBoxRow::new();
                            let line = gtk::Box::new(Orientation::Horizontal, 10);
                            line.set_margin_start(12);
                            line.set_margin_end(12);
                            line.set_margin_top(10);
                            line.set_margin_bottom(10);
                            let text = gtk::Box::new(Orientation::Vertical, 2);
                            text.set_hexpand(true);
                            let name = gtk::Label::new(Some(&location.display_name));
                            name.set_xalign(0.0);
                            name.add_css_class("heading");
                            let info = gtk::Label::new(Some(&format!(
                                "{} · {:.4}, {:.4} · {}",
                                location.country_code,
                                location.latitude,
                                location.longitude,
                                location.timezone
                            )));
                            info.set_xalign(0.0);
                            info.add_css_class("dim-label");
                            text.append(&name);
                            text.append(&info);
                            let add = gtk::Button::with_label("Add");
                            add.add_css_class("suggested-action");
                            line.append(&text);
                            line.append(&add);
                            row.set_child(Some(&line));
                            {
                                let l = location.clone();
                                let s = s.clone();
                                let v = v.clone();
                                let st = st.clone();
                                let dialog = dialog.clone();
                                add.connect_clicked(move |_| {
                                    add_location(l.clone(), &s, &v, &st);
                                    dialog.close();
                                });
                            }
                            results.append(&row)
                        }
                    }
                    Ok(Err(e)) => {
                        let p = gtk::Label::new(Some(&e));
                        p.set_wrap(true);
                        p.set_margin_top(24);
                        results.append(&p)
                    }
                    Err(_) => {}
                }
            });
        }
    };
    {
        let action = search_action.clone();
        go.connect_clicked(move |_| action());
    }
    {
        entry.connect_activate(move |_| search_action());
    }
    {
        let parent = dialog.clone();
        let s = state.clone();
        let v = views.clone();
        let st = store.clone();
        current.connect_clicked(move |_|{
            let confirm=gtk::AlertDialog::builder().message("Allow Weatherglass to access your current location once?").detail("GeoClue will handle desktop permission. You can keep using every other feature if you decline.").buttons(["Cancel","Allow Once"]).cancel_button(0).default_button(1).build();
            let parent=parent.clone();let s=s.clone();let v=v.clone();let st=st.clone();
            glib::spawn_future_local(async move{if confirm.choose_future(Some(&parent)).await.ok()!=Some(1){return}let(tx,rx)=async_channel::bounded(1);RUNTIME.spawn(async move{let _=tx.send(crate::location::current_location().await.map_err(|e|e.to_string())).await;});match rx.recv().await{Ok(Ok(location))=>{add_location(location,&s,&v,&st);parent.close()},Ok(Err(e))=>toast(&v,&e),Err(_)=>{}}});
        });
    }
    dialog.present();
}

fn add_location(
    mut l: SavedLocation,
    state: &Rc<RefCell<UiState>>,
    views: &ViewRefs,
    store: &LocationStore,
) {
    let mut s = state.borrow_mut();
    if s.locations.iter().any(|x| {
        (x.latitude - l.latitude).abs() < 0.000001 && (x.longitude - l.longitude).abs() < 0.000001
    }) {
        drop(s);
        toast(views, "That location is already saved");
        return;
    }
    l.sort_order = s.locations.len() as i64;
    for x in &mut s.locations {
        x.last_selected = false
    }
    l.last_selected = true;
    s.selected = Some(l.id.clone());
    s.weather.insert(l.id.clone(), demo_weather());
    s.locations.push(l.clone());
    drop(s);
    let st = store.clone();
    RUNTIME.spawn(async move {
        let _ = st.upsert(l.clone()).await;
        let _ = st.select(l.id).await;
    });
    render_all(state, views, store);
    toast(views, "Location added");
}
fn remove_selected(state: &Rc<RefCell<UiState>>, views: &ViewRefs, store: &LocationStore) {
    let mut s = state.borrow_mut();
    let Some(id) = s.selected.clone() else { return };
    let Some(index) = s.locations.iter().position(|x| x.id == id) else {
        return;
    };
    let name = s.locations[index].display_name.clone();
    s.locations.remove(index);
    s.weather.remove(&id);
    s.selected = s
        .locations
        .get(index.min(s.locations.len().saturating_sub(1)))
        .map(|x| x.id.clone());
    let next = s.selected.clone();
    drop(s);
    let st = store.clone();
    RUNTIME.spawn(async move {
        let _ = st.delete(id).await;
        if let Some(x) = next {
            let _ = st.select(x).await;
        }
    });
    render_all(state, views, store);
    toast(views, &format!("Removed {name}"));
}

fn show_rename(state: &Rc<RefCell<UiState>>, views: &ViewRefs, store: &LocationStore) {
    let (s_id, current) = {
        let s = state.borrow();
        let Some(id) = s.selected.clone() else { return };
        let Some(l) = s.locations.iter().find(|x| x.id == id) else {
            return;
        };
        (id, l.display_name.clone())
    };
    let dialog = adw::Window::builder()
        .title("Rename Location")
        .transient_for(&views.window)
        .modal(true)
        .default_width(420)
        .default_height(150)
        .build();
    let root = gtk::Box::new(Orientation::Vertical, 12);
    root.set_margin_start(18);
    root.set_margin_end(18);
    root.set_margin_top(18);
    root.set_margin_bottom(18);
    let entry = gtk::Entry::new();
    entry.set_text(&current);
    entry.set_activates_default(true);
    let save = gtk::Button::with_label("Rename");
    save.add_css_class("suggested-action");
    save.set_halign(Align::End);
    dialog.set_default_widget(Some(&save));
    root.append(&entry);
    root.append(&save);
    dialog.set_content(Some(&root));
    let s = state.clone();
    let v = views.clone();
    let st = store.clone();
    let dialog2 = dialog.clone();
    save.connect_clicked(move |_| {
        let name = entry.text().trim().to_string();
        if name.is_empty() {
            return;
        }
        if let Some(l) = s.borrow_mut().locations.iter_mut().find(|x| x.id == s_id) {
            l.display_name = name.clone();
        }
        let store2 = st.clone();
        let id = s_id.clone();
        RUNTIME.spawn(async move {
            let _ = store2.rename(id, name).await;
        });
        render_all(&s, &v, &st);
        dialog2.close();
    });
    dialog.present();
}

fn show_settings(state: &Rc<RefCell<UiState>>, views: &ViewRefs, store: &LocationStore) {
    let dialog = adw::Window::builder()
        .title("Weatherglass Settings")
        .transient_for(&views.window)
        .modal(true)
        .default_width(620)
        .default_height(680)
        .build();
    let toolbar = adw::ToolbarView::new();
    let header = adw::HeaderBar::new();
    let header_title = gtk::Label::new(Some("Weatherglass Settings"));
    header_title.add_css_class("title");
    header.set_title_widget(Some(&header_title));
    let save = gtk::Button::with_label("Save");
    save.add_css_class("suggested-action");
    save.set_tooltip_text(Some("Save units, theme, credentials, and privacy settings"));
    header.pack_end(&save);
    toolbar.add_top_bar(&header);
    let settings_scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::External)
        .vscrollbar_policy(gtk::PolicyType::External)
        .vexpand(true)
        .build();
    toolbar.set_content(Some(&settings_scroll));
    dialog.set_content(Some(&toolbar));
    let page = adw::PreferencesPage::new();
    page.set_title("Settings");
    page.set_icon_name(Some("preferences-system-symbolic"));
    let provider_group = adw::PreferencesGroup::new();
    provider_group.set_title("Weather provider");
    provider_group.set_description(Some(
        "Open-Meteo is free and needs no account. WeatherKit remains available with your Apple credentials.",
    ));
    let provider = adw::ComboRow::new();
    provider.set_title("Forecast source");
    provider.set_model(Some(&gtk::StringList::new(&[
        "Open-Meteo (free, no key)",
        "Apple WeatherKit",
    ])));
    provider.set_selected(
        if state.borrow().settings.weather_provider == "weatherkit" {
            1
        } else {
            0
        },
    );
    provider_group.add(&provider);
    page.add(&provider_group);
    let auth = adw::PreferencesGroup::new();
    auth.set_title("WeatherKit credentials");
    auth.set_description(Some(
        "Stored locally; the private key is saved only in GNOME Keyring.",
    ));
    let team = adw::EntryRow::new();
    team.set_title("Team ID");
    let key = adw::EntryRow::new();
    key.set_title("Key ID");
    let service = adw::EntryRow::new();
    service.set_title("Service ID");
    {
        let s = state.borrow();
        team.set_text(&s.settings.credentials.team_id);
        key.set_text(&s.settings.credentials.key_id);
        service.set_text(&s.settings.credentials.service_id);
    }
    auth.add(&team);
    auth.add(&key);
    auth.add(&service);
    let import = adw::ActionRow::new();
    import.set_title("Private key (.p8)");
    import.set_subtitle("Import into GNOME Secret Service");
    let import_button = gtk::Button::with_label("Import…");
    import_button.set_valign(Align::Center);
    import.add_suffix(&import_button);
    auth.add(&import);
    let test = gtk::Button::with_label("Test Selected Provider");
    test.set_margin_top(10);
    auth.add(&test);
    page.add(&auth);
    let units_group = adw::PreferencesGroup::new();
    units_group.set_title("Units and appearance");
    let temp = adw::ComboRow::new();
    temp.set_title("Temperature");
    temp.set_model(Some(&gtk::StringList::new(&["Celsius", "Fahrenheit"])));
    temp.set_selected(
        if state.borrow().settings.temperature == TemperatureUnit::Celsius {
            0
        } else {
            1
        },
    );
    units_group.add(&temp);
    let wind = adw::ComboRow::new();
    wind.set_title("Wind speed");
    wind.set_model(Some(&gtk::StringList::new(&[
        "km/h", "mph", "m/s", "knots",
    ])));
    wind.set_selected(match state.borrow().settings.wind {
        crate::units::WindUnit::KilometresPerHour => 0,
        crate::units::WindUnit::MilesPerHour => 1,
        crate::units::WindUnit::MetresPerSecond => 2,
        crate::units::WindUnit::Knots => 3,
    });
    units_group.add(&wind);
    let precip = adw::ComboRow::new();
    precip.set_title("Precipitation");
    precip.set_model(Some(&gtk::StringList::new(&["Millimetres", "Inches"])));
    precip.set_selected(
        if state.borrow().settings.precipitation == crate::units::PrecipitationUnit::Millimetres {
            0
        } else {
            1
        },
    );
    units_group.add(&precip);
    let pressure = adw::ComboRow::new();
    pressure.set_title("Pressure");
    pressure.set_model(Some(&gtk::StringList::new(&["hPa", "inHg"])));
    pressure.set_selected(
        if state.borrow().settings.pressure == crate::units::PressureUnit::Hectopascals {
            0
        } else {
            1
        },
    );
    units_group.add(&pressure);
    let distance = adw::ComboRow::new();
    distance.set_title("Visibility");
    distance.set_model(Some(&gtk::StringList::new(&["Kilometres", "Miles"])));
    distance.set_selected(
        if state.borrow().settings.distance == crate::units::DistanceUnit::Kilometres {
            0
        } else {
            1
        },
    );
    units_group.add(&distance);
    let theme = adw::ComboRow::new();
    theme.set_title("Theme");
    theme.set_model(Some(&gtk::StringList::new(&["System", "Light", "Dark"])));
    theme.set_selected(match state.borrow().settings.theme.as_str() {
        "light" => 1,
        "dark" => 2,
        _ => 0,
    });
    {
        let window = views.window.clone();
        theme.connect_selected_notify(move |row| {
            apply_theme(
                match row.selected() {
                    1 => "light",
                    2 => "dark",
                    _ => "system",
                },
                &window,
            )
        });
    }
    units_group.add(&theme);
    let reduced = adw::SwitchRow::new();
    reduced.set_title("Reduce motion");
    reduced.set_subtitle("Disable optional interface motion");
    reduced.set_active(state.borrow().settings.reduce_motion);
    units_group.add(&reduced);
    page.add(&units_group);
    let privacy = adw::PreferencesGroup::new();
    privacy.set_title("Privacy");
    privacy.set_description(Some("Forecast requests go to the selected provider. Open-Meteo requires no key. WeatherKit credentials remain in GNOME Keyring. Place searches are cached and rate-limited."));
    page.add(&privacy);
    settings_scroll.set_child(Some(&page));
    {
        let s = state.clone();
        let window = views.window.clone();
        dialog.connect_close_request(move |_| {
            apply_theme(&s.borrow().settings.theme, &window);
            glib::Propagation::Proceed
        });
    }
    let collect_settings: Rc<dyn Fn() -> Settings> = {
        let original = state.borrow().settings.clone();
        let provider = provider.clone();
        let team = team.clone();
        let key = key.clone();
        let service = service.clone();
        let temp = temp.clone();
        let wind = wind.clone();
        let precip = precip.clone();
        let pressure = pressure.clone();
        let distance = distance.clone();
        let theme = theme.clone();
        let reduced = reduced.clone();
        Rc::new(move || {
            let mut settings = original.clone();
            settings.weather_provider = if provider.selected() == 1 {
                "weatherkit"
            } else {
                "open-meteo"
            }
            .into();
            settings.credentials.team_id = team.text().trim().to_string();
            settings.credentials.key_id = key.text().trim().to_string();
            settings.credentials.service_id = service.text().trim().to_string();
            settings.temperature = if temp.selected() == 0 {
                TemperatureUnit::Celsius
            } else {
                TemperatureUnit::Fahrenheit
            };
            settings.wind = match wind.selected() {
                1 => crate::units::WindUnit::MilesPerHour,
                2 => crate::units::WindUnit::MetresPerSecond,
                3 => crate::units::WindUnit::Knots,
                _ => crate::units::WindUnit::KilometresPerHour,
            };
            settings.precipitation = if precip.selected() == 0 {
                crate::units::PrecipitationUnit::Millimetres
            } else {
                crate::units::PrecipitationUnit::Inches
            };
            settings.pressure = if pressure.selected() == 0 {
                crate::units::PressureUnit::Hectopascals
            } else {
                crate::units::PressureUnit::InchesMercury
            };
            settings.distance = if distance.selected() == 0 {
                crate::units::DistanceUnit::Kilometres
            } else {
                crate::units::DistanceUnit::Miles
            };
            settings.theme = match theme.selected() {
                1 => "light",
                2 => "dark",
                _ => "system",
            }
            .into();
            settings.reduce_motion = reduced.is_active();
            settings.units_configured = true;
            settings
        })
    };
    {
        let window = views.window.clone();
        let v = views.clone();
        import_button.connect_clicked(move |_| {
            let chooser = gtk::FileDialog::builder()
                .title("Import WeatherKit .p8 key")
                .accept_label("Import")
                .build();
            let filter = gtk::FileFilter::new();
            filter.add_pattern("*.p8");
            filter.set_name(Some("Apple private key (*.p8)"));
            let filters = gio::ListStore::new::<gtk::FileFilter>();
            filters.append(&filter);
            chooser.set_filters(Some(&filters));
            let window = window.clone();
            let v = v.clone();
            glib::spawn_future_local(async move {
                let Ok(file) = chooser.open_future(Some(&window)).await else {
                    return;
                };
                let Some(path) = file.path() else {
                    toast(&v, "The selected key is not a local file");
                    return;
                };
                let (tx, rx) = async_channel::bounded(1);
                RUNTIME.spawn(async move {
                    let result = async {
                        let text = tokio::fs::read_to_string(path).await?;
                        GnomeSecretStore
                            .save_private_key(&secrecy::SecretString::from(text))
                            .await
                            .map_err(anyhow::Error::from)?;
                        Ok::<_, anyhow::Error>(())
                    }
                    .await;
                    let _ = tx.send(result.map_err(|e| e.to_string())).await;
                });
                match rx.recv().await {
                    Ok(Ok(())) => toast(&v, "Private key stored securely in GNOME Keyring"),
                    Ok(Err(e)) => toast(&v, &e),
                    Err(_) => {}
                }
            });
        });
    }
    {
        let s = state.clone();
        let v = views.clone();
        let st = store.clone();
        let dialog = dialog.clone();
        let collect_settings = collect_settings.clone();
        save.connect_clicked(move |_| {
            let settings = collect_settings();
            apply_theme(&settings.theme, &v.window);
            s.borrow_mut().settings = settings.clone();
            render_all(&s, &v, &st);
            let (tx, rx) = async_channel::bounded(1);
            RUNTIME.spawn(async move {
                let result = async { settings.save(Settings::xdg_path()?).await }.await;
                let _ = tx.send(result.map_err(|e| e.to_string())).await;
            });
            let v = v.clone();
            glib::spawn_future_local(async move {
                match rx.recv().await {
                    Ok(Ok(())) => toast(&v, "Settings saved"),
                    Ok(Err(e)) => toast(&v, &format!("Could not save settings: {e}")),
                    Err(_) => {}
                }
            });
            dialog.close();
        });
    }
    {
        let s = state.clone();
        let v = views.clone();
        let st = store.clone();
        let dialog = dialog.clone();
        let collect_settings = collect_settings.clone();
        test.connect_clicked(move |_| {
            let settings = collect_settings();
            apply_theme(&settings.theme, &v.window);
            s.borrow_mut().settings = settings.clone();
            RUNTIME.spawn(async move {
                if let Ok(path) = Settings::xdg_path() {
                    let _ = settings.save(path).await;
                }
            });
            render_all(&s, &v, &st);
            refresh_selected(&s, &v, &st, true);
            dialog.close();
        });
    }
    dialog.present();
}
fn apply_theme(theme: &str, window: &adw::ApplicationWindow) {
    let manager = match gdk::Display::default() {
        Some(display) => adw::StyleManager::for_display(&display),
        None => adw::StyleManager::default(),
    };
    manager.set_color_scheme(match theme {
        "light" => adw::ColorScheme::ForceLight,
        "dark" => adw::ColorScheme::ForceDark,
        _ => adw::ColorScheme::Default,
    });
    window.remove_css_class("light");
    window.remove_css_class("dark");
    let effective = match theme {
        "light" => "light",
        "dark" => "dark",
        _ if manager.is_dark() => "dark",
        _ => "light",
    };
    window.add_css_class(effective);
}
fn toast(views: &ViewRefs, message: &str) {
    let t = adw::Toast::new(message);
    t.set_timeout(5);
    views.toast.add_toast(t);
}

const CSS: &str = r#"
* { -gtk-icon-style: symbolic; }
window { font-family:"Inter","Ubuntu Sans",sans-serif; font-size:15px; background:#071120; color:#f5f7fc; }
headerbar { min-height:58px; padding:0 16px; background:alpha(#081321,.90); border:0; box-shadow:none; }
.main-header { border-bottom:1px solid alpha(white,.07); }
.sidebar-header { min-height:62px; background:transparent; }
button { color:inherit; background:transparent; background-image:none; border:0; box-shadow:none; text-shadow:none; }
button:hover { background:alpha(white,.08); }
button:active { background:alpha(white,.14); }
button:focus-visible { outline:2px solid #53adff; outline-offset:2px; }
button.flat { padding:7px; border-radius:10px; }
.sidebar { background:linear-gradient(165deg,#091422 0%,#07101e 52%,#0a1728 100%); border-right:1px solid alpha(white,.12); }
.forecast-page { background:linear-gradient(145deg,#0d1d33 0%,#102946 45%,#132a45 100%); color:#f5f7fc; border-radius:20px 20px 0 0; }
.forecast-card,.metric-card,.alert-card,.radar-card { background:alpha(#182b43,.84); border:1px solid alpha(white,.09); box-shadow:0 8px 24px alpha(black,.18); border-radius:14px; padding:12px; }
.light window { color:#14213a; background:#e4edf8; }
.light .sidebar { background:linear-gradient(165deg,#eef5fc,#dbe8f7); color:#14213a; border-color:alpha(#14213a,.14); }
.light .main-header { background:alpha(#eaf2fd,.88); border-color:alpha(#14213a,.08); }
.light .forecast-page { background:linear-gradient(145deg,#eff6ff 0%,#dceafb 46%,#c6daf1 100%); color:#14213a; }
.light .forecast-card,.light .metric-card,.light .alert-card,.light .radar-card { background:alpha(white,.72); color:#14213a; border-color:alpha(#24436a,.14); box-shadow:0 8px 22px alpha(#24436a,.10); }
.light .forecast-page .dim-label,.light .sidebar .dim-label { color:alpha(#14213a,.68); }
.light .hourly-panel,.light .metric-tabs { background:alpha(#50709c,.10); border-color:alpha(#14213a,.12); }
.light .day-row { border-color:alpha(#14213a,.14); }
.light .sidebar-range,.light .location-time { color:alpha(#14213a,.68); }
.light .location-list row { background:alpha(white,.35); border-color:alpha(#14213a,.08); }
.light .selected-location { background:alpha(#6ca7e8,.28); border-color:alpha(#315f95,.50); }
.hero { min-height:178px; padding:22px 28px 18px; border-radius:18px; background:linear-gradient(135deg,alpha(#1a4675,.96),alpha(#143457,.85) 55%,alpha(#7b8d9d,.58)); border:1px solid alpha(white,.10); box-shadow:inset 0 -50px 80px alpha(#071120,.24); }
.hero-top { min-height:130px; }
.hero-location { font-size:31px; font-weight:750; letter-spacing:-.6px; }
.hero-date { font-size:16px; color:alpha(white,.80); }
.hero-updated { font-size:14px; color:alpha(white,.62); }
.hero-temp { font-size:62px; font-weight:300; letter-spacing:-3px; line-height:1; }
.hero-icon { font-size:49px; color:#ffd43b; line-height:1; }
.hero-condition { font-size:17px; font-weight:600; }
.hero-range { font-size:16px; color:alpha(white,.86); }
.section-title,.metric-title { font-size:13px; font-weight:800; letter-spacing:1.5px; opacity:.86; }
.metric-title { letter-spacing:.8px; }
.metric-value { font-size:21px; font-weight:550; }
.metric-icon { font-size:19px; color:#9edcff; }
.forecast-page .dim-label { color:alpha(white,.70); }
.sidebar-search { margin:0 19px 12px; min-height:40px; padding:0 13px; border-radius:12px; border:1px solid alpha(white,.20); background:alpha(#020811,.32); color:alpha(white,.70); }
.sidebar-search:hover { border-color:alpha(#7ccaff,.75); background:alpha(#142a43,.55); }
.location-list { padding:0 7px; }
.location-list row { border-radius:14px; margin:5px 7px; background:alpha(white,.045); border:1px solid alpha(white,.055); }
.location-list row:hover { background:alpha(white,.10); }
.selected-location { background:alpha(#1c5d9f,.38); border-color:#3199eb; }
.location-name { font-size:16px; font-weight:700; }
.location-time { font-size:13px; color:alpha(white,.64); }
.location-condition { font-size:14px; color:alpha(white,.68); }
.sidebar-range { font-size:13px; color:alpha(white,.68); }
.sidebar-temp { font-size:31px; font-weight:350; letter-spacing:-1px; }
.sidebar-condition-icon { font-size:24px; color:#ffca2b; }
.add-location { min-width:40px; min-height:32px; margin:6px 8px 7px; padding:0; border-radius:9px; border:1px solid alpha(white,.16); background:alpha(#21344c,.72); }
.add-location:hover { background:alpha(#2d5275,.85); border-color:alpha(#71bcff,.70); }
.dialog-cancel { min-height:36px; padding:0 18px; border-radius:10px; border:1px solid alpha(white,.16); background:alpha(#21344c,.72); }
.attribution-small { font-size:12px; margin:10px 16px 15px; color:#52b1ff; }
.hourly-panel { margin-top:8px; padding:12px 12px 10px; border:1px solid alpha(white,.10); border-radius:15px; background:alpha(#1a304b,.78); }
.hour-tile { min-width:56px; min-height:92px; padding:5px 4px; border-radius:0; border-right:1px solid alpha(white,.16); background:transparent; font-size:14px; }
.hour-tile:last-child { border-right:0; }
.hour-tile label { color:alpha(white,.84); }
.hour-icon,.day-icon { font-size:23px; }
.hour-temp { font-size:18px; font-weight:700; }
.precip { color:#8ed8ff; font-size:13px; font-weight:700; }
.metric-tabs { margin-top:2px; padding:2px; min-height:38px; border-radius:13px; background:alpha(#1a304b,.86); border:1px solid alpha(white,.08); }
.metric-tab { min-height:34px; padding:4px 20px; border-radius:11px; color:alpha(white,.90); font-size:14px; }
.metric-tab:hover { background:alpha(white,.08); }
.metric-tab:checked { background:linear-gradient(180deg,#2585d8,#1764ad); color:white; font-weight:700; box-shadow:0 2px 8px alpha(#0074d9,.38); }
.metric-tab:disabled { opacity:.45; }
.chart { min-height:82px; padding:8px; }
.chart progressbar trough,.forecast-card progressbar trough { background:alpha(white,.12); }
.chart progressbar progress,.forecast-card progressbar progress { background:#76d2ff; }
.pill { min-height:32px; padding:4px 11px; border-radius:10px; border:1px solid alpha(white,.12); background:alpha(#1b314b,.85); font-size:13px; }
.pill:hover { border-color:alpha(#83caff,.7); background:alpha(#2a4866,.92); }
.pill:checked { background:#1477ca; color:white; font-weight:700; }
.layer-button { min-width:40px; min-height:38px; padding:0; border-radius:10px; border:1px solid alpha(white,.14); background:alpha(#1b314b,.92); }
.layer-button:hover { background:alpha(#2d5275,.95); border-color:alpha(#83caff,.72); }
.layer-menu { min-width:205px; }
.layer-menu > box { min-height:30px; }
.layer-menu checkbutton { min-height:30px; padding:4px 7px; border-radius:8px; }
.layer-menu checkbutton:hover { background:alpha(white,.08); }
.layer-menu switch { min-width:38px; }
.weather-dashboard { margin-top:2px; }
.radar-column { min-width:0; }
.weather-dashboard .metric-card { min-height:74px; }
.radar-card { padding:12px; }
.radar-preview,.radar-full { background:#101d2d; border-radius:11px; }
.radar-time { font-size:13px; font-weight:600; color:white; }
.radar-legend { font-size:11px; font-weight:700; color:white; border-radius:4px; padding:3px 6px; background:linear-gradient(90deg,#3c87ff,#35d9ce,#ffe550,#ff8a2b,#d832d8); text-shadow:0 1px 2px black; }
.radar-marker { color:white; background:#172343; border:2px solid white; border-radius:999px; padding:4px; font-size:12px; }
.day-row { background:transparent; border-bottom:1px solid alpha(white,.13); border-radius:0; padding:5px 4px; min-height:32px; font-size:15px; }
.day-row:hover { background:alpha(white,.08); }
.day-row levelbar { min-height:15px; }
.day-row levelbar trough { background:alpha(white,.14); border-radius:9px; }
.day-row levelbar block.filled { background:linear-gradient(90deg,#6cb5d0,#a4bd61 55%,#ffbc27); border-radius:9px; }
.alert-card { background:alpha(#ffb447,.20); border-color:alpha(#ffd38b,.45); }
.alert-severity { font-size:12px; font-weight:900; letter-spacing:1px; color:#ffe0a6; }
.alert-title { font-size:19px; font-weight:700; }
.conditions-grid { margin-top:0; }
.conditions-grid > child { min-width:145px; }
.weather-attribution { font-size:12px; color:alpha(white,.76); }
.title-2 { font-size:24px; font-weight:700; }
scrolledwindow scrollbar { opacity:0; min-width:0; min-height:0; }
"#;

#[cfg(test)]
mod hourly_chart_tests {
    use super::*;

    fn fixture() -> crate::models::HourlyForecast {
        let weather: WeatherResponse =
            serde_json::from_str(include_str!("../tests/fixtures/demo_weather.json")).unwrap();
        weather.forecast_hourly.unwrap()
    }

    #[test]
    fn every_hourly_button_has_a_fixture_series() {
        let hourly = fixture();
        let settings = Settings::for_locale("en_US");
        for metric in [
            HourlyMetric::Temperature,
            HourlyMetric::Precipitation,
            HourlyMetric::Wind,
            HourlyMetric::Humidity,
            HourlyMetric::Pressure,
            HourlyMetric::FeelsLike,
        ] {
            assert!(metric_series(&hourly, metric, &settings).is_some());
        }
    }

    #[test]
    fn hourly_series_uses_selected_units_and_missing_metrics_are_disabled() {
        let mut hourly = fixture();
        let imperial = Settings::for_locale("en_US");
        let temperature = metric_series(&hourly, HourlyMetric::Temperature, &imperial).unwrap();
        assert_eq!(temperature.unit, "°F");
        assert!((temperature.values[0].unwrap() - 75.56).abs() < 0.01);

        for hour in &mut hourly.hours {
            hour.temperature_apparent = None;
        }
        assert!(metric_series(&hourly, HourlyMetric::FeelsLike, &imperial).is_none());
    }

    #[test]
    fn moon_phase_uses_clear_fractional_terms() {
        assert_eq!(format_moon_phase("waningGibbous"), "3/4 full");
        assert_eq!(format_moon_phase("firstQuarter"), "1/2 full");
        assert_eq!(format_moon_illumination(0.5), "Full moon");
        assert_eq!(format_moon_illumination(0.0), "New moon");
    }

    #[test]
    fn hourly_forecast_begins_with_the_current_hour() {
        let mut hourly = fixture();
        let now = Utc::now();
        hourly.hours[0].forecast_start = now - chrono::Duration::hours(2);
        hourly.hours[1].forecast_start = now - chrono::Duration::minutes(30);
        assert_eq!(current_hour_index(&hourly.hours, now), 1);
    }

    #[test]
    fn radar_zoom_changes_and_stays_in_supported_range() {
        assert_eq!(radar_zoom_in(6), 7);
        assert_eq!(radar_zoom_in(7), 7);
        assert_eq!(radar_zoom_out(7), 6);
        assert_eq!(radar_zoom_out(2), 2);
    }
}
