use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum TemperatureUnit {
    Celsius,
    Fahrenheit,
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum WindUnit {
    KilometresPerHour,
    MilesPerHour,
    MetresPerSecond,
    Knots,
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum PrecipitationUnit {
    Millimetres,
    Inches,
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum PressureUnit {
    Hectopascals,
    InchesMercury,
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum DistanceUnit {
    Kilometres,
    Miles,
}

pub fn temperature(c: f64, unit: TemperatureUnit) -> f64 {
    match unit {
        TemperatureUnit::Celsius => c,
        TemperatureUnit::Fahrenheit => c * 9.0 / 5.0 + 32.0,
    }
}
pub fn wind(kmh: f64, unit: WindUnit) -> f64 {
    match unit {
        WindUnit::KilometresPerHour => kmh,
        WindUnit::MilesPerHour => kmh * 0.621_371,
        WindUnit::MetresPerSecond => kmh / 3.6,
        WindUnit::Knots => kmh * 0.539_957,
    }
}
pub fn precipitation(mm: f64, unit: PrecipitationUnit) -> f64 {
    match unit {
        PrecipitationUnit::Millimetres => mm,
        PrecipitationUnit::Inches => mm / 25.4,
    }
}
pub fn pressure(hpa: f64, unit: PressureUnit) -> f64 {
    match unit {
        PressureUnit::Hectopascals => hpa,
        PressureUnit::InchesMercury => hpa * 0.029_529_983_071_4,
    }
}
pub fn distance(km: f64, unit: DistanceUnit) -> f64 {
    match unit {
        DistanceUnit::Kilometres => km,
        DistanceUnit::Miles => km * 0.621_371,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn conversions() {
        assert!((temperature(0.0, TemperatureUnit::Fahrenheit) - 32.0).abs() < 1e-9);
        assert!((precipitation(25.4, PrecipitationUnit::Inches) - 1.0).abs() < 1e-9);
        assert!((wind(3.6, WindUnit::MetresPerSecond) - 1.0).abs() < 1e-9);
        assert!((pressure(1013.25, PressureUnit::InchesMercury) - 29.921).abs() < 0.01);
    }
    #[test]
    fn saved_timezone_handles_dst() {
        use chrono::TimeZone;
        let tz: chrono_tz::Tz = "America/Chicago".parse().unwrap();
        let winter = tz.with_ymd_and_hms(2026, 1, 15, 12, 0, 0).unwrap();
        let summer = tz.with_ymd_and_hms(2026, 7, 15, 12, 0, 0).unwrap();
        assert_ne!(winter.offset().to_string(), summer.offset().to_string());
    }
}
