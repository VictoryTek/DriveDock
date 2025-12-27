pub mod smb;
pub mod nfs;

// Re-export main types and functions
pub use smb::{SmbShare, scan_smb_shares, mount_smb_share};
pub use nfs::{NfsExport, scan_nfs_shares, mount_nfs_export};
