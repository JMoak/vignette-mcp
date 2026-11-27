//! `kw-input-helper`: turns a remote keeper's pointer and keys, sent by the console under a live
//! grant, into input on this machine, and tells the console when someone here uses it.
//! `README.md` has the protocol and what the helper may do.

// `--cfg check_foreign` compiles every backend on any host to type-check them; there they go unused.
#![cfg_attr(check_foreign, allow(dead_code))]

pub mod backend;
pub mod json;
pub mod keys;
pub mod limits;
pub mod lines;
pub mod protocol;
pub mod screen;
pub mod session;

#[cfg(any(target_os = "macos", check_foreign))]
pub mod macos;
#[cfg(any(unix, check_foreign))]
pub mod posix;
#[cfg(windows)]
pub mod win32;
#[cfg(any(target_os = "linux", check_foreign))]
pub mod x11;
