//! Display units. Geometry always remains in metres and angles in degrees.
use serde::{Deserialize, Serialize};

pub const MAX_DECIMAL_PLACES: u8 = 6;

macro_rules! units {
    ($name:ident, $default:ident, $digits:literal, $( $variant:ident => ($label:literal, $symbol:literal, $factor:expr) ),+ $(,)?) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
        #[serde(rename_all = "snake_case")]
        pub enum $name { $( $variant ),+ }

        impl Default for $name {
            fn default() -> Self { Self::$default }
        }

        impl $name {
            pub const ALL: &[Self] = &[$( Self::$variant ),+];

            pub fn label(self) -> &'static str {
                match self { $( Self::$variant => $label ),+ }
            }

            pub fn symbol(self) -> &'static str {
                match self { $( Self::$variant => $symbol ),+ }
            }

            /// Base units per selected unit (metres, square/cubic metres, or degrees).
            pub fn factor(self) -> f64 {
                match self { $( Self::$variant => $factor ),+ }
            }

            pub fn format(self, value: f64) -> String {
                self.format_with_precision(value, $digits)
            }

            pub fn format_with_precision(self, value: f64, decimal_places: u8) -> String {
                format!("{:.*} {}", decimal_places.min(MAX_DECIMAL_PLACES) as usize, value / self.factor(), self.symbol())
            }
        }
    };
}

units!(LengthUnit, Meters, 3,
    Millimeters => ("Millimeters (mm)", "mm", 0.001),
    Centimeters => ("Centimeters (cm)", "cm", 0.01),
    Meters => ("Meters (m)", "m", 1.),
    Kilometers => ("Kilometers (km)", "km", 1000.),
    Inches => ("Inches (in)", "in", 0.0254),
    Feet => ("Feet (ft)", "ft", 0.3048),
    Yards => ("Yards (yd)", "yd", 0.9144),
);

units!(AreaUnit, SquareMeters, 3,
    SquareMillimeters => ("Square millimeters (mm²)", "mm²", 1e-6),
    SquareCentimeters => ("Square centimeters (cm²)", "cm²", 1e-4),
    SquareMeters => ("Square meters (m²)", "m²", 1.),
    SquareKilometers => ("Square kilometers (km²)", "km²", 1e6),
    SquareInches => ("Square inches (in²)", "in²", 0.0254_f64.powi(2)),
    SquareFeet => ("Square feet (ft²)", "ft²", 0.3048_f64.powi(2)),
    SquareYards => ("Square yards (yd²)", "yd²", 0.9144_f64.powi(2)),
    Hectares => ("Hectares (ha)", "ha", 10000.),
    Acres => ("Acres (ac)", "ac", 4046.8564224),
);

units!(VolumeUnit, CubicMeters, 3,
    CubicMillimeters => ("Cubic millimeters (mm³)", "mm³", 1e-9),
    CubicCentimeters => ("Cubic centimeters (cm³)", "cm³", 1e-6),
    CubicMeters => ("Cubic meters (m³)", "m³", 1.),
    CubicInches => ("Cubic inches (in³)", "in³", 0.0254_f64.powi(3)),
    CubicFeet => ("Cubic feet (ft³)", "ft³", 0.3048_f64.powi(3)),
    CubicYards => ("Cubic yards (yd³)", "yd³", 0.9144_f64.powi(3)),
    Liters => ("Liters (L)", "L", 0.001),
    Milliliters => ("Milliliters (mL)", "mL", 1e-6),
);

units!(AngleUnit, Degrees, 2,
    Degrees => ("Degrees (°)", "°", 1.),
    Radians => ("Radians (rad)", "rad", 180. / std::f64::consts::PI),
    Gradians => ("Gradians (gon)", "gon", 0.9),
);

impl AngleUnit {
    pub fn default_precision(self) -> u8 {
        if self == Self::Radians { 4 } else { 2 }
    }

    pub fn format_angle(self, degrees: f64) -> String {
        self.format_angle_with_precision(degrees, self.default_precision())
    }

    pub fn format_angle_with_precision(self, degrees: f64, decimal_places: u8) -> String {
        if self == Self::Degrees {
            format!(
                "{:.*}°",
                decimal_places.min(MAX_DECIMAL_PLACES) as usize,
                degrees
            )
        } else {
            self.format_with_precision(degrees, decimal_places)
        }
    }
}

/// None preserves the existing default for the selected unit, including radians.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct UnitPrecision {
    pub length: Option<u8>,
    pub area: Option<u8>,
    pub volume: Option<u8>,
    pub angle: Option<u8>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct UnitSettings {
    pub length: LengthUnit,
    pub area: AreaUnit,
    pub volume: VolumeUnit,
    pub angle: AngleUnit,
    pub precision: UnitPrecision,
}

impl UnitSettings {
    pub fn format_length(self, meters: f64) -> String {
        self.length
            .format_with_precision(meters, self.precision.length.unwrap_or(3))
    }

    pub fn format_area(self, square_meters: f64) -> String {
        self.area
            .format_with_precision(square_meters, self.precision.area.unwrap_or(3))
    }

    pub fn format_volume(self, cubic_meters: f64) -> String {
        self.volume
            .format_with_precision(cubic_meters, self.precision.volume.unwrap_or(3))
    }

    pub fn format_angle(self, degrees: f64) -> String {
        self.angle.format_angle_with_precision(
            degrees,
            self.precision
                .angle
                .unwrap_or_else(|| self.angle.default_precision()),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DisplaySettings, Document};

    #[test]
    fn precision_rounds_each_converted_measurement_independently() {
        let mut units = UnitSettings {
            length: LengthUnit::Feet,
            area: AreaUnit::SquareFeet,
            volume: VolumeUnit::Liters,
            angle: AngleUnit::Radians,
            precision: UnitPrecision {
                length: Some(2),
                area: Some(1),
                volume: Some(0),
                angle: Some(5),
            },
        };
        assert_eq!(units.format_length(0.3048 * 1.23456), "1.23 ft");
        assert_eq!(units.format_area(0.3048_f64.powi(2) * 12.3456), "12.3 ft²");
        assert_eq!(units.format_volume(0.00123456), "1 L");
        assert_eq!(units.format_angle(90.), "1.57080 rad");
        units.angle = AngleUnit::Degrees;
        assert_eq!(units.format_angle(90.), "90.00000°");
        units.angle = AngleUnit::Gradians;
        assert_eq!(units.format_angle(90.), "100.00000 gon");
        units.precision.length = Some(0);
        assert_eq!(units.format_length(0.3048 * 1.23456), "1 ft");
    }

    #[test]
    fn older_precision_defaults_are_preserved_and_saved_values_are_bounded() {
        let mut units: UnitSettings = serde_json::from_str(r#"{"angle":"radians"}"#).unwrap();
        assert_eq!(units.precision, UnitPrecision::default());
        assert_eq!(units.format_length(1.), "1.000 m");
        assert_eq!(units.format_area(1.), "1.000 m²");
        assert_eq!(units.format_volume(1.), "1.000 m³");
        assert_eq!(units.format_angle(90.), "1.5708 rad");
        units.angle = AngleUnit::Degrees;
        assert_eq!(units.format_angle(90.), "90.00°");

        let partial: UnitSettings = serde_json::from_str(r#"{"precision":{"area":1}}"#).unwrap();
        assert_eq!(partial.format_area(1.), "1.0 m²");
        assert_eq!(partial.format_length(1.), "1.000 m");
        units.precision = UnitPrecision {
            length: Some(u8::MAX),
            area: Some(u8::MAX),
            volume: Some(u8::MAX),
            angle: Some(u8::MAX),
        };
        assert_eq!(units.format_length(1. / 3.), "0.333333 m");
        assert_eq!(units.format_area(1. / 3.), "0.333333 m²");
        assert_eq!(units.format_volume(1. / 3.), "0.333333 m³");
        assert_eq!(units.format_angle(90.123456789), "90.123457°");
    }

    #[test]
    fn units_convert_lengths_areas_volumes_and_angles_independently() {
        assert_eq!(LengthUnit::Millimeters.format(1.25), "1250.000 mm");
        assert_eq!(LengthUnit::Feet.format(0.3048), "1.000 ft");
        assert_eq!(LengthUnit::Inches.format(-0.0508), "-2.000 in");
        assert_eq!(AreaUnit::SquareFeet.format(0.3048_f64.powi(2)), "1.000 ft²");
        assert_eq!(AreaUnit::Acres.format(4046.8564224), "1.000 ac");
        assert_eq!(
            VolumeUnit::CubicFeet.format(0.3048_f64.powi(3)),
            "1.000 ft³"
        );
        assert_eq!(VolumeUnit::Liters.format(1.), "1000.000 L");
        assert_eq!(AngleUnit::Degrees.format_angle(90.), "90.00°");
        assert_eq!(AngleUnit::Radians.format_angle(90.), "1.5708 rad");
        assert_eq!(AngleUnit::Gradians.format_angle(90.), "100.00 gon");
        for factor in LengthUnit::ALL
            .iter()
            .map(|u| u.factor())
            .chain(AreaUnit::ALL.iter().map(|u| u.factor()))
            .chain(VolumeUnit::ALL.iter().map(|u| u.factor()))
            .chain(AngleUnit::ALL.iter().map(|u| u.factor()))
        {
            assert!(factor.is_finite() && factor > 0.);
        }
    }

    #[test]
    fn units_round_trip_in_documents_and_default_in_older_settings() {
        let defaults = UnitSettings::default();
        assert_eq!(defaults.length, LengthUnit::Meters);
        assert_eq!(defaults.area, AreaUnit::SquareMeters);
        assert_eq!(defaults.volume, VolumeUnit::CubicMeters);
        assert_eq!(defaults.angle, AngleUnit::Degrees);
        assert_eq!(
            serde_json::from_str::<DisplaySettings>("{}").unwrap().units,
            defaults
        );
        let partial: UnitSettings = serde_json::from_str(r#"{"length":"inches"}"#).unwrap();
        assert_eq!(partial.length, LengthUnit::Inches);
        assert_eq!(partial.area, defaults.area);
        assert_eq!(partial.volume, defaults.volume);
        assert_eq!(partial.angle, defaults.angle);

        let mut doc = Document::default();
        doc.display.units = UnitSettings {
            length: LengthUnit::Feet,
            area: AreaUnit::SquareInches,
            volume: VolumeUnit::Liters,
            angle: AngleUnit::Radians,
            precision: UnitPrecision {
                length: Some(0),
                area: Some(1),
                volume: Some(4),
                angle: Some(6),
            },
        };
        let restored = Document::from_json(&serde_json::to_string(&doc).unwrap()).unwrap();
        assert_eq!(restored.display.units, doc.display.units);
        assert_eq!(restored.model, doc.model);
    }
}
