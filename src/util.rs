use std::time::SystemTime;

/// source format: 'Jan 02, 2006 03:04 PM'
pub fn simple_parse(time_str: &str) -> Option<SystemTime> {
    let mut time = time_str.as_bytes();
    if time.len() < 19 {
        return None; // early return to eliminate bound checks
    }

    let mut buf = *b"Sun, 0D MMM YYYY_hh_mm:00 GMT";
    buf[8..11].copy_from_slice(&time[..3]);
    if time[5] == b',' {
        buf[6] = time[4];
        time = &time[7..];
    } else {
        buf[5] = time[4];
        buf[6] = time[5];
        time = &time[8..];
    }
    buf[12..22].copy_from_slice(&time[..10]);
    if time[11] == b'P' {
        buf[17] += 1;
        buf[18] += 2;
        if buf[18] >= 58 {
            buf[17] += 1;
            buf[18] -= 10;
        }
    }

    let buf = unsafe { core::str::from_utf8_unchecked(&buf) };
    let date = httpdate::parse_http_date(buf);
    tracing::info!(target: "time-converter", "{time_str:?} -> {buf:?} -> {date:?}");

    date.ok()
}
