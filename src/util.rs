use std::ascii::Char;
use std::time::SystemTime;

const TEMPLATE_DATE: [Char; 29] = *b"Sun, 0D MMM YYYY_hh_mm:00 GMT".as_ascii().unwrap();

/// source format: 'Jan 02, 2006 03:04 PM'
pub fn simple_parse(mut time: &[Char]) -> Option<SystemTime> {
    if time.len() < 19 {
        return None; // early return to eliminate bound checks
    }

    let mut buf = TEMPLATE_DATE;
    buf[8..11].copy_from_slice(&time[..3]);
    if time[5] == Char::Comma {
        buf[6] = time[4];
        time = &time[7..];
    } else {
        buf[5] = time[4];
        buf[6] = time[5];
        time = &time[8..];
    }
    buf[12..22].copy_from_slice(&time[..10]);
    if time[11] == Char::CapitalP {
        buf[17] = Char::from_u8((buf[17] as u8) + 1)?;
        buf[18] = Char::from_u8((buf[18] as u8) + 2)?;
        if buf[18] > Char::Digit9 {
            buf[17] = Char::from_u8((buf[17] as u8) + 1)?;
            buf[18] = Char::from_u8((buf[18] as u8) - 10)?;
        }
    }

    let date = httpdate::parse_http_date(buf.as_str());
    tracing::info!(target: "time-converter", "{time:?} -> {buf:?} -> {date:?}");

    date.ok()
}
