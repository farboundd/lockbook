use super::format_bytes;

#[derive(Clone, Copy)]
pub enum Unit {
    Count,
    Bytes,
    Percent,
    Hours,
}

impl Unit {
    pub fn label(self, value: f64) -> String {
        match self {
            Self::Count => format!("{value:.0}"),
            Self::Bytes => format_bytes(value as u64),
            Self::Percent => format!("{value:.1}%"),
            Self::Hours => format!("{value:.1} h"),
        }
    }
}

pub struct YAxis {
    unit: Unit,
    scale: f64,
    suffix: &'static str,
    first: f64,
    last: f64,
    step: f64,
}

impl YAxis {
    pub fn new(unit: Unit, values: impl Iterator<Item = f64>) -> Self {
        if matches!(unit, Unit::Percent) {
            return Self { unit, scale: 1.0, suffix: "%", first: 0.0, last: 4.0, step: 25.0 };
        }

        let (min, max) = values
            .filter(|value| value.is_finite() && *value >= 0.0)
            .fold((f64::INFINITY, 0.0_f64), |(min, max), value| (min.min(value), max.max(value)));
        let (scale, suffix) = match unit {
            Unit::Bytes => {
                [(1_u64 << 40, " TiB"), (1 << 30, " GiB"), (1 << 20, " MiB"), (1 << 10, " KiB")]
                    .into_iter()
                    .find(|&(scale, _)| max >= scale as f64)
                    .map_or((1.0, " B"), |(scale, suffix)| (scale as f64, suffix))
            }
            Unit::Hours => (1.0, " h"),
            _ => (1.0, ""),
        };
        let max = max / scale;
        let min = min / scale;
        let lower = if max > 0.0 && min >= max * 0.8 {
            (min - ((max - min) * 0.1).max(max * 0.02)).max(0.0)
        } else {
            0.0
        };
        let upper = if max > 0.0 { max * 1.02 } else { 1.0 };
        let min_step = match unit {
            Unit::Count => 1.0,
            Unit::Bytes => 1.0 / scale,
            Unit::Hours => 0.1,
            Unit::Percent => unreachable!(),
        };
        let step = nice_step(((upper - lower) / 4.0).max(min_step));
        let first = (lower / step).floor();
        let last = (upper / step).ceil().max(first + 1.0);
        Self { unit, scale, suffix, first, last, step }
    }

    pub fn fraction(&self, value: f64) -> f64 {
        ((value / self.scale / self.step - self.first) / (self.last - self.first)).clamp(0.0, 1.0)
    }

    pub fn ticks(&self) -> impl Iterator<Item = (f64, String)> + '_ {
        let digits = match self.unit {
            Unit::Count | Unit::Percent => 0,
            _ => (-self.step.log10().floor()).max(0.0) as usize,
        };
        (0..=(self.last - self.first) as usize).map(move |i| {
            let tick = (self.first + i as f64) * self.step;
            (tick * self.scale, format!("{tick:.digits$}{}", self.suffix))
        })
    }
}

fn nice_step(step: f64) -> f64 {
    let magnitude = 10.0_f64.powf(step.log10().floor());
    let normalized = step / magnitude;
    let multiple = if normalized < 1.5 {
        1.0
    } else if normalized < 3.5 {
        2.0
    } else if normalized < 7.5 {
        5.0
    } else {
        10.0
    };
    magnitude * multiple
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_use_unique_integer_ticks_even_with_one_user() {
        let axis = YAxis::new(Unit::Count, [0.0, 1.0].into_iter());
        assert_eq!(axis.ticks().map(|(_, text)| text).collect::<Vec<_>>(), ["0", "1", "2"]);
    }

    #[test]
    fn zero_based_counts_keep_moderate_headroom() {
        let axis = YAxis::new(Unit::Count, [0.0, 100.0].into_iter());
        assert_eq!(axis.ticks().last().unwrap().0, 120.0);
    }

    #[test]
    fn tightly_clustered_totals_show_their_variation() {
        let axis = YAxis::new(Unit::Count, [9_980.0, 10_020.0].into_iter());
        assert!(axis.ticks().next().unwrap().0 > 0.0);
        assert!(axis.fraction(10_020.0) - axis.fraction(9_980.0) >= 0.05);
        assert!(axis.fraction(9_980.0) > 0.0 && axis.fraction(10_020.0) < 1.0);
    }

    #[test]
    fn percent_ticks_stay_fixed_and_byte_ticks_share_a_unit() {
        let percent = YAxis::new(Unit::Percent, [7.0, 13.0].into_iter());
        assert_eq!(
            percent.ticks().map(|(_, text)| text).collect::<Vec<_>>(),
            ["0%", "25%", "50%", "75%", "100%",]
        );
        let bytes = YAxis::new(Unit::Bytes, [0.0, 1_500_000.0].into_iter());
        let ticks = bytes.ticks().map(|(_, text)| text).collect::<Vec<_>>();
        assert!(ticks.iter().all(|label| label.ends_with(" MiB")));
        assert!(ticks.windows(2).all(|pair| pair[0] != pair[1]));
    }
}
