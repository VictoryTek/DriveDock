use gtk::prelude::*;
use gtk::{gio, glib};
use libadwaita as adw;
use adw::prelude::*;
use crate::system;

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
    /// - Header bar with title
    /// - Main content area with three sections
    fn build_content() -> gtk::Widget {
        // Create the main toolbar view (provides header bar and content area)
        let toolbar_view = adw::ToolbarView::new();

        // Create header bar
        let header_bar = adw::HeaderBar::new();
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
        main_box.append(&Self::build_network_drives_section());
        main_box.append(&Self::build_status_section());

        // Load drives after UI is built
        Self::load_drives(&drives_section);

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

        // Add unmount button if safe
        if drive.is_safe_to_unmount {
            let unmount_btn = gtk::Button::builder()
                .icon_name("media-eject-symbolic")
                .tooltip_text("Unmount drive")
                .valign(gtk::Align::Center)
                .build();
            unmount_btn.add_css_class("flat");

            let device = drive.device.clone();
            let mount_point = drive.mount_point.clone();
            unmount_btn.connect_clicked(move |_| {
                tracing::info!("Unmount requested for: {}", device);
                // TODO: Implement unmount action
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
        let group = adw::PreferencesGroup::builder()
            .title("Network Drives")
            .description("Available network shares (SMB, NFS)")
            .margin_top(12)
            .margin_bottom(12)
            .margin_start(12)
            .margin_end(12)
            .build();

        // Placeholder row - will be replaced with detected shares
        let placeholder_row = adw::ActionRow::builder()
            .title("No network shares detected")
            .subtitle("SMB and NFS shares will appear here")
            .build();

        // Add network icon
        let icon = gtk::Image::from_icon_name("network-server-symbolic");
        placeholder_row.add_prefix(&icon);

        // Add scan button as suffix
        let scan_button = gtk::Button::builder()
            .icon_name("view-refresh-symbolic")
            .tooltip_text("Scan for network shares")
            .valign(gtk::Align::Center)
            .build();

        scan_button.add_css_class("flat");

        // Connect scan button click - will trigger network scanning
        scan_button.connect_clicked(|_| {
            tracing::info!("Scanning for network shares...");
            // TODO: Call network::smb::scan_smb_shares() and network::nfs::scan_nfs_shares()
        });

        placeholder_row.add_suffix(&scan_button);

        group.add(&placeholder_row);

        group.upcast()
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
