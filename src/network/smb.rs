use anyhow::{Context, Result};
use std::net::IpAddr;

/// Represents a discovered SMB/CIFS share
#[derive(Debug, Clone)]
pub struct SmbShare {
    /// Server hostname or IP address
    pub server: String,
    
    /// Server IP address (resolved)
    pub ip_address: Option<IpAddr>,
    
    /// Share name (e.g., "Public", "Documents", "Media")
    pub share_name: String,
    
    /// Full UNC path (e.g., "//server/share")
    pub unc_path: String,
    
    /// Workgroup or domain name
    pub workgroup: Option<String>,
    
    /// Whether authentication is required
    pub requires_auth: bool,
    
    /// Description/comment from the server
    pub description: Option<String>,
}

impl SmbShare {
    /// Create mount URI for CIFS
    /// Returns something like "//192.168.1.100/Public"
    pub fn mount_uri(&self) -> String {
        self.unc_path.clone()
    }

    /// Get display name for UI
    pub fn display_name(&self) -> String {
        format!("{} on {}", self.share_name, self.server)
    }
}

/// Scan the local network for available SMB/CIFS shares
/// 
/// This function discovers SMB shares by:
/// 1. Using Avahi/mDNS to find SMB servers (_smb._tcp)
/// 2. Using nmblookup to query NetBIOS names
/// 3. Using smbclient -L to list shares on discovered servers
/// 4. Parsing and aggregating results
/// 
/// # Implementation Strategy
/// 
/// Multiple approaches for SMB discovery:
/// 
/// ## Approach 1: smbclient (traditional)
/// ```bash
/// # Find servers
/// nmblookup -S '*'
/// smbtree -N
/// 
/// # List shares on specific server
/// smbclient -L //server -N
/// ```
/// 
/// ## Approach 2: Avahi/mDNS (modern)
/// - Query _smb._tcp.local for SMB servers
/// - Use D-Bus interface to Avahi daemon
/// - More reliable in modern networks
/// 
/// ## Approach 3: GIO/GVFS integration
/// - Use GTK's GIO to enumerate SMB shares
/// - Leverage GVFS backend (gvfs-smb)
/// - Best integration with GNOME
/// 
/// For Flatpak:
/// - Ensure D-Bus permissions for Avahi
/// - May need to spawn smbclient via flatpak-spawn --host
/// - Consider GIO/GVFS as primary method
/// 
/// Current implementation returns empty list as scaffold.
pub async fn scan_smb_shares() -> Result<Vec<SmbShare>> {
    tracing::info!("Scanning for SMB/CIFS shares...");

    let mut shares = Vec::new();
    
    // Try multiple discovery methods
    
    // Method 1: avahi-browse for _smb._tcp services
    if let Ok(servers) = discover_smb_servers_avahi().await {
        for server in servers {
            if let Ok(server_shares) = list_server_shares(&server).await {
                shares.extend(server_shares);
            }
        }
    }
    
    // Method 2: smbtree for network browsing
    if shares.is_empty() {
        if let Ok(tree_shares) = scan_smbtree().await {
            shares.extend(tree_shares);
        }
    }
    
    tracing::info!("Found {} SMB shares", shares.len());
    Ok(shares)
}

/// Discover SMB servers using Avahi
async fn discover_smb_servers_avahi() -> Result<Vec<String>> {
    tracing::debug!("Discovering SMB servers via Avahi...");
    
    let output = std::process::Command::new("avahi-browse")
        .args(&["-t", "-p", "_smb._tcp"])
        .output()
        .context("Failed to run avahi-browse")?;
    
    if !output.status.success() {
        tracing::warn!("avahi-browse failed, SMB discovery may be limited");
        return Ok(Vec::new());
    }
    
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut servers = Vec::new();
    
    for line in stdout.lines() {
        // Parse avahi-browse output: =;eth0;IPv4;hostname;_smb._tcp;local
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
    
    tracing::debug!("Discovered {} SMB servers via Avahi", servers.len());
    Ok(servers)
}

/// List shares on a specific SMB server
async fn list_server_shares(server: &str) -> Result<Vec<SmbShare>> {
    tracing::debug!("Listing shares on server: {}", server);
    
    let output = std::process::Command::new("smbclient")
        .args(&["-L", server, "-N", "-g"]) // -N: no password, -g: grepable
        .output()
        .context("Failed to run smbclient")?;
    
    if !output.status.success() {
        tracing::warn!("smbclient failed for server {}", server);
        return Ok(Vec::new());
    }
    
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut shares = Vec::new();
    
    for line in stdout.lines() {
        // Parse smbclient -g output: Disk|share_name|comment
        if line.starts_with("Disk|") {
            let parts: Vec<&str> = line.split('|').collect();
            if parts.len() >= 2 {
                let share_name = parts[1].to_string();
                
                // Skip administrative shares
                if share_name.ends_with('$') {
                    continue;
                }
                
                let description = if parts.len() >= 3 && !parts[2].is_empty() {
                    Some(parts[2].to_string())
                } else {
                    None
                };
                
                let unc_path = format!("//{}/{}", server, share_name);
                
                shares.push(SmbShare {
                    server: server.to_string(),
                    ip_address: None, // TODO: resolve IP
                    share_name,
                    unc_path,
                    workgroup: None,
                    requires_auth: false, // We used -N, so these are guest accessible
                    description,
                });
            }
        }
    }
    
    tracing::debug!("Found {} shares on {}", shares.len(), server);
    Ok(shares)
}

/// Scan using smbtree command (alternative method)
async fn scan_smbtree() -> Result<Vec<SmbShare>> {
    tracing::debug!("Scanning SMB network with smbtree...");
    
    let output = std::process::Command::new("smbtree")
        .args(&["-N", "--no-pass"])
        .output()
        .context("Failed to run smbtree")?;
    
    if !output.status.success() {
        tracing::warn!("smbtree command failed");
        return Ok(Vec::new());
    }
    
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut shares = Vec::new();
    let mut current_server = String::new();
    
    for line in stdout.lines() {
        let trimmed = line.trim();
        
        // Server line: \\SERVER
        if trimmed.starts_with("\\\\") && !trimmed.contains("\\\\") {
            current_server = trimmed.trim_start_matches("\\\\").to_string();
        }
        // Share line: \t\\SERVER\SHARE
        else if trimmed.starts_with("\\\\") && trimmed.contains('\\') {
            if let Some(unc) = trimmed.split_whitespace().next() {
                let parts: Vec<&str> = unc.trim_start_matches("\\\\").split('\\').collect();
                if parts.len() == 2 {
                    let server = parts[0].to_string();
                    let share_name = parts[1].to_string();
                    
                    // Skip administrative shares
                    if share_name.ends_with('$') {
                        continue;
                    }
                    
                    let unc_path = format!("//{}/{}", server, share_name);
                    
                    shares.push(SmbShare {
                        server,
                        ip_address: None,
                        share_name,
                        unc_path,
                        workgroup: None,
                        requires_auth: false,
                        description: None,
                    });
                }
            }
        }
    }
    
    tracing::debug!("Found {} shares via smbtree", shares.len());
    Ok(shares)
}

/// Mount an SMB share to a local mount point
/// 
/// # Arguments
/// * `share` - The SMB share to mount
/// * `mount_point` - Optional mount point path (auto-generated if None)
/// * `username` - Optional username for authentication
/// * `password` - Optional password for authentication
/// * `persist` - If true, add entry to /etc/fstab for persistence
/// 
/// # Implementation
/// 
/// Will use one of:
/// - GIO/GVFS mount (preferred, no root needed)
/// - mount.cifs command via UDisks2/Polkit
/// - Direct mount.cifs with pkexec
/// 
/// For persistence:
/// - Validate /etc/fstab syntax
/// - Check for duplicate entries
/// - Append new entry with proper escaping
/// - Store credentials in /root/.smbcredentials or similar
pub async fn mount_smb_share(
    share: &SmbShare,
    mount_point: Option<String>,
    username: Option<String>,
    password: Option<String>,
    persist: bool,
) -> Result<String> {
    tracing::info!("Mounting SMB share: {}", share.display_name());

    // Determine mount point
    let mount_path = if let Some(mp) = mount_point {
        mp
    } else {
        // Auto-generate mount point in /run/media/$USER/
        let user = std::env::var("USER").unwrap_or_else(|_| "user".to_string());
        format!("/run/media/{}/{}", user, share.share_name)
    };
    
    // Create mount point if it doesn't exist
    std::fs::create_dir_all(&mount_path)
        .with_context(|| format!("Failed to create mount point: {}", mount_path))?;
    
    // Build mount options
    let mut options = vec!["vers=3.0".to_string()];
    
    if let Some(ref user) = username {
        options.push(format!("username={}", user));
        if let Some(ref pass) = password {
            options.push(format!("password={}", pass));
        }
    } else {
        options.push("guest".to_string());
    }
    
    let options_str = options.join(",");
    
    // Mount using mount.cifs
    let output = std::process::Command::new("pkexec")
        .args(&[
            "mount",
            "-t",
            "cifs",
            &share.unc_path,
            &mount_path,
            "-o",
            &options_str,
        ])
        .output()
        .context("Failed to execute mount command")?;
    
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow::anyhow!(
            "Failed to mount SMB share: {}",
            stderr.trim()
        ));
    }
    
    // Handle persistence
    if persist {
        add_to_fstab(&share.unc_path, &mount_path, &options_str)?;
    }
    
    tracing::info!("Successfully mounted {} to {}", share.unc_path, mount_path);
    Ok(mount_path)
}

/// Add mount entry to /etc/fstab
fn add_to_fstab(source: &str, mount_point: &str, options: &str) -> Result<()> {
    
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
    let entry = format!("\n{} {} cifs {} 0 0\n", source, mount_point, options);
    
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
    
    tracing::info!("Added mount entry to /etc/fstab");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_smb_share_display_name() {
        let share = SmbShare {
            server: "fileserver".to_string(),
            ip_address: None,
            share_name: "Public".to_string(),
            unc_path: "//fileserver/Public".to_string(),
            workgroup: Some("WORKGROUP".to_string()),
            requires_auth: false,
            description: Some("Public files".to_string()),
        };

        assert_eq!(share.display_name(), "Public on fileserver");
    }

    #[test]
    fn test_mount_uri() {
        let share = SmbShare {
            server: "192.168.1.100".to_string(),
            ip_address: None,
            share_name: "Media".to_string(),
            unc_path: "//192.168.1.100/Media".to_string(),
            workgroup: None,
            requires_auth: true,
            description: None,
        };

        assert_eq!(share.mount_uri(), "//192.168.1.100/Media");
    }
}
