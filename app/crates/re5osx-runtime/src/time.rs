//! Clocks and time zones, as the browser provides them.
//!
//! The system's time zone comes from jiff, the same on every platform: C's
//! `localtime_r`/`mktime` differ between them (Windows has no `tm_gmtoff`).

use std::time::{Instant, SystemTime, UNIX_EPOCH};

use jiff::civil::{Date, DateTime};
use jiff::tz::TimeZone;
use jiff::{Span, Timestamp, Zoned};

use crate::memory::Mem;

/// `performance.timeOrigin + performance.now()`: epoch milliseconds that never
/// go backwards.
pub(crate) struct Clock {
    origin_ms: f64,
    start: Instant,
}

impl Clock {
    pub fn new() -> Self {
        Clock { origin_ms: date_now(), start: Instant::now() }
    }

    pub fn now_ms(&self) -> f64 {
        self.origin_ms + self.start.elapsed().as_secs_f64() * 1000.0
    }
}

/// `Date.now()`.
pub(crate) fn date_now() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0.0, |d| d.as_millis() as f64)
}

/// Timed waits that end on time. Windows wakes them at its timer tick,
/// 15.6 ms by default: the engine's short sleeps (Emscripten's, futex waits
/// with a timeout) would last that long. 1 ms, as games ask for; for this
/// process only since Windows 10 2004. Elsewhere waits are precise already.
pub(crate) fn fine_timers() {
    #[cfg(windows)]
    unsafe {
        windows_sys::Win32::Media::timeBeginPeriod(1);
    }
}

// struct tm (wasm64): nine i32 fields, then tm_gmtoff (i64) at 40.
const TM_SEC: u64 = 0;
const TM_MIN: u64 = 4;
const TM_HOUR: u64 = 8;
const TM_MDAY: u64 = 12;
const TM_MON: u64 = 16;
const TM_YEAR: u64 = 20;
const TM_WDAY: u64 = 24;
const TM_YDAY: u64 = 28;
const TM_ISDST: u64 = 32;
const TM_GMTOFF: u64 = 40;

fn write_tm(mem: Mem, at: u64, z: &Zoned, local: bool) {
    mem.set_i32(at + TM_SEC, z.second().into());
    mem.set_i32(at + TM_MIN, z.minute().into());
    mem.set_i32(at + TM_HOUR, z.hour().into());
    mem.set_i32(at + TM_MDAY, z.day().into());
    mem.set_i32(at + TM_MON, i32::from(z.month()) - 1);
    mem.set_i32(at + TM_YEAR, i32::from(z.year()) - 1900);
    mem.set_i32(at + TM_WDAY, z.weekday().to_sunday_zero_offset().into());
    mem.set_i32(at + TM_YDAY, i32::from(z.day_of_year()) - 1);
    if local {
        let dst = z.time_zone().to_offset_info(z.timestamp()).dst().is_dst();
        mem.set_i32(at + TM_ISDST, dst as i32);
        mem.set_i64(at + TM_GMTOFF, z.offset().seconds().into());
    }
}

fn zone(local: bool) -> TimeZone {
    if local { TimeZone::system() } else { TimeZone::UTC }
}

/// `_localtime_js` / `_gmtime_js`.
pub(crate) fn tm_from_time(mem: Mem, t: i64, at: u64, local: bool) -> u32 {
    match Timestamp::from_second(t) {
        Ok(ts) => {
            write_tm(mem, at, &ts.to_zoned(zone(local)), local);
            0
        }
        Err(_) => 1,
    }
}

/// `_mktime_js`: normalises the struct in place, returns the time (or -1).
pub(crate) fn mktime(mem: Mem, at: u64) -> i64 {
    let tz = TimeZone::system();
    let Some(ts) = local_time(mem, at, &tz) else { return -1 };
    write_tm(mem, at, &ts.to_zoned(tz), true);
    ts.as_second()
}

/// The time the struct's local fields name, out-of-range fields carried
/// over (31 January + 1 month is 3 March), as C's `mktime` does. A
/// `tm_isdst` of 0 or 1 that contradicts the zone at that time reads the
/// fields with the other offset, as glibc and the browser build do.
fn local_time(mem: Mem, at: u64, tz: &TimeZone) -> Option<Timestamp> {
    let year = i16::try_from(mem.i32(at + TM_YEAR).checked_add(1900)?).ok()?;
    let span = Span::new()
        .try_months(mem.i32(at + TM_MON))
        .ok()?
        .try_days(mem.i32(at + TM_MDAY) - 1)
        .ok()?
        .try_hours(mem.i32(at + TM_HOUR))
        .ok()?
        .try_minutes(mem.i32(at + TM_MIN))
        .ok()?
        .try_seconds(mem.i32(at + TM_SEC))
        .ok()?;
    let civil = Date::new(year, 1, 1).ok()?.at(0, 0, 0, 0).checked_add(span).ok()?;
    let ts = tz.to_ambiguous_timestamp(civil).compatible().ok()?;
    let isdst = mem.i32(at + TM_ISDST);
    if isdst < 0 || tz.to_offset_info(ts).dst().is_dst() == (isdst > 0) {
        return Some(ts);
    }
    let (winter, summer) = year_offsets(tz, civil.year())?;
    let (standard, daylight) = (winter.min(summer), winter.max(summer));
    let wanted = if isdst > 0 { daylight } else { standard };
    let actual = i64::from(tz.to_offset(ts).seconds());
    Timestamp::from_second(ts.as_second() + actual - wanted).ok()
}

/// The zone's UTC offsets in seconds on 1 January and 1 July of `year`.
fn year_offsets(tz: &TimeZone, year: i16) -> Option<(i64, i64)> {
    let at = |month: i8| -> Option<i64> {
        let civil: DateTime = Date::new(year, month, 1).ok()?.at(0, 0, 0, 0);
        let ts = tz.to_ambiguous_timestamp(civil).compatible().ok()?;
        Some(tz.to_offset(ts).seconds().into())
    };
    Some((at(1)?, at(7)?))
}

/// `_tzset_js(timezone, daylight, std_name, dst_name)`.
pub(crate) fn tzset(mem: Mem, timezone: u64, daylight: u64, std_name: u64, dst_name: u64) {
    let tz = TimeZone::system();
    let year = Timestamp::now().to_zoned(tz.clone()).year();
    // JS offsets: minutes, positive west of UTC.
    let (winter, summer) = year_offsets(&tz, year).map_or((0, 0), |(w, s)| (-w / 60, -s / 60));
    mem.set_u64(timezone, (winter.max(summer) * 60) as u64);
    mem.set_i32(daylight, (winter != summer) as i32);
    let zone = |off: i64| format!("UTC{}{:02}{:02}", if off >= 0 { '-' } else { '+' }, off.abs() / 60, off.abs() % 60);
    let (w, s) = (zone(winter), zone(summer));
    if summer < winter {
        mem.write_str(std_name, &w, 17);
        mem.write_str(dst_name, &s, 17);
    } else {
        mem.write_str(dst_name, &w, 17);
        mem.write_str(std_name, &s, 17);
    }
}
