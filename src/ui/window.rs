use gtk::prelude::*;
use gtk::glib;
use libadwaita as adw;
use adw::prelude::*;
use crate::system;
use crate::network;

/// Main application window
/// 
/// This struct represents the DriveDock main window with three primary sections:
/// 1. Local Drives - displays mounted drives with unmount functionality
/// 2. Network Drives - shows available SMB/NFS shares with mount options
/// 3. Status - displays operation messages and feedback
pub struct Window {
    pub window: adw::ApplicationWindow,
}

impl Window {
    /// Create a new DriveDock window
    pub fn new(app: &adw::Application) -> Self {
        // Create the main application window
        let window = adw::ApplicationWindow::builder()
            .application(app)
            .title("DriveDock")
            .default_width(800)
            .default_height(600)
            .build();

        // Build the window content
        let content = Self::build_content();
        window.set_content(Some(&content));

        Self { window }
    }

    /// Build the main content layout
    /// 
    /// Creates a modern GNOME-style layout with:
    /// - Header bar with title and refresh button
    /// - Main content area with three sections
    fn build_content() -> gtk::Widget {
        // Create the main toolbar view (provides header bar and content area)
        let toolbar_view = adw::ToolbarView::new();

        // Create header bar
        let header_bar = adw::HeaderBar::new();
        
        // Add refresh button to header
        let refresh_btn = gtk::Button::builder()
            .icon_name("view-refresh-symbolic")
            .tooltip_text("Refresh all drives and shares")
            .build();
        header_bar.pack_end(&refresh_btn);
        
        toolbar_view.add_top_bar(&header_bar);

        // Create main content box with vertical orientation
        let main_box = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(0)
            .build();

        // Use AdwClamp to constrain width on large screens (adaptive design)
        let clamp = adw::Clamp::builder()
            .maximum_size(1000)
            .tightening_threshold(600)
            .child(&main_box)
            .build();

        // Create scrolled window for content
        let scrolled = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vscrollbar_policy(gtk::PolicyType::Automatic)
            .child(&clamp)
            .vexpand(true)
            .build();

        toolbar_view.set_content(Some(&scrolled));

        // Add the three main sections
        let drives_section = Self::build_local_drives_section();
        main_box.append(&drives_section);
        
        let network_section = Self::build_network_drives_section();
        main_box.append(&network_section);
        main_box.append(&Self::build_status_section());

        // Load drives after UI is built
        Self::load_drives(&drives_section);
        
        // Connect refresh button
        let drives_section_clone = drives_section.clone();
        refresh_btn.connect_clicked(move |btn| {
            btn.set_sensitive(false);
            let drives_section = drives_section_clone.clone();
            let btn_clone = btn.clone();
            
            glib::spawn_future_local(async move {
                tracing::info!("Refreshing drives and shares...");
                
                // Collect all child rows first
                let mut rows_to_remove = Vec::new();
                let mut child = drives_section.first_child();
                while let Some(row) = child {
                    let next = row.next_sibling();
                    rows_to_remove.push(row);
                    child = next;
                }
                
                // Remove all rows
                for row in rows_to_remove {
                    drives_section.remove(&row);
                }
                
                // Reload drives
                Self::load_drives(&drives_section);
                
                // Note: Network shares need to be manually rescanned via their button
                // since we don't have a reference to the network section here
                
                btn_clone.set_sensitive(true);
            });
        });

        toolbar_view.upcast()
    }

    /// Build the Local Drives section
    /// 
    /// Returns a PreferencesGroup that will be populated with drive data
    fn build_local_drives_section() -> adw::PreferencesGroup {
        adw::PreferencesGroup::builder()
            .title("Local Drives")
            .description("Mounted local drives and partitions")
            .margin_top(24)
            .margin_bottom(12)
            .margin_start(12)
            .margin_end(12)
            .build()
    }

    /// Load and display mounted drives
    fn load_drives(group: &adw::PreferencesGroup) {
        let group = group.clone();

        // Spawn async task to load drives
        glib::spawn_future_local(async move {
            match system::list_mounted_drives().await {
                Ok(drives) => {
                    if drives.is_empty() {
                        // Show placeholder
                        let placeholder_row = adw::ActionRow::builder()
                            .title("No drives found")
                            .subtitle("No mounted drives detected")
                            .build();
                        let icon = gtk::Image::from_icon_name("drive-harddisk-symbolic");
                        placeholder_row.add_prefix(&icon);
                        group.add(&placeholder_row);
                    } else {
                        // Add each drive as a row
                        for drive in drives {
                            let row = Self::create_drive_row(&drive);
                            group.add(&row);
                        }
                    }
                }
                Err(e) => {
                    tracing::error!("Failed to load drives: {}", e);
                    let error_row = adw::ActionRow::builder()
                        .title("Error loading drives")
                        .subtitle(format!("Failed to read /proc/mounts: {}", e))
                        .build();
                    let icon = gtk::Image::from_icon_name("dialog-error-symbolic");
                    error_row.add_prefix(&icon);
                    group.add(&error_row);
                }
            }
        });
    }

    /// Create a UI row for a mounted drive
    fn create_drive_row(drive: &system::MountedDrive) -> adw::ActionRow {
        // Format the subtitle with mount point and filesystem
        let subtitle = format!(
            "{} • {}",
            drive.mount_point.display(),
            drive.fs_type
        );

        let row = adw::ActionRow::builder()
            .title(&drive.device)
            .subtitle(&subtitle)
            .build();

        // Add drive icon
        let icon = gtk::Image::from_icon_name(Self::icon_for_drive(drive));
        row.add_prefix(&icon);

        // Add storage usage visualization
        if let (Some(total), Some(used)) = (drive.total_size, drive.used_space) {
            // Create vertical box for storage info
            let storage_box = gtk::Box::builder()
                .orientation(gtk::Orientation::Vertical)
                .spacing(4)
                .valign(gtk::Align::Center)
                .build();

            // Calculate usage percentage
            let used_fraction = if total > 0 {
                used as f64 / total as f64
            } else {
                0.0
            };

            // Create level bar (storage bar)
            let level_bar = gtk::LevelBar::builder()
                .min_value(0.0)
                .max_value(1.0)
                .value(used_fraction)
                .width_request(150)
                .build();

            // Add warning/full offsets for color coding
            level_bar.add_offset_value(gtk::LEVEL_BAR_OFFSET_LOW, 0.75);
            level_bar.add_offset_value(gtk::LEVEL_BAR_OFFSET_HIGH, 0.90);
            level_bar.add_offset_value(gtk::LEVEL_BAR_OFFSET_FULL, 1.0);

            // Format size labels
            let available = total - used;
            let label_text = format!(
                "{} available of {}",
                system::MountedDrive::format_size(available),
                system::MountedDrive::format_size(total)
            );

            let size_label = gtk::Label::builder()
                .label(&label_text)
                .css_classes(vec!["caption".to_string(), "dim-label".to_string()])
                .xalign(0.0)
                .build();

            storage_box.append(&level_bar);
            storage_box.append(&size_label);
            row.add_suffix(&storage_box);
        }

        // Add unmount button if safe
        if drive.is_safe_to_unmount {
            let unmount_btn = gtk::Button::builder()
                .icon_name("media-eject-symbolic")
                .tooltip_text("Unmount drive")
                .valign(gtk::Align::Center)
                .build();
            unmount_btn.add_css_class("flat");

            let device = drive.device.clone();
            let mount_point_path = drive.mount_point.clone();
            
            unmount_btn.connect_clicked(move |btn| {
                tracing::info!("Unmount requested for: {}", device);
                btn.set_sensitive(false);
                
                let mount_point = mount_point_path.clone();
                let btn_clone = btn.clone();
                
                // Spawn async unmount
                gtk::glib::spawn_future_local(async move {
                    match system::unmount_drive(&mount_point).await {
                        Ok(()) => {
                            tracing::info!("Successfully unmounted {}", mount_point.display());
                            // Show success message
                            let dialog = adw::AlertDialog::builder()
                                .heading("Drive Unmounted")
                                .body(&format!("Successfully unmounted {}", mount_point.display()))
                                .build();
                            dialog.add_response("ok", "OK");
                            if let Some(window) = btn_clone.root().and_downcast_ref::<gtk::Window>() {
                                dialog.present(Some(window));
                            }
                        }
                        Err(e) => {
                            tracing::error!("Failed to unmount {}: {}", mount_point.display(), e);
                            btn_clone.set_sensitive(true);
                            // Show error to user
                            let dialog = adw::AlertDialog::builder()
                                .heading("Unmount Failed")
                                .body(&format!("Failed to unmount drive: {}", e))
                                .build();
                            dialog.add_response("ok", "OK");
                            if let Some(window) = btn_clone.root().and_downcast_ref::<gtk::Window>() {
                                dialog.present(Some(window));
                            }
                        }
                    }
                });
            });

            row.add_suffix(&unmount_btn);
        } else {
            // Add lock icon to indicate system drive
            let lock_icon = gtk::Image::from_icon_name("changes-prevent-symbolic");
            lock_icon.set_tooltip_text(Some("System drive - cannot unmount"));
            row.add_suffix(&lock_icon);
        }

        row
    }

    /// Select appropriate icon for drive type
    fn icon_for_drive(drive: &system::MountedDrive) -> &'static str {
        if drive.device.contains("nvme") {
            "drive-harddisk-solidstate-symbolic"
        } else if drive.device.contains("mmcblk") || drive.device.contains("sd") && drive.device.len() < 10 {
            "media-flash-sd-symbolic"
        } else if drive.device.contains("loop") {
            "media-optical-symbolic"
        } else {
            "drive-harddisk-symbolic"
        }
    }

    /// Build the Network Drives section
    /// 
    /// This section will display detected network shares:
    /// - SMB/CIFS shares
    /// - NFS exports
    /// - Protocol labels
    /// - Mount button with persistence option
    fn build_network_drives_section() -> gtk::Widget {
        // Create a container box for the whole section
        let section_box = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(0)
            .build();
        
        // Create the group for network shares
        let group = adw::PreferencesGroup::builder()
            .title("Network Drives")
            .margin_top(12)
            .margin_bottom(12)
            .margin_start(12)
            .margin_end(12)
            .build();
        
        // Create a header row with scan button that stays persistent
        let header_row = adw::ActionRow::builder()
            .title("Network Shares")
            .subtitle("SMB and NFS shares on the network")
            .build();
        
        let scan_button = gtk::Button::builder()
            .icon_name("view-refresh-symbolic")
            .tooltip_text("Scan for network shares")
            .valign(gtk::Align::Center)
            .build();
        scan_button.add_css_class("flat");
        header_row.add_suffix(&scan_button);
        
        let header_icon = gtk::Image::from_icon_name("network-server-symbolic");
        header_row.add_prefix(&header_icon);
        
        group.add(&header_row);

        // Placeholder row
        let placeholder_row = adw::ActionRow::builder()
            .title("Click scan button to discover shares")
            .build();

        // Connect scan button click - will trigger network scanning
        let group_clone = group.clone();
        scan_button.connect_clicked(move |btn| {
            btn.set_sensitive(false);
            let group = group_clone.clone();
            let btn_clone = btn.clone();
            
            glib::spawn_future_local(async move {
                tracing::info!("Scanning for network shares...");
                
                // Create scanning row
                let scanning_row = adw::ActionRow::builder()
                    .title("Scanning...")
                    .subtitle("Discovering network shares (may take a moment)")
                    .build();
                let spinner = gtk::Spinner::new();
                spinner.start();
                scanning_row.add_prefix(&spinner);
                
                // Remove only share rows (skip first child which is the header)
                let mut rows_to_remove = Vec::new();
                let mut child = group.first_child();
                let mut is_first = true;
                while let Some(row) = child {
                    let next = row.next_sibling();
                    if !is_first {
                        rows_to_remove.push(row);
                    }
                    is_first = false;
                    child = next;
                }
                
                for row in rows_to_remove {
                    group.remove(&row);
                }
                
                group.add(&scanning_row);
                
                // Clone for UI updates
                let group_for_updates = group.clone();
                let scanning_row_cleanup = scanning_row.clone();
                let btn_cleanup = btn_clone.clone();
                
                // Shared state for results
                let smb_results = std::sync::Arc::new(std::sync::Mutex::new(None));
                let nfs_results = std::sync::Arc::new(std::sync::Mutex::new(None));
                
                let smb_results_scan = smb_results.clone();
                let nfs_results_scan = nfs_results.clone();
                
                // SMB scan thread
                std::thread::spawn(move || {
                    // Use futures executor for async without glib context
                    let result = futures::executor::block_on(network::scan_smb_shares());
                    *smb_results_scan.lock().unwrap() = Some(result);
                });
                
                // NFS scan thread
                std::thread::spawn(move || {
                    // Use futures executor for async without glib context
                    let result = futures::executor::block_on(network::scan_nfs_shares());
                    *nfs_results_scan.lock().unwrap() = Some(result);
                });
                
                // Poll for completion and update UI on main thread
                glib::timeout_add_local(std::time::Duration::from_millis(100), move || {
                    let smb_done = smb_results.lock().unwrap().is_some();
                    let nfs_done = nfs_results.lock().unwrap().is_some();
                    
                    if smb_done && nfs_done {
                        // Add SMB shares
                        if let Some(Ok(smb_shares)) = smb_results.lock().unwrap().take() {
                            for share in smb_shares {
                                let row = Self::create_smb_share_row(&share);
                                group_for_updates.add(&row);
                            }
                        }
                        
                        // Add NFS exports
                        if let Some(Ok(nfs_exports)) = nfs_results.lock().unwrap().take() {
                            for export in nfs_exports {
                                let row = Self::create_nfs_export_row(&export);
                                group_for_updates.add(&row);
                            }
                        }
                        
                        // Clean up scanning row
                        group_for_updates.remove(&scanning_row_cleanup);
                        
                        // Check if we have any shares (more than just header)
                        let mut count = 0;
                        let mut child = group_for_updates.first_child();
                        while let Some(row) = child {
                            count += 1;
                            child = row.next_sibling();
                        }
                        
                        if count <= 1 {  // Only header, no shares
                            let no_shares_row = adw::ActionRow::builder()
                                .title("No shares found")
                                .subtitle("No SMB or NFS shares detected on the network")
                                .build();
                            let icon = gtk::Image::from_icon_name("network-offline-symbolic");
                            no_shares_row.add_prefix(&icon);
                            group_for_updates.add(&no_shares_row);
                        }
                        
                        btn_cleanup.set_sensitive(true);
                        
                        glib::ControlFlow::Break
                    } else {
                        glib::ControlFlow::Continue
                    }
                });
            });
        });

        group.add(&placeholder_row);
        
        section_box.append(&group);
        section_box.upcast()
    }
    
    /// Create a UI row for an SMB share
    fn create_smb_share_row(share: &network::SmbShare) -> adw::ActionRow {
        let subtitle = format!("SMB: {}", share.unc_path);
        
        let row = adw::ActionRow::builder()
            .title(&share.share_name)
            .subtitle(&subtitle)
            .build();
        
        // Add SMB icon
        let icon = gtk::Image::from_icon_name("folder-remote-symbolic");
        row.add_prefix(&icon);
        
        // Add mount button
        let mount_btn = gtk::Button::builder()
            .icon_name("list-add-symbolic")
            .tooltip_text("Mount share")
            .valign(gtk::Align::Center)
            .build();
        mount_btn.add_css_class("flat");
        
        let share_clone = share.clone();
        mount_btn.connect_clicked(move |btn| {
            btn.set_sensitive(false);
            let share = share_clone.clone();
            let btn_clone = btn.clone();
            
            glib::spawn_future_local(async move {
                // Show authentication dialog for SMB
                Self::show_smb_auth_dialog(&share, &btn_clone).await;
            });
        });
        
        row.add_suffix(&mount_btn);
        row
    }
    
    /// Create a UI row for an NFS export
    fn create_nfs_export_row(export: &network::NfsExport) -> adw::ActionRow {
        let subtitle = format!("NFS: {}", export.mount_string());
        
        let row = adw::ActionRow::builder()
            .title(export.export_path.display().to_string())
            .subtitle(&subtitle)
            .build();
        
        // Add NFS icon
        let icon = gtk::Image::from_icon_name("folder-remote-symbolic");
        row.add_prefix(&icon);
        
        // Add mount button
        let mount_btn = gtk::Button::builder()
            .icon_name("list-add-symbolic")
            .tooltip_text("Mount export")
            .valign(gtk::Align::Center)
            .build();
        mount_btn.add_css_class("flat");
        
        let export_clone = export.clone();
        mount_btn.connect_clicked(move |btn| {
            btn.set_sensitive(false);
            let export = export_clone.clone();
            let btn_clone = btn.clone();
            
            glib::spawn_future_local(async move {
                match network::mount_nfs_export(&export, None, vec![], false).await {
                    Ok(mount_path) => {
                        tracing::info!("Successfully mounted NFS export to {}", mount_path);
                        let dialog = adw::AlertDialog::builder()
                            .heading("NFS Export Mounted")
                            .body(&format!("Successfully mounted to {}", mount_path))
                            .build();
                        dialog.add_response("ok", "OK");
                        if let Some(window) = btn_clone.root().and_downcast_ref::<gtk::Window>() {
                            dialog.present(Some(window));
                        }
                    }
                    Err(e) => {
                        tracing::error!("Failed to mount NFS export: {}", e);
                        btn_clone.set_sensitive(true);
                        let dialog = adw::AlertDialog::builder()
                            .heading("Mount Failed")
                            .body(&format!("Failed to mount NFS export: {}", e))
                            .build();
                        dialog.add_response("ok", "OK");
                        if let Some(window) = btn_clone.root().and_downcast_ref::<gtk::Window>() {
                            dialog.present(Some(window));
                        }
                    }
                }
            });
        });
        
        row.add_suffix(&mount_btn);
        row
    }
    
    /// Show authentication dialog for SMB mounting
    async fn show_smb_auth_dialog(share: &network::SmbShare, btn: &gtk::Button) {
        // For now, try guest mount
        // TODO: Implement proper dialog with username/password entry
        match network::mount_smb_share(share, None, None, None, false).await {
            Ok(mount_path) => {
                tracing::info!("Successfully mounted SMB share to {}", mount_path);
                let dialog = adw::AlertDialog::builder()
                    .heading("SMB Share Mounted")
                    .body(&format!("Successfully mounted to {}", mount_path))
                    .build();
                dialog.add_response("ok", "OK");
                if let Some(window) = btn.root().and_downcast_ref::<gtk::Window>() {
                    dialog.present(Some(window));
                }
            }
            Err(e) => {
                tracing::error!("Failed to mount SMB share: {}", e);
                btn.set_sensitive(true);
                let dialog = adw::AlertDialog::builder()
                    .heading("Mount Failed")
                    .body(&format!("Failed to mount SMB share: {}\n\nTip: Guest mounting attempted. Authentication dialog coming soon.", e))
                    .build();
                dialog.add_response("ok", "OK");
                if let Some(window) = btn.root().and_downcast_ref::<gtk::Window>() {
                    dialog.present(Some(window));
                }
            }
        }
    }

    /// Build the Status section
    /// 
    /// This section displays:
    /// - Success messages (green)
    /// - Error messages (red)
    /// - Warning messages (yellow)
    /// - Operational feedback
    fn build_status_section() -> gtk::Widget {
        let group = adw::PreferencesGroup::builder()
            .title("Status")
            .description("Operation messages and feedback")
            .margin_top(12)
            .margin_bottom(24)
            .margin_start(12)
            .margin_end(12)
            .build();

        // Status message row with info banner styling
        let status_row = adw::ActionRow::builder()
            .title("Ready")
            .subtitle("DriveDock is ready to manage your drives")
            .build();

        let icon = gtk::Image::from_icon_name("emblem-ok-symbolic");
        status_row.add_prefix(&icon);

        group.add(&status_row);

        // TODO: Make this dynamic with message queue system
        // For future: Use gtk::ListBox with custom StatusMessage widgets

        group.upcast()
    }

    /// Present the window to the user
    pub fn present(&self) {
        self.window.present();
    }
}
