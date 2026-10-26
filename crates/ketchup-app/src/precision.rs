//! Typed dimensions are converted to meters before reaching geometry operations.
use super::*;

pub(super) const MAX_INPUT_LEN: usize = 512;
const FORMULA_EXAMPLE: &str = "Enter a formula such as 60cm - 89mm or (1m + 20cm) / 2";

pub(super) fn length(input: &str) -> Result<f64, &'static str> {
    let value = Formula::parse(input, true)?;
    if matches!(value.dimension, 0 | 1) {
        Ok(value.number)
    } else {
        Err("The formula must produce a length, not an area or volume")
    }
}

fn unit_factor(unit: &str) -> Result<f64, &'static str> {
    Ok(match unit.to_ascii_lowercase().as_str() {
        "m" | "meter" | "meters" | "metre" | "metres" => 1.,
        "mm" | "millimeter" | "millimeters" | "millimetre" | "millimetres" => 0.001,
        "cm" | "centimeter" | "centimeters" | "centimetre" | "centimetres" => 0.01,
        "km" | "kilometer" | "kilometers" | "kilometre" | "kilometres" => 1000.,
        "in" | "inch" | "inches" | "\"" => 0.0254,
        "ft" | "foot" | "feet" | "'" => 0.3048,
        "yd" | "yard" | "yards" => 0.9144,
        _ => return Err("Use length units mm, cm, m, km, in, ft, or yd"),
    })
}

#[derive(Clone, Copy)]
struct Quantity {
    number: f64,
    // Bare numbers are scalars in products and default to meters in sums.
    dimension: i32,
}

impl Quantity {
    fn finite(self) -> Result<Self, &'static str> {
        if self.number.is_finite() {
            Ok(self)
        } else {
            Err("The formula must produce a finite value")
        }
    }
}

struct Formula<'a> {
    input: &'a str,
    cursor: usize,
    depth: usize,
    allow_units: bool,
}

impl<'a> Formula<'a> {
    fn parse(input: &'a str, allow_units: bool) -> Result<Quantity, &'static str> {
        if input.len() > MAX_INPUT_LEN {
            return Err("The formula is too long");
        }
        let mut parser = Self {
            input,
            cursor: 0,
            depth: 0,
            allow_units,
        };
        let value = parser.sum()?;
        if parser.peek().is_some() {
            return Err(FORMULA_EXAMPLE);
        }
        value.finite()
    }

    fn peek(&mut self) -> Option<u8> {
        while self
            .input
            .as_bytes()
            .get(self.cursor)
            .is_some_and(u8::is_ascii_whitespace)
        {
            self.cursor += 1;
        }
        self.input.as_bytes().get(self.cursor).copied()
    }

    fn sum(&mut self) -> Result<Quantity, &'static str> {
        let mut value = self.product()?;
        while let Some(operator @ (b'+' | b'-')) = self.peek() {
            self.cursor += 1;
            let mut rhs = self.product()?;
            if value.dimension != rhs.dimension {
                if matches!((value.dimension, rhs.dimension), (0, 1) | (1, 0)) {
                    value.dimension = 1;
                    rhs.dimension = 1;
                } else {
                    return Err("Add or subtract quantities with matching units");
                }
            }
            value.number = if operator == b'+' {
                value.number + rhs.number
            } else {
                value.number - rhs.number
            };
            value = value.finite()?;
        }
        Ok(value)
    }

    fn product(&mut self) -> Result<Quantity, &'static str> {
        let mut value = self.unary()?;
        while let Some(operator @ (b'*' | b'/')) = self.peek() {
            self.cursor += 1;
            let rhs = self.unary()?;
            if operator == b'*' {
                value.number *= rhs.number;
                value.dimension += rhs.dimension;
            } else {
                if rhs.number == 0. {
                    return Err("Cannot divide by zero");
                }
                value.number /= rhs.number;
                value.dimension -= rhs.dimension;
            }
            value = value.finite()?;
        }
        Ok(value)
    }

    fn unary(&mut self) -> Result<Quantity, &'static str> {
        let mut sign = 1.;
        while let Some(operator @ (b'+' | b'-')) = self.peek() {
            self.cursor += 1;
            if operator == b'-' {
                sign = -sign;
            }
        }
        let mut value = self.primary()?;
        value.number *= sign;
        Ok(value)
    }

    fn primary(&mut self) -> Result<Quantity, &'static str> {
        let mut value = if self.peek() == Some(b'(') {
            if self.depth >= 32 {
                return Err("Use fewer nested parentheses");
            }
            self.depth += 1;
            self.cursor += 1;
            let value = self.sum()?;
            if self.peek() != Some(b')') {
                return Err("Close each opening parenthesis");
            }
            self.cursor += 1;
            self.depth -= 1;
            value
        } else {
            let start = self.cursor;
            let bytes = self.input.as_bytes();
            while bytes.get(self.cursor).is_some_and(u8::is_ascii_digit) {
                self.cursor += 1;
            }
            if bytes.get(self.cursor) == Some(&b'.') {
                self.cursor += 1;
                while bytes.get(self.cursor).is_some_and(u8::is_ascii_digit) {
                    self.cursor += 1;
                }
            }
            if matches!(bytes.get(self.cursor), Some(b'e' | b'E')) {
                self.cursor += 1;
                if matches!(bytes.get(self.cursor), Some(b'+' | b'-')) {
                    self.cursor += 1;
                }
                while bytes.get(self.cursor).is_some_and(u8::is_ascii_digit) {
                    self.cursor += 1;
                }
            }
            Quantity {
                number: self.input[start..self.cursor]
                    .parse::<f64>()
                    .map_err(|_| FORMULA_EXAMPLE)?,
                dimension: 0,
            }
            .finite()?
        };
        self.peek();
        let start = self.cursor;
        if matches!(self.input.as_bytes().get(self.cursor), Some(b'\'' | b'"')) {
            self.cursor += 1;
        } else {
            while self
                .input
                .as_bytes()
                .get(self.cursor)
                .is_some_and(u8::is_ascii_alphabetic)
            {
                self.cursor += 1;
            }
        }
        if self.cursor > start {
            if !self.allow_units {
                return Err("Scale factors must use numbers without units");
            }
            value.number *= unit_factor(&self.input[start..self.cursor])?;
            value.dimension += 1;
        }
        value.finite()
    }
}

pub(super) fn positive_length(input: &str) -> Result<f64, &'static str> {
    let value = length(input)?;
    if value > 1e-7 && value <= 1e7 {
        Ok(value)
    } else {
        Err("Enter a positive length between 0.0000001 m and 10000000 m")
    }
}

pub(super) fn modeling_value(tool: Tool, input: &str) -> Result<f64, &'static str> {
    if tool != Tool::Scale {
        return length(input);
    }
    Formula::parse(input, false).map(|value| value.number)
}

impl KetchupApp {
    pub(super) fn line_preview_snap(&self) -> Option<Snap> {
        let mut snap = self.snap?;
        if self.tool == Tool::Line
            && let Some(start) = self.line_start
            && let Ok(length) = positive_length(&self.length_input)
            && let Some(direction) = (snap.point - start).try_normalize()
        {
            snap.point = start + direction * length;
            if self
                .drawing_axis_guides
                .iter()
                .chain(self.drawing_guide.iter())
                .any(|g| (snap.point - g.origin).cross(g.axis.vector()).length() > 1e-6)
            {
                snap.kind = self
                    .preview_alignment()
                    .map(Inference::Axis)
                    .unwrap_or(Inference::Plane);
            } else if !matches!(snap.kind, Inference::Axis(_) | Inference::OnAxis(_)) {
                snap.kind = Inference::Plane;
            }
        }
        Some(snap)
    }

    pub(super) fn precision_available(&self) -> bool {
        (self.tool == Tool::Line && self.line_start.is_some())
            || self.drawing.is_some()
            || self.modeling.is_some()
            || self.last_move_copy.is_some()
    }

    pub(super) fn precise_drawing_points(&self) -> Result<Vec<DVec3>, &'static str> {
        let points = self.raw_drawing_points();
        let Some(drawing) = &self.drawing else {
            return Ok(points);
        };
        if self.length_input.trim().is_empty() {
            return Ok(points);
        }
        let origin = drawing.origin;
        let delta = self.snap.map_or(DVec3::ZERO, |snap| snap.point - origin);
        match self.tool {
            Tool::Rectangle => {
                let (width, height) = self
                    .length_input
                    .split_once(',')
                    .ok_or("Enter width,height, such as 5m,50mm")?;
                let width = positive_length(width)?;
                let height = positive_length(height)?;
                let direction = |value: f64| if value < 0. { -1. } else { 1. };
                let a = drawing.u * width * direction(delta.dot(drawing.u));
                let b = drawing.v * height * direction(delta.dot(drawing.v));
                Ok(vec![origin, origin + a, origin + a + b, origin + b, origin])
            }
            Tool::Circle | Tool::Polygon => {
                let radius = positive_length(&self.length_input)?;
                let delta = delta.try_normalize().unwrap_or(drawing.u) * radius;
                let tangent = drawing.normal.cross(delta);
                let sides = if self.tool == Tool::Circle {
                    48
                } else {
                    self.polygon_sides
                };
                Ok((0..=sides)
                    .map(|i| {
                        let angle = (i % sides) as f64 * std::f64::consts::TAU / sides as f64;
                        origin + delta * angle.cos() + tangent * angle.sin()
                    })
                    .collect())
            }
            Tool::Freehand => {
                let target = positive_length(&self.length_input)?;
                let current: f64 = points.windows(2).map(|p| p[0].distance(p[1])).sum();
                if current <= 1e-7 {
                    return Err("Draw a stroke before setting its total length");
                }
                Ok(points
                    .into_iter()
                    .map(|p| origin + (p - origin) * (target / current))
                    .collect())
            }
            _ => Ok(points),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lengths_convert_explicit_units_and_default_to_meters() {
        for (input, expected) in [
            ("5", 5.),
            ("5m", 5.),
            ("50mm", 0.05),
            ("5cm", 0.05),
            (" 5 CM ", 0.05),
            ("-50 mm", -0.05),
            ("1e2cm", 1.),
            ("2km", 2000.),
            ("2in", 0.0508),
            ("2\"", 0.0508),
            ("3ft", 0.9144),
            ("3'", 0.9144),
            ("1yd", 0.9144),
        ] {
            assert!((length(input).unwrap() - expected).abs() < 1e-10, "{input}");
        }
    }

    #[test]
    fn invalid_units_and_nonfinite_dimensions_are_rejected() {
        for input in ["", "m", "5kg", "5m,50mm", "1.2.3cm", "NaN", "inf", "1e999m"] {
            assert!(length(input).is_err(), "{input}");
        }
        for input in ["0cm", "-5cm", "1e-12mm", "1e9km"] {
            assert!(positive_length(input).is_err(), "{input}");
        }
        assert_eq!(modeling_value(Tool::Scale, "0.5"), Ok(0.5));
        assert!(modeling_value(Tool::Scale, "5cm").is_err());
    }

    #[test]
    fn formulas_respect_units_precedence_parentheses_and_signs() {
        for (input, expected) in [
            ("60cm - 89mm", 0.511),
            ("60cm-89mm", 0.511),
            ("(60cm - 89mm) / 2", 0.2555),
            ("(60 - 8.9)cm", 0.511),
            ("5cm * 2", 0.1),
            ("2 * 5cm", 0.1),
            ("1m + 2 * 5cm", 1.1),
            ("1 + 2 * 3", 7.),
            ("1m + 2", 3.),
            ("-(60cm - 89mm)", -0.511),
            ("60cm - -89mm", 0.689),
            ("1e-2m + 5mm", 0.015),
            ("1m / (2m / 3m)", 1.5),
            ("(2m * 3m) / 1m", 6.),
            ("2ft - 6in", 0.4572),
        ] {
            assert!((length(input).unwrap() - expected).abs() < 1e-10, "{input}");
        }
        assert_eq!(modeling_value(Tool::Scale, "(1 + 2) / 6"), Ok(0.5));
    }

    #[test]
    fn invalid_formulas_do_not_produce_a_length() {
        for input in [
            "60cm -",
            "1m / 0",
            "1m / (1 - 1)",
            "(1m + 5cm",
            "1m + 5cm)",
            "1m * 2m",
            "(2m)cm",
            "1m + 2m * 3m",
            "1m + 5kg",
            "2(5cm)",
            "1e-",
            "1e308 * 1e308",
            "sqrt(2m)",
            "()",
            "1m / / 2",
            "1m;2m",
        ] {
            assert!(length(input).is_err(), "{input}");
        }
        assert!(positive_length("89mm - 60cm").is_err());
        assert!(length(&"1".repeat(MAX_INPUT_LEN + 1)).is_err());
        assert!(length(&format!("{}1m{}", "(".repeat(33), ")".repeat(33))).is_err());
        assert!(modeling_value(Tool::Scale, "1m / 2m").is_err());
    }
}
