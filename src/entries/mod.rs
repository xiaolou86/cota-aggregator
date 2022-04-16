use lazy_static::lazy_static;
use parking_lot::Mutex;

pub(crate) mod claim;
pub(crate) mod claim_update;
mod constants;
pub(crate) mod define;
pub mod helper;
pub(crate) mod mint;
pub mod smt;
pub(crate) mod transfer;
pub(crate) mod transfer_update;
pub(crate) mod update;
pub(crate) mod withdrawal;

lazy_static! {
    static ref SMT_LOCK: Mutex<()> = Mutex::new(());
}
