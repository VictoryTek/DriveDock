pub mod smb;
pub mod nfs;

// Re-export main types
pub use smb::{SmbShare, scan_smb_shares};
pub use nfs::{NfsExport, scan_nfs_shares};
