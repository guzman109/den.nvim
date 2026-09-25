//! Sunrise and sunset, worked out offline from a location.
//!
//! The standard sunrise equation (as NOAA and most almanacs use it),
//! including refraction and the size of the sun's disc. It is good to a
//! minute or two away from the poles, which is plenty for "sunset in 40
//! minutes".

use jiff::Timestamp;
use jiff::civil::Date;

const J2000: f64 = 2_451_545.0;
const UNIX_EPOCH_JD: f64 = 2_440_587.5;

fn sin(deg: f64) -> f64 {
    deg.to_radians().sin()
}

fn cos(deg: f64) -> f64 {
    deg.to_radians().cos()
}

/// The sun's day at a place, or why it has no sunrise and sunset.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SunDay {
    Normal {
        rise: Timestamp,
        set: Timestamp,
    },
    /// The sun never sets (summer near the poles).
    AlwaysUp,
    /// The sun never rises (winter near the poles).
    AlwaysDown,
}

/// Sunrise and sunset on a calendar date at a latitude and longitude
/// (degrees, north and east positive).
pub fn day(date: Date, lat: f64, lon: f64) -> SunDay {
    let noon_utc = date
        .to_zoned(jiff::tz::TimeZone::UTC)
        .map(|z| z.timestamp().as_second())
        .unwrap_or(0)
        + 43_200;
    let jd_noon = noon_utc as f64 / 86_400.0 + UNIX_EPOCH_JD;
    let n = (jd_noon - J2000 + 0.0008).round();
    let mean_noon = n - lon / 360.0;
    let anomaly = (357.5291 + 0.985_600_28 * mean_noon).rem_euclid(360.0);
    let center = 1.9148 * sin(anomaly) + 0.02 * sin(2.0 * anomaly) + 0.0003 * sin(3.0 * anomaly);
    let longitude = (anomaly + center + 180.0 + 102.9372).rem_euclid(360.0);
    let transit = J2000 + mean_noon + 0.0053 * sin(anomaly) - 0.0069 * sin(2.0 * longitude);
    let declination = (sin(longitude) * sin(23.4397)).asin().to_degrees();
    let cos_hour = (sin(-0.833) - sin(lat) * sin(declination)) / (cos(lat) * cos(declination));
    if cos_hour < -1.0 {
        return SunDay::AlwaysUp;
    }
    if cos_hour > 1.0 {
        return SunDay::AlwaysDown;
    }
    let hour = cos_hour.acos().to_degrees();
    let at = |jd: f64| {
        let seconds = ((jd - UNIX_EPOCH_JD) * 86_400.0).round() as i64;
        Timestamp::from_second(seconds).unwrap_or(Timestamp::UNIX_EPOCH)
    };
    SunDay::Normal {
        rise: at(transit - hour / 360.0),
        set: at(transit + hour / 360.0),
    }
}

/// Sunset on a date, if the sun sets there that day.
pub fn sunset(date: Date, lat: f64, lon: f64) -> Option<Timestamp> {
    match day(date, lat, lon) {
        SunDay::Normal { set, .. } => Some(set),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jiff::civil::date;

    fn near(got: Timestamp, want: &str) {
        let want: Timestamp = want.parse().unwrap_or(Timestamp::UNIX_EPOCH);
        let off = (got.as_second() - want.as_second()).abs();
        assert!(off <= 90, "got {got}, want {want} (±90 s), off by {off} s");
    }

    #[test]
    fn matches_noaa() {
        // NOAA's solar calculator, in UTC.
        let columbus = day(date(2026, 9, 24), 39.96, -83.0);
        let SunDay::Normal { rise, set } = columbus else {
            panic!("{columbus:?}");
        };
        near(rise, "2026-09-24T11:21:37Z");
        near(set, "2026-09-24T23:25:39Z");
        near(
            sunset(date(2016, 1, 1), 43.6532, -79.3832).unwrap_or(Timestamp::UNIX_EPOCH),
            "2016-01-01T21:51:00Z",
        );
        near(
            sunset(date(2026, 6, 21), 51.5074, -0.1278).unwrap_or(Timestamp::UNIX_EPOCH),
            "2026-06-21T20:21:35Z",
        );
        near(
            sunset(date(2026, 3, 20), -33.8688, 151.2093).unwrap_or(Timestamp::UNIX_EPOCH),
            "2026-03-20T08:06:57Z",
        );
    }

    #[test]
    fn near_the_poles_some_days_have_no_sunset() {
        assert_eq!(day(date(2026, 6, 21), 69.65, 18.96), SunDay::AlwaysUp);
        assert_eq!(day(date(2026, 12, 21), 69.65, 18.96), SunDay::AlwaysDown);
        assert_eq!(sunset(date(2026, 6, 21), 69.65, 18.96), None);
    }
}
