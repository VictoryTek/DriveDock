use anyhow::{Context, Result};
use std::net::IpAddr;
use std::path::PathBuf;

/// Represents a discovered NFS export
#[derive(Debug, Clone)]
pub struct NfsExport {
    /// Server hostname or IP address
    pub server: String,
    
    /// Server IP address (resolved)
    pub ip_address: Option<IpAddr>,
    
    /// Export path on the server (e.g., "/export/data", "/home")
    pub export_path: PathBuf,
    
    /// Full NFS URI (e.g., "nfs://server:/export/data")
    pub nfs_uri: String,
    
    /// NFS version (3 or 4)
    pub nfs_version: Option<u8>,
    
    /// Export options/permissions (ro, rw, etc.)
    pub options: Vec<String>,
}

impl NfsExport {
    /// Create mount string for NFS
    /// Returns something like "192.168.1.100:/export/data"
    pub fn mount_string(&self) -> String {
        format!("{}:{}", self.server, self.export_path.display())
    }

    /// Get display name for UI
    pub fn display_name(&self) -> String {
        format!("{} on {}", self.export_path.display(), self.server)
    }

    /// Check if export is read-only
    pub fn is_read_only(&self) -> bool {
        self.options.iter().any(|opt| opt == "ro")
    }
}

/// Scan the local network for available NFS exports
/// 
/// This function discovers NFS exports by:
/// 1. Using Avahi/mDNS to find NFS servers (_nfs._tcp)
/// 2. Using showmount -e to query exports from discovered servers
/// 3. Parsing and aggregating results
/// 
/// # Implementation Strategy
/// 
/// Multiple approaches for NFS discovery:
/// 
/// ## Approach 1: showmount (traditional)
/// ```bash
/// # Find NFS servers via Avahi
/// avahi-browse -t _nfs._tcp
/// 
/// # Query exports from specific server
/// showmount -e server.local
/// showmount -e 192.168.1.100
/// ```
/// 
/// ## Approach 2: Avahi/mDNS (modern)
/// - Query _nfs._tcp.local for NFS servers
/// - Use D-Bus interface to Avahi daemon
/// - More reliable in modern networks
/// - Can discover NFSv4 servers advertising via mDNS
/// 
/// ## Approach 3: Direct RPC calls
/// - Use mountd RPC protocol (port 635/tcp usually)
/// - Query MOUNT program exports
/// - More complex but doesn't require showmount utility
/// 
/// ## Approach 4: GIO/GVFS integration
/// - Use GTK's GIO to enumerate NFS shares
/// - Leverage GVFS backend (gvfs-nfs)
/// - Best integration with GNOME
/// 
/// For Flatpak:
/// - Ensure D-Bus permissions for Avahi
/// - May need to spawn showmount via flatpak-spawn --host
/// - Consider GIO/GVFS as primary method
/// 
/// Current implementation returns empty list as scaffold.
pub async fn scan_nfs_shares() -> Result<Vec<NfsExport>> {
    tracing::info!("Scanning for NFS exports...");

    let mut exports = Vec::new();
    
    // Method 1: Discover NFS servers via Avahi
    if let Ok(servers) = discover_nfs_servers_avahi().await {
        for server in servers {
            if let Ok(server_exports) = query_server_exports(&server).await {
                exports.extend(server_exports);
            }
        }
    }
    
    // Method 2: Try common broadcast address if avahi fails
    if exports.is_empty() {
        if let Ok(broadcast_exports) = scan_broadcast_servers().await {
            exports.extend(broadcast_exports);
        }
    }
    
    tracing::info!("Found {} NFS exports", exports.len());
    Ok(exports)
}

/// Discover NFS servers using Avahi
async fn discover_nfs_servers_avahi() -> Result<Vec<String>> {
    tracing::debug!("Discovering NFS servers via Avahi...");
    
    let output = std::process::Command::new("avahi-browse")
        .args(&["-t", "-p", "_nfs._tcp"])
        .output()
        .context("Failed to run avahi-browse")?;
    
    if !output.status.success() {
        tracing::warn!("avahi-browse failed, NFS discovery may be limited");
        return Ok(Vec::new());
    }
    
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut servers = Vec::new();
    
    for line in stdout.lines() {
        // Parse avahi-browse output: =;eth0;IPv4;hostname;_nfs._tcp;local
        if line.starts_with('=') {
            let parts: Vec<&str> = line.split(';').collect();
            if parts.len() >= 4 {
                let hostname = parts[3].to_string();
                if !servers.contains(&hostname) {
                    servers.push(hostname);
                }
            }
        }
    }
    
    tracing::debug!("Discovered {} NFS servers via Avahi", servers.len());
    Ok(servers)
}

/// Scan for NFS servers on broadcast
async fn scan_broadcast_servers() -> Result<Vec<NfsExport>> {
    tracing::debug!("Scanning for NFS servers on local network...");
    
    // Try a few common server names/IPs
    let common_servers = vec!["nfs", "nas", "fileserver", "storage"];
    let mut exports = Vec::new();
    
    for server in common_servers {
        if let Ok(server_exports) = query_server_exports(server).await {
            exports.extend(server_exports);
        }
    }
    
    Ok(exports)
}

/// Query NFS exports from a specific server
/// 
/// Helper function to get all exports from a known NFS server.
/// Useful when user knows the server address but wants to see available exports.
/// 
/// # Arguments
/// * `server` - Hostname or IP address of NFS server
/// 
/// # Returns
/// List of exports available on that server
pub async fn query_server_exports(server: &str) -> Result<Vec<NfsExport>> {
    tracing::info!("Querying NFS exports from: {}", server);

    let output = std::process::Command::new("showmount")
        .args(&["-e", server])
        .output()
        .context("Failed to run showmount")?;
    
    if !output.status.success() {
        tracing::warn!("showmount failed for server {}", server);
        return Ok(Vec::new());
    }
    
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut exports = Vec::new();
    
    for line in stdout.lines().skip(1) { // Skip header line
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.is_empty() {
            continue;
        }
        
        let export_path = PathBuf::from(parts[0]);
        
        // Parse options if present (e.g., "(everyone)" or "host1,host2")
        let options = if parts.len() > 1 {
            vec![parts[1..].join(" ")]
        } else {
            vec![]
        };
        
        let nfs_uri = format!("nfs://{}:{}", server, export_path.display());
        
        exports.push(NfsExport {
            server: server.to_string(),
            ip_address: None,
            export_path,
            nfs_uri,
            nfs_version: Some(4), // Default to v4, can be auto-negotiated
            options,
        });
    }
    
    tracing::debug!("Found {} exports on {}", exports.len(), server);
    Ok(exports)
}

/// Mount an NFS export to a local mount point
/// 
/// # Arguments
/// * `export` - The NFS export to mount
/// * `mount_point` - Optional mount point path (auto-generated if None)
/// * `mount_options` - Additional mount options (e.g., "ro", "nolock", "soft")
/// * `persist` - If true, add entry to /etc/fstab for persistence
/// 
/// # Implementation
/// 
/// Will use one of:
/// - GIO/GVFS mount (preferred, no root needed)
/// - mount.nfs command via UDisks2/Polkit
/// - Direct mount with pkexec
/// 
/// For persistence:
/// - Validate /etc/fstab syntax
/// - Check for duplicate entries
/// - Append new entry with proper formatting
/// - Set appropriate options (auto, nofail, etc.)
/// 
/// NFS-specific considerations:
/// - NFSv3 vs NFSv4 mount syntax
/// - Handle mount timeouts
/// - Verify server is reachable
/// - Configure proper mount options (soft vs hard, timeo, retrans)
pub async fn mount_nfs_export(
    export: &NfsExport,
    mount_point: Option<String>,
    mount_options: Vec<String>,
    persist: bool,
) -> Result<String> {
    tracing::info!("Mounting NFS export: {}", export.display_name());

    // Determine mount point
    let mount_path = if let Some(mp) = mount_point {
        mp
    } else {
        // Auto-generate mount point in /run/media/$USER/
        let user = std::env::var("USER").unwrap_or_else(|_| "user".to_string());
        let export_name = export.export_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("nfs_export");
        format!("/run/media/{}/{}", user, export_name)
    };
    
    // Create mount point if it doesn't exist
    std::fs::create_dir_all(&mount_path)
        .with_context(|| format!("Failed to create mount point: {}", mount_path))?;
    
    // Build mount options
    let mut options = vec!["vers=4".to_string()]; // Default to NFSv4
    
    // Add user-provided options
    if !mount_options.is_empty() {
        options.extend(mount_options);
    } else {
        // Add some sensible defaults
        options.push("soft".to_string());
        options.push("timeo=100".to_string());
    }
    
    let options_str = options.join(",");
    let mount_source = export.mount_string();
    
    // Determine NFS version for mount command
    let nfs_type = match export.nfs_version {
        Some(4) => "nfs4",
        _ => "nfs",
    };
    
    // Mount using mount.nfs
    let output = std::process::Command::new("pkexec")
        .args(&[
            "mount",
            "-t",
            nfs_type,
            &mount_source,
            &mount_path,
            "-o",
            &options_str,
        ])
        .output()
        .context("Failed to execute mount command")?;
    
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow::anyhow!(
            "Failed to mount NFS export: {}",
            stderr.trim()
        ));
    }
    
    // Handle persistence
    if persist {
        add_nfs_to_fstab(&mount_source, &mount_path, nfs_type, &options_str)?;
    }
    
    tracing::info!("Successfully mounted {} to {}", mount_source, mount_path);
    Ok(mount_path)
}

/// Add NFS mount entry to /etc/fstab
fn add_nfs_to_fstab(source: &str, mount_point: &str, fs_type: &str, options: &str) -> Result<()> {
    // Read existing fstab
    let fstab_content = std::fs::read_to_string("/etc/fstab")
        .context("Failed to read /etc/fstab")?;
    
    // Check if entry already exists
    for line in fstab_content.lines() {
        if line.contains(source) && line.contains(mount_point) {
            tracing::info!("Entry already exists in /etc/fstab");
            return Ok(());
        }
    }
    
    // Prepare new entry
    let entry = format!("\n{} {} {} {} 0 0\n", source, mount_point, fs_type, options);
    
    // Append to fstab using pkexec
    let temp_file = format!("/tmp/fstab_append_{}", std::process::id());
    std::fs::write(&temp_file, entry)
        .context("Failed to write temporary fstab entry")?;
    
    let output = std::process::Command::new("pkexec")
        .args(&[
            "sh",
            "-c",
            &format!("cat {} >> /etc/fstab", temp_file),
        ])
        .output()
        .context("Failed to update /etc/fstab")?;
    
    // Clean up temp file
    let _ = std::fs::remove_file(&temp_file);
    
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow::anyhow!("Failed to update fstab: {}", stderr.trim()));
    }
    
    tracing::info!("Added NFS mount entry to /etc/fstab");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_nfs_export_display_name() {
        let export = NfsExport {
            server: "nfs.local".to_string(),
            ip_address: None,
            export_path: PathBuf::from("/export/data"),
            nfs_uri: "nfs://nfs.local:/export/data".to_string(),
            nfs_version: Some(4),
            options: vec!["rw".to_string(), "sync".to_string()],
        };

        assert_eq!(export.display_name(), "/export/data on nfs.local");
    }

    #[test]
    fn test_mount_string() {
        let export = NfsExport {
            server: "192.168.1.100".to_string(),
            ip_address: None,
            export_path: PathBuf::from("/mnt/storage"),
            nfs_uri: "nfs://192.168.1.100:/mnt/storage".to_string(),
            nfs_version: Some(3),
            options: vec!["ro".to_string()],
        };

        assert_eq!(export.mount_string(), "192.168.1.100:/mnt/storage");
    }

    #[test]
    fn test_is_read_only() {
        let export_ro = NfsExport {
            server: "server".to_string(),
            ip_address: None,
            export_path: PathBuf::from("/export"),
            nfs_uri: "nfs://server:/export".to_string(),
            nfs_version: Some(4),
            options: vec!["ro".to_string()],
        };

        let export_rw = NfsExport {
            server: "server".to_string(),
            ip_address: None,
            export_path: PathBuf::from("/export"),
            nfs_uri: "nfs://server:/export".to_string(),
            nfs_version: Some(4),
            options: vec!["rw".to_string()],
        };

        assert!(export_ro.is_read_only());
        assert!(!export_rw.is_read_only());
    }
}
