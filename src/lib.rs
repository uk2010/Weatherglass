pub mod auth;
pub mod cache;
pub mod conditions;
pub mod geocode;
pub mod location;
pub mod map_layers;
pub mod models;
pub mod openmeteo;
pub mod radar;
pub mod settings;
pub mod state;
pub mod storage;
pub mod ui;
pub mod units;
pub mod weatherkit;
pub mod widgets;

use once_cell::sync::Lazy;

pub const APP_ID: &str = "io.github.weatherglass.Weatherglass";
pub const APP_NAME: &str = "Weatherglass";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub static RUNTIME: Lazy<tokio::runtime::Runtime> = Lazy::new(|| {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(4)
        .thread_name("weatherglass-worker")
        .enable_all()
        .build()
        .expect("create asynchronous runtime")
});
