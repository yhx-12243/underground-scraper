use std::{
    ascii::Char, error::Error, ffi::OsString, io, mem::ManuallyDrop, path::PathBuf, sync::Arc,
    time::SystemTime,
};

const TEMPLATE_DATE: [Char; 29] = *b"Sun, 0D MMM YYYY_hh_mm:00 GMT".as_ascii().unwrap();

/// source format: 'Jan 02, 2006 03:04 PM'
pub fn simple_parse(mut time: &[Char]) -> Option<SystemTime> {
    if time.len() < 20 {
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

pub fn xmax_to_success<'a, I>(rows: I) -> usize
where
    I: Iterator<Item = &'a tokio_postgres::Row>,
{
    rows.filter(|row| !row.try_get(0).is_ok_and(|p: u32| p != 0)).count()
}

#[inline]
#[must_use]
pub fn box_io_error(e: io::Error) -> Box<dyn Error + Send + Sync> {
    if e.get_ref().is_some() {
        unsafe { e.into_inner().unwrap_unchecked() }
    } else {
        Box::new(e)
    }
}

pub trait SetLenExt {
    /// # Safety
    ///
    /// Caller should satisfy the safety requirements of `Vec::set_len`.
    unsafe fn set_len(&mut self, len: usize);
    fn append_i32(&mut self, value: i32);
}

impl SetLenExt for OsString {
    #[inline]
    unsafe fn set_len(&mut self, len: usize) {
        unsafe {
            let mut vec = core::ptr::read(self).into_encoded_bytes();
            vec.set_len(len);
            core::ptr::write(self, Self::from_encoded_bytes_unchecked(vec));
        }
    }

    #[inline]
    fn append_i32(&mut self, value: i32) {
        unsafe {
            use std::fmt::Display;
            let inner = core::ptr::NonNull::from(self).cast::<String>().as_mut();
            let mut fmt = core::fmt::Formatter::new(inner, core::fmt::FormattingOptions::new());
            let _ = value.fmt(&mut fmt);
        }
    }
}

impl SetLenExt for PathBuf {
    #[inline]
    unsafe fn set_len(&mut self, len: usize) {
        unsafe {
            self.as_mut_os_string().set_len(len);
        }
    }

    #[inline]
    fn append_i32(&mut self, value: i32) {
        self.push("");
        self.as_mut_os_string().append_i32(value);
    }
}

pub fn clone_arc<T>(this: &T) -> Arc<T> {
    let arc = ManuallyDrop::new(unsafe { Arc::from_raw(this) });
    Arc::clone(&arc)
}
