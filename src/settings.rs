use crate::auth::CredentialMetadata;
use crate::units::*;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Settings {
    #[serde(default = "default_provider")]
    pub weather_provider: String,
    pub temperature: TemperatureUnit,
    pub wind: WindUnit,
    pub precipitation: PrecipitationUnit,
    pub pressure: PressureUnit,
    pub distance: DistanceUnit,
    pub theme: String,
    pub reduce_motion: bool,
    pub credentials: CredentialMetadata,
    pub geocoder_endpoint: String,
    #[serde(default)]
    pub units_configured: bool,
}
impl Default for Settings {
    fn default() -> Self {
        Self::for_locale(&preferred_measurement_locale())
    }
}
impl Settings {
    pub fn for_locale(locale: &str) -> Self {
        let imperial = locale_country(locale)
            .is_some_and(|country| matches!(country.as_str(), "US" | "LR" | "MM"));
        Self {
            weather_provider: default_provider(),
            temperature: if imperial {
                TemperatureUnit::Fahrenheit
            } else {
                TemperatureUnit::Celsius
            },
            wind: if imperial {
                WindUnit::MilesPerHour
            } else {
                WindUnit::KilometresPerHour
            },
            precipitation: if imperial {
                PrecipitationUnit::Inches
            } else {
                PrecipitationUnit::Millimetres
            },
            pressure: if imperial {
                PressureUnit::InchesMercury
            } else {
                PressureUnit::Hectopascals
            },
            distance: if imperial {
                DistanceUnit::Miles
            } else {
                DistanceUnit::Kilometres
            },
            theme: "system".into(),
            reduce_motion: false,
            credentials: CredentialMetadata::default(),
            geocoder_endpoint: "https://nominatim.openstreetmap.org".into(),
            units_configured: false,
        }
    }

    pub fn apply_country_defaults(&mut self, country_code: &str) {
        if self.units_configured {
            return;
        }
        let imperial = matches!(country_code, "US" | "LR" | "MM");
        self.temperature = if imperial {
            TemperatureUnit::Fahrenheit
        } else {
            TemperatureUnit::Celsius
        };
        self.wind = if imperial {
            WindUnit::MilesPerHour
        } else {
            WindUnit::KilometresPerHour
        };
        self.precipitation = if imperial {
            PrecipitationUnit::Inches
        } else {
            PrecipitationUnit::Millimetres
        };
        self.pressure = if imperial {
            PressureUnit::InchesMercury
        } else {
            PressureUnit::Hectopascals
        };
        self.distance = if imperial {
            DistanceUnit::Miles
        } else {
            DistanceUnit::Kilometres
        };
    }
    pub fn xdg_path() -> Result<PathBuf> {
        Ok(
            directories::ProjectDirs::from("io", "Weatherglass", "Weatherglass")
                .context("XDG config directory unavailable")?
                .config_dir()
                .join("settings.json"),
        )
    }
    pub async fn load(path: PathBuf) -> Result<Self> {
        match tokio::fs::read(&path).await {
            Ok(b) => {
                let had_units_flag = serde_json::from_slice::<serde_json::Value>(&b)?
                    .get("units_configured")
                    .is_some();
                let mut settings: Self = serde_json::from_slice(&b)?;
                // Settings written by releases before this flag already represented an explicit save.
                if !had_units_flag {
                    settings.units_configured = true;
                }
                Ok(settings)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e.into()),
        }
    }
    pub async fn save(&self, path: PathBuf) -> Result<()> {
        if let Some(p) = path.parent() {
            tokio::fs::create_dir_all(p).await?;
        }
        let temp = path.with_extension("tmp");
        tokio::fs::write(&temp, serde_json::to_vec_pretty(self)?).await?;
        tokio::fs::rename(temp, path).await?;
        Ok(())
    }
}

fn default_provider() -> String {
    "open-meteo".into()
}

fn preferred_measurement_locale() -> String {
    ["LC_MEASUREMENT", "LC_ALL", "LANG"]
        .into_iter()
        .filter_map(|key| std::env::var(key).ok())
        .find(|locale| locale_country(locale).is_some())
        .unwrap_or_else(|| "en_GB".into())
}
fn locale_country(locale: &str) -> Option<String> {
    let normalized = locale.split(['.', '@']).next()?.replace('-', "_");
    normalized
        .split('_')
        .nth(1)
        .filter(|value| value.len() == 2)
        .map(str::to_uppercase)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn locale_defaults_follow_measurement_region() {
        let us = Settings::for_locale("en_US.UTF-8");
        assert_eq!(us.temperature, TemperatureUnit::Fahrenheit);
        assert_eq!(us.distance, DistanceUnit::Miles);
        let gb = Settings::for_locale("en_GB.UTF-8");
        assert_eq!(gb.temperature, TemperatureUnit::Celsius);
        assert_eq!(gb.distance, DistanceUnit::Kilometres);
        assert!(!us.units_configured && !gb.units_configured);
    }
    #[test]
    fn location_country_controls_unsaved_defaults_only() {
        let mut settings = Settings::for_locale("en_US");
        settings.apply_country_defaults("GB");
        assert_eq!(settings.temperature, TemperatureUnit::Celsius);
        settings.units_configured = true;
        settings.apply_country_defaults("US");
        assert_eq!(settings.temperature, TemperatureUnit::Celsius);
    }
    #[tokio::test]
    async fn older_saved_settings_remain_user_configured() {
        let t = tempfile::tempdir().unwrap();
        let path = t.path().join("settings.json");
        let mut value = serde_json::to_value(Settings::for_locale("en_GB")).unwrap();
        value.as_object_mut().unwrap().remove("units_configured");
        tokio::fs::write(&path, serde_json::to_vec(&value).unwrap())
            .await
            .unwrap();
        assert!(Settings::load(path).await.unwrap().units_configured);
    }
    #[tokio::test]
    async fn explicit_units_and_theme_persist() {
        let t = tempfile::tempdir().unwrap();
        let path = t.path().join("settings.json");
        let mut settings = Settings::for_locale("en_US");
        settings.temperature = TemperatureUnit::Celsius;
        settings.distance = DistanceUnit::Kilometres;
        settings.theme = "dark".into();
        settings.units_configured = true;
        settings.save(path.clone()).await.unwrap();
        assert_eq!(Settings::load(path).await.unwrap(), settings);
    }
}
