use std::{sync::atomic::AtomicI64, time::SystemTime};

use parking_lot::RwLock;

pub mod dripfeedpanel;
pub mod ezkify;
pub mod smmrapid;

pub static GLOBAL_DATE: RwLock<SystemTime> = RwLock::new(SystemTime::UNIX_EPOCH);
pub static CID: AtomicI64 = AtomicI64::new(0);
