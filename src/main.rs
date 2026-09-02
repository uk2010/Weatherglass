use adw::prelude::*;
use weatherglass::{APP_ID, ui};

fn main() -> gtk::glib::ExitCode {
    let app = adw::Application::builder().application_id(APP_ID).build();
    app.set_resource_base_path(Some("/io/github/weatherglass/Weatherglass"));
    app.connect_startup(|_| ui::install_css());
    app.connect_activate(ui::build_window);
    app.set_accels_for_action("win.refresh", &["<Ctrl>R"]);
    app.set_accels_for_action("win.search", &["<Ctrl>L"]);
    app.set_accels_for_action("win.settings", &["<Ctrl>comma"]);
    app.set_accels_for_action("win.remove", &["Delete"]);
    app.run()
}
