#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConditionPresentation {
    pub symbol: &'static str,
    pub description: &'static str,
    pub gradient: &'static str,
}

pub fn present(code: &str, daylight: bool) -> ConditionPresentation {
    let clear = if daylight {
        ("☀", "Clear", "clear")
    } else {
        ("☾", "Clear", "night")
    };
    let (symbol, description, gradient) = match code {
        "Clear" | "MostlyClear" => clear,
        "PartlyCloudy" | "MostlyCloudy" => (
            if daylight { "🌤" } else { "☁" },
            "Partly cloudy",
            if daylight { "cloudy" } else { "night" },
        ),
        "Cloudy" => ("☁", "Cloudy", "cloudy"),
        "Drizzle" | "Rain" | "HeavyRain" | "SunShowers" => ("●", "Rain", "rain"),
        "ScatteredThunderstorms" | "StrongStorms" | "Thunderstorms" => {
            ("ϟ", "Thunderstorms", "storm")
        }
        "Flurries" | "Snow" | "HeavySnow" | "SunFlurries" | "WintryMix" => ("✻", "Snow", "snow"),
        "Foggy" | "Haze" | "Smoky" => ("≋", "Low visibility", "fog"),
        "Breezy" | "Windy" => ("≋", "Windy", "cloudy"),
        "Hail" | "Sleet" | "FreezingDrizzle" | "FreezingRain" => {
            ("◆", "Wintry precipitation", "snow")
        }
        "Blizzard" | "BlowingSnow" => ("✣", "Blizzard", "snow"),
        "Frigid" => ("❄", "Frigid", "snow"),
        "Hot" => ("☀", "Hot", "clear"),
        "Hurricane" | "TropicalStorm" => ("◉", "Tropical storm", "storm"),
        _ => ("◌", "Conditions unavailable", "cloudy"),
    };
    ConditionPresentation {
        symbol,
        description,
        gradient,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn known_and_unknown_codes() {
        assert_eq!(present("Rain", true).description, "Rain");
        assert_eq!(present("FutureAppleCode", true).symbol, "◌");
    }
    #[test]
    fn clear_respects_daylight() {
        assert_ne!(
            present("Clear", true).symbol,
            present("Clear", false).symbol
        );
    }
}
