//! Thin wrapper around the `udisks2` crate's D-Bus client.
//!
//! Covers exactly the operations DriveDock needs:
//! - `Filesystem.Mount` / `Filesystem.Unmount` for local block devices (privileged,
//!   Polkit-gated by UDisks2 itself - no manual `pkexec` needed).
//! - `Block.AddConfigurationItem` / `RemoveConfigurationItem` for "permanently dock"
//!   (fstab) persistence.
//!
//! Network shares are handled separately, via GVfs (`crate::network::gvfs`) - see
//! the Phase 1 spec for why UDisks2's `Filesystem` interface only applies to local
//! block devices.

use anyhow::{anyhow, Result};
use std::collections::HashMap;
use udisks2::zbus::zvariant::Value;
use udisks2::{Client, Object};

/// A thin wrapper around `udisks2::Client`.
pub struct Udisks {
    client: Client,
}

impl Udisks {
    /// Connect to the system UDisks2 D-Bus service.
    pub async fn new() -> Result<Self> {
        let client = Client::new()
            .await
            .map_err(|e| anyhow!("Failed to connect to UDisks2: {e}"))?;
        Ok(Self { client })
    }

    /// Find the UDisks2 `Object` for a local block device.
    ///
    /// Prefers matching by filesystem UUID (stable across device renumbering), and
    /// falls back to matching the raw device path (e.g. "/dev/sdb1") against each
    /// block object's `Device` property when no UUID is available - per spec risk 5.
    pub async fn find_block_object(
        &self,
        unix_device: &str,
        uuid: Option<&str>,
    ) -> Result<Object> {
        if let Some(uuid) = uuid.filter(|u| !u.is_empty()) {
            if let Some(block) = self.client.block_for_uuid(uuid).await.into_iter().next() {
                let path = block.inner().path().to_owned();
                return self
                    .client
                    .object(path)
                    .map_err(|e| anyhow!("Invalid object path: {e:?}"));
            }
        }

        // Fallback: scan all block devices for a matching `Device` path.
        for (object_path, _) in self
            .client
            .object_manager()
            .get_managed_objects()
            .await
            .map_err(|e| anyhow!("Failed to list UDisks2 objects: {e}"))?
            .into_iter()
        {
            let object = self
                .client
                .object(object_path)
                .map_err(|e| anyhow!("Invalid object path: {e:?}"))?;
            let Ok(block) = object.block().await else {
                continue;
            };
            let Ok(device_bytes) = block.device().await else {
                continue;
            };
            if bytes_to_str(&device_bytes) == unix_device {
                return Ok(object);
            }
        }

        Err(anyhow!(
            "No UDisks2 block object found for device {unix_device}"
        ))
    }

    /// Mount a local block device's filesystem. Returns the mount point path.
    pub async fn mount(&self, object: &Object) -> Result<String> {
        let filesystem = object
            .filesystem()
            .await
            .map_err(|e| anyhow!("Not a mountable filesystem: {e}"))?;
        filesystem
            .mount(HashMap::new())
            .await
            .map_err(|e| anyhow!("UDisks2 mount failed: {e}"))
    }

    /// Unmount a local block device's filesystem.
    pub async fn unmount(&self, object: &Object) -> Result<()> {
        let filesystem = object
            .filesystem()
            .await
            .map_err(|e| anyhow!("Not a mountable filesystem: {e}"))?;
        filesystem
            .unmount(HashMap::new())
            .await
            .map_err(|e| anyhow!("UDisks2 unmount failed: {e}"))
    }

    /// Add a `fstab` configuration item for this block device, so it mounts at
    /// `mount_point` on boot (via UDisks2's own privileged helper - Polkit-gated,
    /// no raw file editing).
    ///
    /// `fsname` is intentionally omitted so UDisks2 defaults it to `UUID=...`.
    pub async fn add_fstab_entry(
        &self,
        object: &Object,
        mount_point: &str,
        fs_type: &str,
        options: &str,
    ) -> Result<()> {
        let block = object
            .block()
            .await
            .map_err(|e| anyhow!("Not a block device: {e}"))?;

        let mut details: HashMap<&str, Value> = HashMap::new();
        details.insert("dir", Value::from(str_to_bytes(mount_point)));
        details.insert("type", Value::from(str_to_bytes(fs_type)));
        details.insert("opts", Value::from(str_to_bytes(options)));
        details.insert("freq", Value::from(0i32));
        details.insert("passno", Value::from(0i32));

        let item = ("fstab", details);
        block
            .add_configuration_item(&item, HashMap::new())
            .await
            .map_err(|e| anyhow!("Failed to add fstab entry: {e}"))
    }

    /// Remove this block device's `fstab` configuration item (inverse of
    /// `add_fstab_entry`), un-toggling "permanently dock".
    pub async fn remove_fstab_entry(&self, object: &Object) -> Result<()> {
        let block = object
            .block()
            .await
            .map_err(|e| anyhow!("Not a block device: {e}"))?;

        let configuration = block
            .configuration()
            .await
            .map_err(|e| anyhow!("Failed to read configuration: {e}"))?;

        let Some((item_type, details)) = configuration.into_iter().find(|(ty, _)| ty == "fstab")
        else {
            // Nothing to remove.
            return Ok(());
        };

        let details_ref: HashMap<&str, Value> = details
            .iter()
            .map(|(k, v)| (k.as_str(), Value::from(v.clone())))
            .collect();
        let item = (item_type.as_str(), details_ref);

        block
            .remove_configuration_item(&item, HashMap::new())
            .await
            .map_err(|e| anyhow!("Failed to remove fstab entry: {e}"))
    }

    /// Best-effort lookup of the filesystem UUID for a block device, used to build the
    /// NixOS `fileSystems` config snippet.
    pub async fn uuid_for(&self, object: &Object) -> Option<String> {
        let block = object.block().await.ok()?;
        block.id_uuid().await.ok().filter(|u| !u.is_empty())
    }
}

fn str_to_bytes(s: &str) -> Vec<u8> {
    let mut bytes = s.as_bytes().to_vec();
    bytes.push(0);
    bytes
}

fn bytes_to_str(bytes: &[u8]) -> String {
    let trimmed = bytes.strip_suffix(&[0]).unwrap_or(bytes);
    String::from_utf8_lossy(trimmed).to_string()
}
