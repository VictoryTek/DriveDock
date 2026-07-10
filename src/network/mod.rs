pub mod discover;
pub mod mount;

// Re-export main types and functions
pub use discover::{scan_network_shares, NetworkShare};
// `Credentials` is part of the public API (the return type of `collect_credentials`)
// even though callers currently only bind it via `let`/type inference, not by name.
#[allow(unused_imports)]
pub use mount::{collect_credentials, mount_share, set_persistent, unmount_share, Credentials};
