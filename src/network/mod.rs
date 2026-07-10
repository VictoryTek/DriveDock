pub mod gvfs;

// Re-export main types and functions
pub use gvfs::{mount_share, scan_network_shares, unmount_share, NetworkShare};
