// Copyright 2012-2013 The Rust Project Developers. See the COPYRIGHT
// file at the top-level directory of this distribution and at
// http://rust-lang.org/COPYRIGHT.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

//! Simple time handling.

#![doc(html_logo_url = "http://www.rust-lang.org/logos/rust-logo-128x128-blk-v2.png",
       html_favicon_url = "http://www.rust-lang.org/favicon.ico",
       html_root_url = "http://doc.rust-lang.org/time/")]
#![cfg_attr(feature = "std-duration", feature(std_misc))]
#![allow(trivial_numeric_casts)]
#![cfg_attr(test, deny(warnings))]
#![cfg_attr(test, feature(test, str_char))]

#[cfg(test)] #[macro_use] extern crate log;

extern crate libc;
#[cfg(feature = "rustc-serialize")]
extern crate rustc_serialize;

use std::cmp::Ordering;
use std::fmt;
use std::ops::{Add, Sub};
use std::io;

#[cfg(feature = "std-duration")]      pub use std::time::Duration;
#[cfg(not(feature = "std-duration"))] pub use duration::Duration;

use self::ParseError::{InvalidDay, InvalidDayOfMonth, InvalidDayOfWeek,
                       InvalidDayOfYear, InvalidFormatSpecifier, InvalidHour,
                       InvalidMinute, InvalidMonth, InvalidSecond, InvalidTime,
                       InvalidYear, InvalidZoneOffset, InvalidSecondsSinceEpoch,
                       MissingFormatConverter, UnexpectedCharacter};

pub use parse::strptime;

mod display;
mod parse;
#[cfg(not(feature = "std-duration"))]
mod duration;

static NSEC_PER_SEC: i32 = 1_000_000_000;

mod rustrt {
    use super::Tm;

    extern {
        pub fn rust_time_gmtime(sec: i64, nsec: i32, result: &mut Tm);
        pub fn rust_time_localtime(sec: i64, nsec: i32, result: &mut Tm) -> i32;
        pub fn rust_time_timegm(tm: &Tm) -> i64;
        pub fn rust_time_mktime(tm: &Tm) -> i64;
    }
}

#[cfg(all(unix, not(target_os = "macos"), not(target_os = "ios")))]
mod imp {
    use libc::{c_int, timespec};

    #[cfg(all(not(target_os = "android"),
              not(target_os = "bitrig"),
              not(target_os = "nacl"),
              not(target_os = "openbsd")))]
    #[link(name = "rt")]
    extern {}

    extern {
        pub fn clock_gettime(clk_id: c_int, tp: *mut timespec) -> c_int;
    }

}
#[cfg(any(target_os = "macos", target_os = "ios"))]
mod imp {
    use libc::{timeval, timezone, c_int, mach_timebase_info};
    use std::sync::{Once, ONCE_INIT};

    extern {
        pub fn gettimeofday(tp: *mut timeval, tzp: *mut timezone) -> c_int;
        pub fn mach_absolute_time() -> u64;
        pub fn mach_timebase_info(info: *mut mach_timebase_info) -> c_int;
    }

    pub fn info() -> &'static mach_timebase_info {
        static mut INFO: mach_timebase_info = mach_timebase_info {
            numer: 0,
            denom: 0,
        };
        static ONCE: Once = ONCE_INIT;

        unsafe {
            ONCE.call_once(|| {
                mach_timebase_info(&mut INFO);
            });
            &INFO
        }
    }
}

#[cfg(windows)]
mod imp {
    use libc;
    use std::sync::{Once, ONCE_INIT};

    pub fn frequency() -> libc::LARGE_INTEGER {
        static mut FREQUENCY: libc::LARGE_INTEGER = 0;
        static ONCE: Once = ONCE_INIT;

        unsafe {
            ONCE.call_once(|| {
                libc::QueryPerformanceFrequency(&mut FREQUENCY);
            });
            FREQUENCY
        }
    }
}

/// A record specifying a time value in seconds and nanoseconds.
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Debug)]
#[cfg_attr(feature = "rustc-serialize", derive(RustcEncodable, RustcDecodable))]
pub struct Timespec { pub sec: i64, pub nsec: i32 }
/*
 * Timespec assumes that pre-epoch Timespecs have negative sec and positive
 * nsec fields. Darwin's and Linux's struct timespec functions handle pre-
 * epoch timestamps using a "two steps back, one step forward" representation,
 * though the man pages do not actually document this. For example, the time
 * -1.2 seconds before the epoch is represented by `Timespec { sec: -2_i64,
 * nsec: 800_000_000 }`.
 */
impl Timespec {
    pub fn new(sec: i64, nsec: i32) -> Timespec {
        assert!(nsec >= 0 && nsec < NSEC_PER_SEC);
        Timespec { sec: sec, nsec: nsec }
    }
}

impl Add<Duration> for Timespec {
    type Output = Timespec;

    fn add(self, other: Duration) -> Timespec {
        let d_sec = other.num_seconds();
        // It is safe to unwrap the nanoseconds, because there cannot be
        // more than one second left, which fits in i64 and in i32.
        let d_nsec = (other - Duration::seconds(d_sec))
                     .num_nanoseconds().unwrap() as i32;
        let mut sec = self.sec + d_sec;
        let mut nsec = self.nsec + d_nsec;
        if nsec >= NSEC_PER_SEC {
            nsec -= NSEC_PER_SEC;
            sec += 1;
        } else if nsec < 0 {
            nsec += NSEC_PER_SEC;
            sec -= 1;
        }
        Timespec::new(sec, nsec)
    }
}

impl Sub<Duration> for Timespec {
    type Output = Timespec;

    fn sub(self, other: Duration) -> Timespec {
        let d_sec = other.num_seconds();
        // It is safe to unwrap the nanoseconds, because there cannot be
        // more than one second left, which fits in i64 and in i32.
        let d_nsec = (other - Duration::seconds(d_sec))
                     .num_nanoseconds().unwrap() as i32;
        let mut sec = self.sec - d_sec;
        let mut nsec = self.nsec - d_nsec;
        if nsec >= NSEC_PER_SEC {
            nsec -= NSEC_PER_SEC;
            sec += 1;
        } else if nsec < 0 {
            nsec += NSEC_PER_SEC;
            sec -= 1;
        }
        Timespec::new(sec, nsec)
    }
}

impl Sub<Timespec> for Timespec {
    type Output = Duration;

    fn sub(self, other: Timespec) -> Duration {
        let sec = self.sec - other.sec;
        let nsec = self.nsec - other.nsec;
        Duration::seconds(sec) + Duration::nanoseconds(nsec as i64)
    }
}

/**
 * Returns the current time as a `timespec` containing the seconds and
 * nanoseconds since 1970-01-01T00:00:00Z.
 */
pub fn get_time() -> Timespec {
    unsafe {
        let (sec, nsec) = os_get_time();
        return Timespec::new(sec, nsec);
    }

    #[cfg(windows)]
    unsafe fn os_get_time() -> (i64, i32) {
        static MICROSECONDS_FROM_1601_TO_1970: u64 = 11644473600000000;

        let mut time = libc::FILETIME {
            dwLowDateTime: 0,
            dwHighDateTime: 0,
        };
        libc::GetSystemTimeAsFileTime(&mut time);

        // A FILETIME contains a 64-bit value representing the number of
        // hectonanosecond (100-nanosecond) intervals since 1601-01-01T00:00:00Z.
        // http://support.microsoft.com/kb/167296/en-us
        let us_since_1601 = (((time.dwHighDateTime as u64) << 32) |
                             ((time.dwLowDateTime  as u64) <<  0)) / 10;
        let us_since_1970 = us_since_1601 - MICROSECONDS_FROM_1601_TO_1970;

        ((us_since_1970 / 1000000) as i64,
         ((us_since_1970 % 1000000) * 1000) as i32)
    }

    #[cfg(any(target_os = "macos", target_os = "ios"))]
    unsafe fn os_get_time() -> (i64, i32) {
        use std::ptr;
        let mut tv = libc::timeval { tv_sec: 0, tv_usec: 0 };
        imp::gettimeofday(&mut tv, ptr::null_mut());
        (tv.tv_sec as i64, tv.tv_usec * 1000)
    }

    #[cfg(not(any(target_os = "macos", target_os = "ios", windows)))]
    unsafe fn os_get_time() -> (i64, i32) {
        let mut tv = libc::timespec { tv_sec: 0, tv_nsec: 0 };
        imp::clock_gettime(libc::CLOCK_REALTIME, &mut tv);
        (tv.tv_sec as i64, tv.tv_nsec as i32)
    }
}


/**
 * Returns the current value of a high-resolution performance counter
 * in nanoseconds since an unspecified epoch.
 */
pub fn precise_time_ns() -> u64 {
    return os_precise_time_ns();

    #[cfg(windows)]
    fn os_precise_time_ns() -> u64 {
        let mut ticks = 0;
        assert_eq!(unsafe {
            libc::QueryPerformanceCounter(&mut ticks)
        }, 1);

        mul_div_i64(ticks as i64, 1000000000, imp::frequency() as i64) as u64
    }

    #[cfg(any(target_os = "macos", target_os = "ios"))]
    fn os_precise_time_ns() -> u64 {
        unsafe {
            let time = imp::mach_absolute_time();
            let info = imp::info();
            time * info.numer as u64 / info.denom as u64
        }
    }

    #[cfg(not(any(windows, target_os = "macos", target_os = "ios")))]
    fn os_precise_time_ns() -> u64 {
        let mut ts = libc::timespec { tv_sec: 0, tv_nsec: 0 };
        unsafe {
            imp::clock_gettime(libc::CLOCK_MONOTONIC, &mut ts);
        }
        return (ts.tv_sec as u64) * 1000000000 + (ts.tv_nsec as u64)
    }
}


/**
 * Returns the current value of a high-resolution performance counter
 * in seconds since an unspecified epoch.
 */
pub fn precise_time_s() -> f64 {
    return (precise_time_ns() as f64) / 1000000000.;
}

/// An opaque structure representing a moment in time.
///
/// The only operation that can be performed on a `PreciseTime` is the
/// calculation of the `Duration` of time that lies between them.
///
/// # Examples
///
/// Repeatedly call a function for 1 second:
///
/// ```rust
/// # #![feature(std_misc)]
/// use time::{Duration, PreciseTime};
/// # fn do_some_work() {}
///
/// let start = PreciseTime::now();
///
/// while start.to(PreciseTime::now()) < Duration::seconds(1) {
///     do_some_work();
/// }
/// ```
#[derive(Copy, Clone)]
pub struct PreciseTime(u64);

impl PreciseTime {
    /// Returns a `PreciseTime` representing the current moment in time.
    pub fn now() -> PreciseTime {
        PreciseTime(precise_time_ns())
    }

    /// Returns a `Duration` representing the span of time from the value of
    /// `self` to the value of `later`.
    ///
    /// # Notes
    ///
    /// If `later` represents a time before `self`, the result of this method
    /// is unspecified.
    ///
    /// If `later` represents a time more than 293 years after `self`, the
    /// result of this method is unspecified.
    #[inline]
    pub fn to(&self, later: PreciseTime) -> Duration {
        // NB: even if later is less than self due to overflow, this will work
        // since the subtraction will underflow properly as well.
        //
        // We could deal with the overflow when casting to an i64, but all that
        // gets us is the ability to handle intervals of up to 584 years, which
        // seems not very useful :)
        Duration::nanoseconds((later.0 - self.0) as i64)
    }
}

/// A structure representing a moment in time.
///
/// `SteadyTime`s are generated by a "steady" clock, that is, a clock which
/// never experiences discontinuous jumps and for which time always flows at
/// the same rate.
///
/// # Examples
///
/// Repeatedly call a function for 1 second:
///
/// ```rust
/// # #![feature(std_misc)]
/// # use time::{Duration, SteadyTime};
/// # fn do_some_work() {}
/// let start = SteadyTime::now();
///
/// while SteadyTime::now() - start < Duration::seconds(1) {
///     do_some_work();
/// }
/// ```
#[derive(Clone, Copy, PartialOrd, Ord, PartialEq, Eq, Debug)]
pub struct SteadyTime(steady::SteadyTime);

impl SteadyTime {
    /// Returns a `SteadyTime` representing the current moment in time.
    pub fn now() -> SteadyTime {
        SteadyTime(steady::SteadyTime::now())
    }
}

impl fmt::Display for SteadyTime {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        // TODO: needs a display customization
        fmt::Debug::fmt(self, fmt)
    }
}

impl Sub for SteadyTime {
    type Output = Duration;

    fn sub(self, other: SteadyTime) -> Duration {
        self.0 - other.0
    }
}

impl Sub<Duration> for SteadyTime {
    type Output = SteadyTime;

    fn sub(self, other: Duration) -> SteadyTime {
        SteadyTime(self.0 - other)
    }
}

impl Add<Duration> for SteadyTime {
    type Output = SteadyTime;

    fn add(self, other: Duration) -> SteadyTime {
        SteadyTime(self.0 + other)
    }
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
mod steady {
    use imp;
    use Duration;
    use std::ops::{Sub, Add};

    #[derive(Clone, Copy, PartialOrd, Ord, PartialEq, Eq, Debug)]
    pub struct SteadyTime {
        t: u64,
    }

    impl SteadyTime {
        pub fn now() -> SteadyTime {
            SteadyTime {
                t: unsafe { imp::mach_absolute_time() },
            }
        }
    }

    impl Sub for SteadyTime {
        type Output = Duration;

        fn sub(self, other: SteadyTime) -> Duration {
            let info = imp::info();
            let diff = self.t as i64 - other.t as i64;
            Duration::nanoseconds(diff * info.numer as i64 / info.denom as i64)
        }
    }

    impl Sub<Duration> for SteadyTime {
        type Output = SteadyTime;

        fn sub(self, other: Duration) -> SteadyTime {
            self + -other
        }
    }

    impl Add<Duration> for SteadyTime {
        type Output = SteadyTime;

        fn add(self, other: Duration) -> SteadyTime {
            let info = imp::info();
            let delta = other.num_nanoseconds().unwrap() * info.denom as i64 / info.numer as i64;
            SteadyTime {
                t: (self.t as i64 + delta) as u64
            }
        }
    }
}

#[cfg(not(any(windows, target_os = "macos", target_os = "ios")))]
mod steady {
    use {imp, NSEC_PER_SEC, Duration};
    use libc;
    use std::cmp::{PartialOrd, Ord, Ordering, PartialEq, Eq};
    use std::ops::{Sub, Add};
    use std::fmt;

    #[derive(Copy)]
    pub struct SteadyTime {
        t: libc::timespec,
    }

    impl fmt::Debug for SteadyTime {
        fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
            write!(fmt, "SteadyTime {{ tv_sec: {:?}, tv_nsec: {:?} }}",
                   self.t.tv_sec, self.t.tv_nsec)
        }
    }

    impl Clone for SteadyTime {
        fn clone(&self) -> SteadyTime {
            SteadyTime { t: self.t }
        }
    }

    impl SteadyTime {
        pub fn now() -> SteadyTime {
            let mut t = SteadyTime {
                t: libc::timespec {
                    tv_sec: 0,
                    tv_nsec: 0,
                }
            };
            unsafe {
                assert_eq!(0, imp::clock_gettime(libc::CLOCK_MONOTONIC, &mut t.t));
            }
            t
        }
    }

    impl Sub for SteadyTime {
        type Output = Duration;

        fn sub(self, other: SteadyTime) -> Duration {
            if self.t.tv_nsec >= other.t.tv_nsec {
                Duration::seconds(self.t.tv_sec as i64 - other.t.tv_sec as i64) +
                    Duration::nanoseconds(self.t.tv_nsec as i64 - other.t.tv_nsec as i64)
            } else {
                Duration::seconds(self.t.tv_sec as i64 - 1 - other.t.tv_sec as i64) +
                    Duration::nanoseconds(self.t.tv_nsec as i64 + NSEC_PER_SEC as i64 -
                                          other.t.tv_nsec as i64)
            }
        }
    }

    impl Sub<Duration> for SteadyTime {
        type Output = SteadyTime;

        fn sub(self, other: Duration) -> SteadyTime {
            self + -other
        }
    }

    impl Add<Duration> for SteadyTime {
        type Output = SteadyTime;

        fn add(mut self, other: Duration) -> SteadyTime {
            let seconds = other.num_seconds();
            let nanoseconds = (other - Duration::seconds(seconds)).num_nanoseconds().unwrap();
            self.t.tv_sec += seconds as libc::time_t;
            self.t.tv_nsec += nanoseconds as libc::c_long;
            if self.t.tv_nsec >= NSEC_PER_SEC as libc::c_long {
                self.t.tv_nsec -= NSEC_PER_SEC as libc::c_long;
                self.t.tv_sec += 1;
            } else if self.t.tv_nsec < 0 {
                self.t.tv_sec -= 1;
                self.t.tv_nsec += NSEC_PER_SEC as libc::c_long;
            }
            self
        }
    }

    impl PartialOrd for SteadyTime {
        fn partial_cmp(&self, other: &SteadyTime) -> Option<Ordering> {
            Some(self.cmp(other))
        }
    }

    impl Ord for SteadyTime {
        fn cmp(&self, other: &SteadyTime) -> Ordering {
            match self.t.tv_sec.cmp(&other.t.tv_sec) {
                Ordering::Equal => self.t.tv_nsec.cmp(&other.t.tv_nsec),
                ord => ord
            }
        }
    }

    impl PartialEq for SteadyTime {
        fn eq(&self, other: &SteadyTime) -> bool {
            self.t.tv_sec == other.t.tv_sec && self.t.tv_nsec == other.t.tv_nsec
        }
    }

    impl Eq for SteadyTime {}
}

#[cfg(windows)]
mod steady {
    use {imp, Duration};
    use libc;
    use std::ops::{Sub, Add};

    #[derive(Clone, Copy, PartialOrd, Ord, PartialEq, Eq, Debug)]
    pub struct SteadyTime {
        t: libc::LARGE_INTEGER,
    }

    impl SteadyTime {
        pub fn now() -> SteadyTime {
            let mut t = SteadyTime { t: 0 };
            unsafe { libc::QueryPerformanceCounter(&mut t.t); }
            t
        }
    }

    impl Sub for SteadyTime {
        type Output = Duration;

        fn sub(self, other: SteadyTime) -> Duration {
            let diff = self.t as i64 - other.t as i64;
            Duration::nanoseconds(::mul_div_i64(diff, 1000000000,
                                                imp::frequency() as i64))
        }
    }

    impl Sub<Duration> for SteadyTime {
        type Output = SteadyTime;

        fn sub(self, other: Duration) -> SteadyTime {
            self + -other
        }
    }

    impl Add<Duration> for SteadyTime {
        type Output = SteadyTime;

        fn add(mut self, other: Duration) -> SteadyTime {
            self.t += (other.num_microseconds().unwrap() * imp::frequency() as i64 / 1_000_000)
                as libc::LARGE_INTEGER;
            self
        }
    }
}

pub fn tzset() {
    #[cfg(windows)] extern { #[link_name = "_tzset"] fn tzset(); }
    #[cfg(not(windows))] extern { fn tzset(); }
    unsafe { tzset() }
}

/// Holds a calendar date and time broken down into its components (year, month,
/// day, and so on), also called a broken-down time value.
// FIXME: use c_int instead of i32?
#[repr(C)]
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct Tm {
    /// Seconds after the minute - [0, 60]
    pub tm_sec: i32,

    /// Minutes after the hour - [0, 59]
    pub tm_min: i32,

    /// Hours after midnight - [0, 23]
    pub tm_hour: i32,

    /// Day of the month - [1, 31]
    pub tm_mday: i32,

    /// Months since January - [0, 11]
    pub tm_mon: i32,

    /// Years since 1900
    pub tm_year: i32,

    /// Days since Sunday - [0, 6]. 0 = Sunday, 1 = Monday, ..., 6 = Saturday.
    pub tm_wday: i32,

    /// Days since January 1 - [0, 365]
    pub tm_yday: i32,

    /// Daylight Saving Time flag.
    ///
    /// This value is positive if Daylight Saving Time is in effect, zero if
    /// Daylight Saving Time is not in effect, and negative if this information
    /// is not available.
    pub tm_isdst: i32,

    /// Identifies the time zone that was used to compute this broken-down time
    /// value, including any adjustment for Daylight Saving Time. This is the
    /// number of seconds east of UTC. For example, for U.S. Pacific Daylight
    /// Time, the value is -7*60*60 = -25200.
    pub tm_utcoff: i32,

    /// Nanoseconds after the second - [0, 10<sup>9</sup> - 1]
    pub tm_nsec: i32,
}

impl Add<Duration> for Tm {
    type Output = Tm;

    /// The resulting Tm is in UTC.
    // FIXME:  The resulting Tm should have the same timezone as `self`;
    // however, we need a function such as `at_tm(clock: Timespec, offset: i32)`
    // for this.
    fn add(self, other: Duration) -> Tm {
        at_utc(self.to_timespec() + other)
    }
}

impl Sub<Duration> for Tm {
    type Output = Tm;

    /// The resulting Tm is in UTC.
    // FIXME:  The resulting Tm should have the same timezone as `self`;
    // however, we need a function such as `at_tm(clock: Timespec, offset: i32)`
    // for this.
    fn sub(self, other: Duration) -> Tm {
        at_utc(self.to_timespec() - other)
    }
}

impl Sub<Tm> for Tm {
    type Output = Duration;

    fn sub(self, other: Tm) -> Duration {
        self.to_timespec() - other.to_timespec()
    }
}

impl PartialOrd for Tm {
    fn partial_cmp(&self, other: &Tm) -> Option<Ordering> {
        self.to_timespec().partial_cmp(&other.to_timespec())
    }
}

impl Ord for Tm {
    fn cmp(&self, other: &Tm) -> Ordering {
        self.to_timespec().cmp(&other.to_timespec())
    }
}

pub fn empty_tm() -> Tm {
    Tm {
        tm_sec: 0,
        tm_min: 0,
        tm_hour: 0,
        tm_mday: 0,
        tm_mon: 0,
        tm_year: 0,
        tm_wday: 0,
        tm_yday: 0,
        tm_isdst: 0,
        tm_utcoff: 0,
        tm_nsec: 0,
    }
}

/// Returns the specified time in UTC
pub fn at_utc(clock: Timespec) -> Tm {
    unsafe {
        let Timespec { sec, nsec } = clock;
        let mut tm = empty_tm();
        rustrt::rust_time_gmtime(sec, nsec, &mut tm);
        tm
    }
}

/// Returns the current time in UTC
pub fn now_utc() -> Tm {
    at_utc(get_time())
}

/// Returns the specified time in the local timezone
pub fn at(clock: Timespec) -> Tm {
    unsafe {
        let Timespec { sec, nsec } = clock;
        let mut tm = empty_tm();
        if rustrt::rust_time_localtime(sec, nsec, &mut tm) == 0 {
            panic!("failed to call localtime: {}",
                   io::Error::last_os_error());
        }
        tm
    }
}

/// Returns the current time in the local timezone
pub fn now() -> Tm {
    at(get_time())
}

impl Tm {
    /// Convert time to the seconds from January 1, 1970
    pub fn to_timespec(&self) -> Timespec {
        unsafe {
            let sec = match self.tm_utcoff {
                0 => rustrt::rust_time_timegm(self),
                _     => rustrt::rust_time_mktime(self)
            };

            Timespec::new(sec, self.tm_nsec)
        }
    }

    /// Convert time to the local timezone
    pub fn to_local(&self) -> Tm {
        at(self.to_timespec())
    }

    /// Convert time to the UTC
    pub fn to_utc(&self) -> Tm {
        match self.tm_utcoff {
            0 => *self,
            _ => at_utc(self.to_timespec())
        }
    }

    /**
     * Returns a TmFmt that outputs according to the `asctime` format in ISO
     * C, in the local timezone.
     *
     * Example: "Thu Jan  1 00:00:00 1970"
     */
    pub fn ctime(&self) -> TmFmt {
        TmFmt {
            tm: self,
            format: Fmt::Ctime,
        }
    }

    /**
     * Returns a TmFmt that outputs according to the `asctime` format in ISO
     * C.
     *
     * Example: "Thu Jan  1 00:00:00 1970"
     */
    pub fn asctime(&self) -> TmFmt {
        TmFmt {
            tm: self,
            format: Fmt::Str("%c"),
        }
    }

    /// Formats the time according to the format string.
    pub fn strftime<'a>(&'a self, format: &'a str) -> Result<TmFmt<'a>, ParseError> {
        validate_format(TmFmt {
            tm: self,
            format: Fmt::Str(format),
        })
    }

    /**
     * Returns a TmFmt that outputs according to RFC 822.
     *
     * local: "Thu, 22 Mar 2012 07:53:18 PST"
     * utc:   "Thu, 22 Mar 2012 14:53:18 GMT"
     */
    pub fn rfc822(&self) -> TmFmt {
        let fmt = if self.tm_utcoff == 0 {
            "%a, %d %b %Y %T GMT"
        } else {
            "%a, %d %b %Y %T %Z"
        };
        TmFmt {
            tm: self,
            format: Fmt::Str(fmt),
        }
    }

    /**
     * Returns a TmFmt that outputs according to RFC 822 with Zulu time.
     *
     * local: "Thu, 22 Mar 2012 07:53:18 -0700"
     * utc:   "Thu, 22 Mar 2012 14:53:18 -0000"
     */
    pub fn rfc822z(&self) -> TmFmt {
        TmFmt {
            tm: self,
            format: Fmt::Str("%a, %d %b %Y %T %z"),
        }
    }

    /**
     * Returns a TmFmt that outputs according to RFC 3339. RFC 3339 is
     * compatible with ISO 8601.
     *
     * local: "2012-02-22T07:53:18-07:00"
     * utc:   "2012-02-22T14:53:18Z"
     */
    pub fn rfc3339<'a>(&'a self) -> TmFmt {
        TmFmt {
            tm: self,
            format: Fmt::Rfc3339,
        }
    }
}

#[derive(Copy, PartialEq, Debug, Clone)]
pub enum ParseError {
    InvalidSecond,
    InvalidMinute,
    InvalidHour,
    InvalidDay,
    InvalidMonth,
    InvalidYear,
    InvalidDayOfWeek,
    InvalidDayOfMonth,
    InvalidDayOfYear,
    InvalidZoneOffset,
    InvalidTime,
    InvalidSecondsSinceEpoch,
    MissingFormatConverter,
    InvalidFormatSpecifier(char),
    UnexpectedCharacter(char, char),
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            InvalidSecond => write!(f, "Invalid second."),
            InvalidMinute => write!(f, "Invalid minute."),
            InvalidHour => write!(f, "Invalid hour."),
            InvalidDay => write!(f, "Invalid day."),
            InvalidMonth => write!(f, "Invalid month."),
            InvalidYear => write!(f, "Invalid year."),
            InvalidDayOfWeek => write!(f, "Invalid day of the week."),
            InvalidDayOfMonth => write!(f, "Invalid day of the month."),
            InvalidDayOfYear => write!(f, "Invalid day of the year."),
            InvalidZoneOffset => write!(f, "Invalid zone offset."),
            InvalidTime => write!(f, "Invalid time."),
            InvalidSecondsSinceEpoch => write!(f, "Invalid seconds since epoch."),
            MissingFormatConverter => {
                write!(f, "missing format converter after `%`")
            }
            InvalidFormatSpecifier(ch) => {
                write!(f, "invalid format specifier: %{}", ch)
            }
            UnexpectedCharacter(a, b) => {
                write!(f, "expected: `{}`, found: `{}`", a, b)
            }
        }
    }
}

/// A wrapper around a `Tm` and format string that implements Display.
#[derive(Debug)]
pub struct TmFmt<'a> {
    tm: &'a Tm,
    format: Fmt<'a>
}

#[derive(Debug)]
enum Fmt<'a> {
    Str(&'a str),
    Rfc3339,
    Ctime,
}

fn validate_format<'a>(fmt: TmFmt<'a>) -> Result<TmFmt<'a>, ParseError> {

    match (fmt.tm.tm_wday, fmt.tm.tm_mon) {
        (0...6, 0...11) => (),
        (_wday, 0...11) => return Err(InvalidDayOfWeek),
        (0...6, _mon) => return Err(InvalidMonth),
        _ => return Err(InvalidDay)
    }
    match fmt.format {
        Fmt::Str(ref s) => {
            let mut chars = s.chars();
            loop {
                match chars.next() {
                    Some('%') => {
                        match chars.next() {
                            Some('A') | Some('a') | Some('B') | Some('b') |
                            Some('C') | Some('c') | Some('D') | Some('d') |
                            Some('e') | Some('F') | Some('f') | Some('G') |
                            Some('g') | Some('H') | Some('h') | Some('I') |
                            Some('j') | Some('k') | Some('l') | Some('M') |
                            Some('m') | Some('n') | Some('P') | Some('p') |
                            Some('R') | Some('r') | Some('S') | Some('s') |
                            Some('T') | Some('t') | Some('U') | Some('u') |
                            Some('V') | Some('v') | Some('W') | Some('w') |
                            Some('X') | Some('x') | Some('Y') | Some('y') |
                            Some('Z') | Some('z') | Some('+') | Some('%') => (),

                            Some(c) => return Err(InvalidFormatSpecifier(c)),
                            None => return Err(MissingFormatConverter),
                        }
                    },
                    None => break,
                    _ => ()
                }
            }
        },
        _ => ()
    }
    Ok(fmt)
}

/// Formats the time according to the format string.
pub fn strftime(format: &str, tm: &Tm) -> Result<String, ParseError> {
    tm.strftime(format).map(|fmt| fmt.to_string())
}

// Computes (value*numer)/denom without overflow, as long as both
// (numer*denom) and the overall result fit into i64 (which is the case
// for our time conversions).
#[allow(dead_code)]
fn mul_div_i64(value: i64, numer: i64, denom: i64) -> i64 {
    let q = value / denom;
    let r = value % denom;
    // Decompose value as (value/denom*denom + value%denom),
    // substitute into (value*numer)/denom and simplify.
    // r < denom, so (denom*numer) is the upper bound of (r*numer)
    q * numer + r * numer / denom
}

#[cfg(test)]
mod tests {
    extern crate test;
    use super::{Timespec, get_time, precise_time_ns, precise_time_s, tzset,
                at_utc, at, strptime, PreciseTime, ParseError, Duration};
    use super::mul_div_i64;
    use super::ParseError::{InvalidTime, InvalidYear, MissingFormatConverter,
                            InvalidFormatSpecifier};

    use self::test::Bencher;

    #[test]
    fn test_muldiv() {
        assert_eq!(mul_div_i64( 1_000_000_000_001, 1_000_000_000, 1_000_000),
                   1_000_000_000_001_000);
        assert_eq!(mul_div_i64(-1_000_000_000_001, 1_000_000_000, 1_000_000),
                   -1_000_000_000_001_000);
        assert_eq!(mul_div_i64(-1_000_000_000_001,-1_000_000_000, 1_000_000),
                   1_000_000_000_001_000);
        assert_eq!(mul_div_i64( 1_000_000_000_001, 1_000_000_000,-1_000_000),
                   -1_000_000_000_001_000);
        assert_eq!(mul_div_i64( 1_000_000_000_001,-1_000_000_000,-1_000_000),
                   1_000_000_000_001_000);
    }

    #[cfg(windows)]
    fn set_time_zone() {
        use libc;
        use std::ffi::CString;
        // Windows crt doesn't see any environment variable set by
        // `SetEnvironmentVariable`, which `os::setenv` internally uses.
        // It is why we use `putenv` here.
        extern {
            fn _putenv(envstring: *const libc::c_char) -> libc::c_int;
        }

        unsafe {
            // Windows does not understand "America/Los_Angeles".
            // PST+08 may look wrong, but not! "PST" indicates
            // the name of timezone. "+08" means UTC = local + 08.
            let c = CString::new("TZ=PST+08").unwrap();
            _putenv(c.as_ptr());
        }
        tzset();
    }
    #[cfg(not(windows))]
    fn set_time_zone() {
        use std::env;
        env::set_var("TZ", "America/Los_Angeles");
        tzset();
    }

    fn test_get_time() {
        static SOME_RECENT_DATE: i64 = 1325376000i64; // 2012-01-01T00:00:00Z
        static SOME_FUTURE_DATE: i64 = 1577836800i64; // 2020-01-01T00:00:00Z

        let tv1 = get_time();
        debug!("tv1={} sec + {} nsec", tv1.sec, tv1.nsec);

        assert!(tv1.sec > SOME_RECENT_DATE);
        assert!(tv1.nsec < 1000000000i32);

        let tv2 = get_time();
        debug!("tv2={} sec + {} nsec", tv2.sec, tv2.nsec);

        assert!(tv2.sec >= tv1.sec);
        assert!(tv2.sec < SOME_FUTURE_DATE);
        assert!(tv2.nsec < 1000000000i32);
        if tv2.sec == tv1.sec {
            assert!(tv2.nsec >= tv1.nsec);
        }
    }

    fn test_precise_time() {
        let s0 = precise_time_s();
        debug!("s0={} sec", s0);
        assert!(s0 > 0.);

        let ns0 = precise_time_ns();
        let ns1 = precise_time_ns();
        debug!("ns0={} ns", ns0);
        debug!("ns1={} ns", ns1);
        assert!(ns1 >= ns0);

        let ns2 = precise_time_ns();
        debug!("ns2={} ns", ns2);
        assert!(ns2 >= ns1);
    }

    fn test_precise_time_to() {
        let t0 = PreciseTime(1000);
        let t1 = PreciseTime(1023);
        assert_eq!(Duration::nanoseconds(23), t0.to(t1));
    }

    fn test_at_utc() {
        set_time_zone();

        let time = Timespec::new(1234567890, 54321);
        let utc = at_utc(time);

        assert_eq!(utc.tm_sec, 30);
        assert_eq!(utc.tm_min, 31);
        assert_eq!(utc.tm_hour, 23);
        assert_eq!(utc.tm_mday, 13);
        assert_eq!(utc.tm_mon, 1);
        assert_eq!(utc.tm_year, 109);
        assert_eq!(utc.tm_wday, 5);
        assert_eq!(utc.tm_yday, 43);
        assert_eq!(utc.tm_isdst, 0);
        assert_eq!(utc.tm_utcoff, 0);
        assert_eq!(utc.tm_nsec, 54321);
    }

    fn test_at() {
        set_time_zone();

        let time = Timespec::new(1234567890, 54321);
        let local = at(time);

        debug!("time_at: {:?}", local);

        assert_eq!(local.tm_sec, 30);
        assert_eq!(local.tm_min, 31);
        assert_eq!(local.tm_hour, 15);
        assert_eq!(local.tm_mday, 13);
        assert_eq!(local.tm_mon, 1);
        assert_eq!(local.tm_year, 109);
        assert_eq!(local.tm_wday, 5);
        assert_eq!(local.tm_yday, 43);
        assert_eq!(local.tm_isdst, 0);
        assert_eq!(local.tm_utcoff, -28800);
        assert_eq!(local.tm_nsec, 54321);
    }

    fn test_to_timespec() {
        set_time_zone();

        let time = Timespec::new(1234567890, 54321);
        let utc = at_utc(time);

        assert_eq!(utc.to_timespec(), time);
        assert_eq!(utc.to_local().to_timespec(), time);
    }

    fn test_conversions() {
        set_time_zone();

        let time = Timespec::new(1234567890, 54321);
        let utc = at_utc(time);
        let local = at(time);

        assert!(local.to_local() == local);
        assert!(local.to_utc() == utc);
        assert!(local.to_utc().to_local() == local);
        assert!(utc.to_utc() == utc);
        assert!(utc.to_local() == local);
        assert!(utc.to_local().to_utc() == utc);
    }

    fn test_strptime() {
        set_time_zone();

        match strptime("", "") {
            Ok(ref tm) => {
                assert!(tm.tm_sec == 0);
                assert!(tm.tm_min == 0);
                assert!(tm.tm_hour == 0);
                assert!(tm.tm_mday == 0);
                assert!(tm.tm_mon == 0);
                assert!(tm.tm_year == 0);
                assert!(tm.tm_wday == 0);
                assert!(tm.tm_isdst == 0);
                assert!(tm.tm_utcoff == 0);
                assert!(tm.tm_nsec == 0);
            }
            Err(_) => ()
        }

        let format = "%a %b %e %T.%f %Y";
        assert_eq!(strptime("", format), Err(ParseError::InvalidDay));
        assert_eq!(strptime("Fri Feb 13 15:31:30", format),
                   Err(InvalidTime));

        match strptime("Fri Feb 13 15:31:30.01234 2009", format) {
            Err(e) => panic!("{}", e),
            Ok(ref tm) => {
                assert_eq!(tm.tm_sec, 30);
                assert_eq!(tm.tm_min, 31);
                assert_eq!(tm.tm_hour, 15);
                assert_eq!(tm.tm_mday, 13);
                assert_eq!(tm.tm_mon, 1);
                assert_eq!(tm.tm_year, 109);
                assert_eq!(tm.tm_wday, 5);
                assert_eq!(tm.tm_yday, 0);
                assert_eq!(tm.tm_isdst, 0);
                assert_eq!(tm.tm_utcoff, 0);
                assert_eq!(tm.tm_nsec, 12340000);
            }
        }

        fn test(s: &str, format: &str) -> bool {
            match strptime(s, format) {
              Ok(tm) => {
                tm.strftime(format).unwrap().to_string() == s.to_string()
              },
              Err(e) => panic!("{:?},  s={:?}, format={:?}", e, s, format)
            }
        }

        fn test_oneway(s : &str, format : &str) -> bool {
            match strptime(s, format) {
              Ok(_) => {
                // oneway tests are used when reformatting the parsed Tm
                // back into a string can generate a different string
                // from the original (i.e. leading zeroes)
                true
              },
              Err(e) => panic!("{:?},  s={:?}, format={:?}", e, s, format)
            }
        }

        let days = [
            "Sunday".to_string(),
            "Monday".to_string(),
            "Tuesday".to_string(),
            "Wednesday".to_string(),
            "Thursday".to_string(),
            "Friday".to_string(),
            "Saturday".to_string()
        ];
        for day in days.iter() {
            assert!(test(&day, "%A"));
        }

        let days = [
            "Sun".to_string(),
            "Mon".to_string(),
            "Tue".to_string(),
            "Wed".to_string(),
            "Thu".to_string(),
            "Fri".to_string(),
            "Sat".to_string()
        ];
        for day in days.iter() {
            assert!(test(&day, "%a"));
        }

        let months = [
            "January".to_string(),
            "February".to_string(),
            "March".to_string(),
            "April".to_string(),
            "May".to_string(),
            "June".to_string(),
            "July".to_string(),
            "August".to_string(),
            "September".to_string(),
            "October".to_string(),
            "November".to_string(),
            "December".to_string()
        ];
        for day in months.iter() {
            assert!(test(&day, "%B"));
        }

        let months = [
            "Jan".to_string(),
            "Feb".to_string(),
            "Mar".to_string(),
            "Apr".to_string(),
            "May".to_string(),
            "Jun".to_string(),
            "Jul".to_string(),
            "Aug".to_string(),
            "Sep".to_string(),
            "Oct".to_string(),
            "Nov".to_string(),
            "Dec".to_string()
        ];
        for day in months.iter() {
            assert!(test(&day, "%b"));
        }

        assert!(test("19", "%C"));
        assert!(test("Fri Feb  3 23:31:30 2009", "%c"));
        assert!(test("Fri Feb 13 23:31:30 2009", "%c"));
        assert!(test("02/13/09", "%D"));
        assert!(test("03", "%d"));
        assert!(test("13", "%d"));
        assert!(test(" 3", "%e"));
        assert!(test("13", "%e"));
        assert!(test("2009-02-13", "%F"));
        assert!(test("03", "%H"));
        assert!(test("13", "%H"));
        assert!(test("03", "%I")); // FIXME (#2350): flesh out
        assert!(test("11", "%I")); // FIXME (#2350): flesh out
        assert!(test("044", "%j"));
        assert!(test(" 3", "%k"));
        assert!(test("13", "%k"));
        assert!(test(" 1", "%l"));
        assert!(test("11", "%l"));
        assert!(test("03", "%M"));
        assert!(test("13", "%M"));
        assert!(test("\n", "%n"));
        assert!(test("am", "%P"));
        assert!(test("pm", "%P"));
        assert!(test("AM", "%p"));
        assert!(test("PM", "%p"));
        assert!(test("23:31", "%R"));
        assert!(test("11:31:30 AM", "%r"));
        assert!(test("11:31:30 PM", "%r"));
        assert!(test("03", "%S"));
        assert!(test("13", "%S"));
        assert!(test("15:31:30", "%T"));
        assert!(test("\t", "%t"));
        assert!(test("1", "%u"));
        assert!(test("7", "%u"));
        assert!(test("13-Feb-2009", "%v"));
        assert!(test("0", "%w"));
        assert!(test("6", "%w"));
        assert!(test("2009", "%Y"));
        assert!(test("09", "%y"));

        assert!(test_oneway("3",  "%d"));
        assert!(test_oneway("3",  "%H"));
        assert!(test_oneway("3",  "%e"));
        assert!(test_oneway("3",  "%M"));
        assert!(test_oneway("3",  "%S"));

        assert!(strptime("-0000", "%z").unwrap().tm_utcoff == 0);
        assert!(strptime("-00:00", "%z").unwrap().tm_utcoff == 0);
        assert_eq!(-28800, strptime("-0800", "%z").unwrap().tm_utcoff);
        assert_eq!(-28800, strptime("-08:00", "%z").unwrap().tm_utcoff);
        assert_eq!(28800, strptime("+0800", "%z").unwrap().tm_utcoff);
        assert_eq!(28800, strptime("+08:00", "%z").unwrap().tm_utcoff);
        assert_eq!(5400, strptime("+0130", "%z").unwrap().tm_utcoff);
        assert_eq!(5400, strptime("+01:30", "%z").unwrap().tm_utcoff);
        assert!(test("%", "%%"));

        // Test for #7256
        assert_eq!(strptime("360", "%Y-%m-%d"), Err(InvalidYear));

        // Test for epoch seconds parsing
        {
            assert!(test("1428035610", "%s"));
            let tm = strptime("1428035610", "%s").unwrap();
            assert_eq!(tm.tm_utcoff, 0);
            assert_eq!(tm.tm_isdst, 0);
            assert_eq!(tm.tm_yday, 92);
            assert_eq!(tm.tm_wday, 5);
            assert_eq!(tm.tm_year, 115);
            assert_eq!(tm.tm_mon, 3);
            assert_eq!(tm.tm_mday, 3);
            assert_eq!(tm.tm_hour, 4);
        }
    }

    fn test_asctime() {
        set_time_zone();

        let time = Timespec::new(1234567890, 54321);
        let utc   = at_utc(time);
        let local = at(time);

        debug!("test_ctime: {} {}", utc.asctime(), local.asctime());

        assert_eq!(utc.asctime().to_string(), "Fri Feb 13 23:31:30 2009".to_string());
        assert_eq!(local.asctime().to_string(), "Fri Feb 13 15:31:30 2009".to_string());
    }

    fn test_ctime() {
        set_time_zone();

        let time = Timespec::new(1234567890, 54321);
        let utc   = at_utc(time);
        let local = at(time);

        debug!("test_ctime: {} {}", utc.ctime(), local.ctime());

        assert_eq!(utc.ctime().to_string(), "Fri Feb 13 15:31:30 2009".to_string());
        assert_eq!(local.ctime().to_string(), "Fri Feb 13 15:31:30 2009".to_string());
    }

    fn test_strftime() {
        set_time_zone();

        let time = Timespec::new(1234567890, 54321);
        let utc = at_utc(time);
        let local = at(time);

        assert_eq!(local.strftime("").unwrap().to_string(), "".to_string());
        assert_eq!(local.strftime("%A").unwrap().to_string(), "Friday".to_string());
        assert_eq!(local.strftime("%a").unwrap().to_string(), "Fri".to_string());
        assert_eq!(local.strftime("%B").unwrap().to_string(), "February".to_string());
        assert_eq!(local.strftime("%b").unwrap().to_string(), "Feb".to_string());
        assert_eq!(local.strftime("%C").unwrap().to_string(), "20".to_string());
        assert_eq!(local.strftime("%c").unwrap().to_string(),
                   "Fri Feb 13 15:31:30 2009".to_string());
        assert_eq!(local.strftime("%D").unwrap().to_string(), "02/13/09".to_string());
        assert_eq!(local.strftime("%d").unwrap().to_string(), "13".to_string());
        assert_eq!(local.strftime("%e").unwrap().to_string(), "13".to_string());
        assert_eq!(local.strftime("%F").unwrap().to_string(), "2009-02-13".to_string());
        assert_eq!(local.strftime("%f").unwrap().to_string(), "000054321".to_string());
        assert_eq!(local.strftime("%G").unwrap().to_string(), "2009".to_string());
        assert_eq!(local.strftime("%g").unwrap().to_string(), "09".to_string());
        assert_eq!(local.strftime("%H").unwrap().to_string(), "15".to_string());
        assert_eq!(local.strftime("%h").unwrap().to_string(), "Feb".to_string());
        assert_eq!(local.strftime("%I").unwrap().to_string(), "03".to_string());
        assert_eq!(local.strftime("%j").unwrap().to_string(), "044".to_string());
        assert_eq!(local.strftime("%k").unwrap().to_string(), "15".to_string());
        assert_eq!(local.strftime("%l").unwrap().to_string(), " 3".to_string());
        assert_eq!(local.strftime("%M").unwrap().to_string(), "31".to_string());
        assert_eq!(local.strftime("%m").unwrap().to_string(), "02".to_string());
        assert_eq!(local.strftime("%n").unwrap().to_string(), "\n".to_string());
        assert_eq!(local.strftime("%P").unwrap().to_string(), "pm".to_string());
        assert_eq!(local.strftime("%p").unwrap().to_string(), "PM".to_string());
        assert_eq!(local.strftime("%R").unwrap().to_string(), "15:31".to_string());
        assert_eq!(local.strftime("%r").unwrap().to_string(), "03:31:30 PM".to_string());
        assert_eq!(local.strftime("%S").unwrap().to_string(), "30".to_string());
        assert_eq!(local.strftime("%s").unwrap().to_string(), "1234567890".to_string());
        assert_eq!(local.strftime("%T").unwrap().to_string(), "15:31:30".to_string());
        assert_eq!(local.strftime("%t").unwrap().to_string(), "\t".to_string());
        assert_eq!(local.strftime("%U").unwrap().to_string(), "06".to_string());
        assert_eq!(local.strftime("%u").unwrap().to_string(), "5".to_string());
        assert_eq!(local.strftime("%V").unwrap().to_string(), "07".to_string());
        assert_eq!(local.strftime("%v").unwrap().to_string(), "13-Feb-2009".to_string());
        assert_eq!(local.strftime("%W").unwrap().to_string(), "06".to_string());
        assert_eq!(local.strftime("%w").unwrap().to_string(), "5".to_string());
        // FIXME (#2350): support locale
        assert_eq!(local.strftime("%X").unwrap().to_string(), "15:31:30".to_string());
        // FIXME (#2350): support locale
        assert_eq!(local.strftime("%x").unwrap().to_string(), "02/13/09".to_string());
        assert_eq!(local.strftime("%Y").unwrap().to_string(), "2009".to_string());
        assert_eq!(local.strftime("%y").unwrap().to_string(), "09".to_string());
        // FIXME (#2350): support locale
        assert_eq!(local.strftime("%Z").unwrap().to_string(), "".to_string());
        assert_eq!(local.strftime("%z").unwrap().to_string(), "-0800".to_string());
        assert_eq!(local.strftime("%+").unwrap().to_string(),
                   "2009-02-13T15:31:30-08:00".to_string());
        assert_eq!(local.strftime("%%").unwrap().to_string(), "%".to_string());

         let invalid_specifiers = ["%E", "%J", "%K", "%L", "%N", "%O", "%o", "%Q", "%q"];
        for &sp in invalid_specifiers.iter() {
            assert_eq!(local.strftime(sp).unwrap_err(), InvalidFormatSpecifier(sp.char_at(1)));
        }
        assert_eq!(local.strftime("%").unwrap_err(), MissingFormatConverter);
        assert_eq!(local.strftime("%A %").unwrap_err(), MissingFormatConverter);

        assert_eq!(local.asctime().to_string(), "Fri Feb 13 15:31:30 2009".to_string());
        assert_eq!(local.ctime().to_string(), "Fri Feb 13 15:31:30 2009".to_string());
        assert_eq!(local.rfc822z().to_string(), "Fri, 13 Feb 2009 15:31:30 -0800".to_string());
        assert_eq!(local.rfc3339().to_string(), "2009-02-13T15:31:30-08:00".to_string());

        assert_eq!(utc.asctime().to_string(), "Fri Feb 13 23:31:30 2009".to_string());
        assert_eq!(utc.ctime().to_string(), "Fri Feb 13 15:31:30 2009".to_string());
        assert_eq!(utc.rfc822().to_string(), "Fri, 13 Feb 2009 23:31:30 GMT".to_string());
        assert_eq!(utc.rfc822z().to_string(), "Fri, 13 Feb 2009 23:31:30 -0000".to_string());
        assert_eq!(utc.rfc3339().to_string(), "2009-02-13T23:31:30Z".to_string());
    }

    fn test_timespec_eq_ord() {
        let a = &Timespec::new(-2, 1);
        let b = &Timespec::new(-1, 2);
        let c = &Timespec::new(1, 2);
        let d = &Timespec::new(2, 1);
        let e = &Timespec::new(2, 1);

        assert!(d.eq(e));
        assert!(c.ne(e));

        assert!(a.lt(b));
        assert!(b.lt(c));
        assert!(c.lt(d));

        assert!(a.le(b));
        assert!(b.le(c));
        assert!(c.le(d));
        assert!(d.le(e));
        assert!(e.le(d));

        assert!(b.ge(a));
        assert!(c.ge(b));
        assert!(d.ge(c));
        assert!(e.ge(d));
        assert!(d.ge(e));

        assert!(b.gt(a));
        assert!(c.gt(b));
        assert!(d.gt(c));
    }

    fn test_timespec_add() {
        let a = Timespec::new(1, 2);
        let b = Duration::seconds(2) + Duration::nanoseconds(3);
        let c = a + b;
        assert_eq!(c.sec, 3);
        assert_eq!(c.nsec, 5);

        let p = Timespec::new(1, super::NSEC_PER_SEC - 2);
        let q = Duration::seconds(2) + Duration::nanoseconds(2);
        let r = p + q;
        assert_eq!(r.sec, 4);
        assert_eq!(r.nsec, 0);

        let u = Timespec::new(1, super::NSEC_PER_SEC - 2);
        let v = Duration::seconds(2) + Duration::nanoseconds(3);
        let w = u + v;
        assert_eq!(w.sec, 4);
        assert_eq!(w.nsec, 1);

        let k = Timespec::new(1, 0);
        let l = Duration::nanoseconds(-1);
        let m = k + l;
        assert_eq!(m.sec, 0);
        assert_eq!(m.nsec, 999_999_999);
    }

    fn test_timespec_sub() {
        let a = Timespec::new(2, 3);
        let b = Timespec::new(1, 2);
        let c = a - b;
        assert_eq!(c.num_nanoseconds(), Some(super::NSEC_PER_SEC as i64 + 1));

        let p = Timespec::new(2, 0);
        let q = Timespec::new(1, 2);
        let r = p - q;
        assert_eq!(r.num_nanoseconds(), Some(super::NSEC_PER_SEC as i64 - 2));

        let u = Timespec::new(1, 2);
        let v = Timespec::new(2, 3);
        let w = u - v;
        assert_eq!(w.num_nanoseconds(), Some(-super::NSEC_PER_SEC as i64 - 1));
    }

    fn test_time_sub() {
        let a = ::now();
        let b = at(a.to_timespec() + Duration::seconds(5));
        let c = b - a;
        assert_eq!(c.num_nanoseconds(), Some(super::NSEC_PER_SEC as i64 * 5));
    }

    #[test]
    #[cfg_attr(target_os = "android", ignore)] // FIXME #10958
    fn run_tests() {
        // The tests race on tzset. So instead of having many independent
        // tests, we will just call the functions now.
        test_get_time();
        test_precise_time();
        test_precise_time_to();
        test_at_utc();
        test_at();
        test_to_timespec();
        test_conversions();
        test_strptime();
        test_asctime();
        test_ctime();
        test_strftime();
        test_timespec_eq_ord();
        test_timespec_add();
        test_timespec_sub();
        test_time_sub();
    }

    #[bench]
    fn bench_precise_time_ns(b: &mut Bencher) {
        b.iter(|| precise_time_ns())
    }
}
