//! Per-object sanding and finishing configuration, saved with project geometry.
const GRITS: [u16; 6] = [80, 120, 150, 180, 220, 320];
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Step {
    pub enabled: bool,
    /// Stable ID in the finishing-product catalog; unassigned for older data.
    pub product_id: Option<crate::MaterialId>,
    pub minutes_per_m2: f64,
    pub coats: u32,
    pub drying_hours: f64,
    // None preserves the original per-coat wait for existing saved settings.
    pub final_drying_hours: Option<f64>,
}

impl Default for Step {
    fn default() -> Self {
        Self {
            enabled: false,
            product_id: None,
            minutes_per_m2: 16.,
            coats: 1,
            drying_hours: 0.,
            final_drying_hours: None,
        }
    }
}

impl Step {
    // Coat numbers are one-based; only the last coat uses the final wait.
    pub fn coat_estimate(&self, area: f64, coat: u32) -> Duration {
        if !self.enabled || area <= 0. {
            return Duration::default();
        }
        Duration {
            work: area * self.minutes_per_m2,
            wait: if coat == self.coats {
                self.final_drying_hours.unwrap_or(self.drying_hours)
            } else {
                self.drying_hours
            } * 60.,
        }
    }

    pub fn sanitize(&mut self, defaults: &Self, sanding: bool) {
        if !self.minutes_per_m2.is_finite() || self.minutes_per_m2 < 0. {
            self.minutes_per_m2 = defaults.minutes_per_m2;
        }
        if !self.drying_hours.is_finite() || self.drying_hours < 0. {
            self.drying_hours = defaults.drying_hours;
        }
        if self
            .final_drying_hours
            .is_some_and(|hours| !hours.is_finite() || hours < 0.)
        {
            self.final_drying_hours = defaults.final_drying_hours;
        }
        self.coats = self.coats.clamp(1, 100);
        if sanding {
            self.product_id = None;
            self.coats = 1;
            self.drying_hours = 0.;
            self.final_drying_hours = None;
        }
    }

    pub fn estimate(&self, area: f64) -> Duration {
        if !self.enabled || area <= 0. {
            return Duration::default();
        }
        Duration {
            work: area * self.minutes_per_m2 * f64::from(self.coats),
            wait: (self.drying_hours * f64::from(self.coats - 1)
                + self.final_drying_hours.unwrap_or(self.drying_hours))
                * 60.,
        }
    }
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Settings {
    pub sanding: [Step; 6],
    pub sanding_after_varnish: [bool; 6],
    pub pre_stain: Step,
    pub stain: Step,
    pub varnish: Step,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            sanding: std::array::from_fn(|_| Step::default()),
            sanding_after_varnish: [false; 6],
            pre_stain: Step {
                minutes_per_m2: 5.,
                drying_hours: 0.25,
                ..Step::default()
            },
            stain: Step {
                minutes_per_m2: 5.,
                drying_hours: 8.,
                ..Step::default()
            },
            varnish: Step {
                minutes_per_m2: 6.,
                coats: 3,
                drying_hours: 6.,
                final_drying_hours: Some(24.),
                ..Step::default()
            },
        }
    }
}

impl Settings {
    pub fn sanitize(&mut self) {
        let defaults = Self::default();
        for (step, default) in self.sanding.iter_mut().zip(&defaults.sanding) {
            step.sanitize(default, true);
        }
        self.pre_stain.sanitize(&defaults.pre_stain, false);
        self.stain.sanitize(&defaults.stain, false);
        self.varnish.sanitize(&defaults.varnish, false);
    }

    pub fn any_enabled(&self) -> bool {
        self.task_estimates(0.).next().is_some()
    }

    pub fn task_estimates(&self, area: f64) -> impl Iterator<Item = (String, Duration)> {
        let mut tasks = Vec::new();
        for ((step, grit), between_coats) in self
            .sanding
            .iter()
            .zip(GRITS)
            .zip(self.sanding_after_varnish)
        {
            if step.enabled && !between_coats {
                tasks.push((format!("Sanding · {grit} grit"), step.estimate(area)));
            }
        }
        for (title, step, varnish) in [
            ("Pre-stain", &self.pre_stain, false),
            ("Stain", &self.stain, false),
            ("Varnish", &self.varnish, true),
        ] {
            if !step.enabled {
                continue;
            }
            for coat in 1..=step.coats {
                tasks.push((
                    format!("{title} · coat {coat}"),
                    step.coat_estimate(area, coat),
                ));
                if varnish && coat < step.coats {
                    for ((sanding, grit), between_coats) in self
                        .sanding
                        .iter()
                        .zip(GRITS)
                        .zip(self.sanding_after_varnish)
                    {
                        if sanding.enabled && between_coats {
                            tasks.push((
                                format!("Sanding · {grit} grit (after coat {coat})"),
                                sanding.estimate(area),
                            ));
                        }
                    }
                }
            }
        }
        tasks.into_iter()
    }

    pub fn estimate(&self, area: f64) -> Duration {
        self.task_estimates(area)
            .fold(Duration::default(), |total, (_, step)| Duration {
                work: total.work + step.work,
                wait: total.wait + step.wait,
            })
    }
}

#[derive(Default, Debug, PartialEq)]
pub struct Duration {
    pub work: f64,
    pub wait: f64,
}
