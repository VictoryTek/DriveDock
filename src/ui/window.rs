use gtk::prelude::*;
use gtk::glib;
use libadwaita as adw;
use adw::prelude::*;

use crate::dock;
use crate::network;
use crate::system;
use crate::udisks::Udisks;

/// Main application window
///
/// DriveDock's narrowed scope: list local drives + network shares, mount ("dock"),
/// unmount ("undock"), and a per-item "permanently dock" toggle. The window has two
/// sections:
/// 1. Drives - local drives and network shares in a single list, each row tagged
///    with a kind badge (Local / SMB / NFS / ...).
/// 2. Status - feedback from the most recent operation, including the NixOS
///    config-snippet guidance when relevant.
pub struct Window {
    pub window: adw::ApplicationWindow,
}

/// Handle to the Status section's message widgets, cloned into async closures so
/// any Dock/Undock/permanent-toggle action can report back to the user.
#[derive(Clone)]
struct StatusHandle {
    icon: gtk::Image,
    label: gtk::Label,
}

impl StatusHandle {
    fn set_ok(&self, message: &str) {
        self.icon.set_icon_name(Some("emblem-ok-symbolic"));
        self.label.set_label(message);
    }

    fn set_error(&self, message: &str) {
        self.icon.set_icon_name(Some("dialog-error-symbolic"));
        self.label.set_label(message);
    }
}

impl Window {
    /// Create a new DriveDock window
    pub fn new(app: &adw::Application) -> Self {
        let window = adw::ApplicationWindow::builder()
            .application(app)
            .title("DriveDock")
            .default_width(800)
            .default_height(600)
            .build();

        let content = Self::build_content();
        window.set_content(Some(&content));

        Self { window }
    }

    /// Build the main content layout: header bar + Drives section + Status section.
    fn build_content() -> gtk::Widget {
        let toolbar_view = adw::ToolbarView::new();

        let header_bar = adw::HeaderBar::new();
        let refresh_btn = gtk::Button::builder()
            .icon_name("view-refresh-symbolic")
            .tooltip_text("Refresh drives and shares")
            .build();
        header_bar.pack_end(&refresh_btn);
        toolbar_view.add_top_bar(&header_bar);

        let main_box = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(0)
            .build();

        let clamp = adw::Clamp::builder()
            .maximum_size(1000)
            .tightening_threshold(600)
            .child(&main_box)
            .build();

        let scrolled = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vscrollbar_policy(gtk::PolicyType::Automatic)
            .child(&clamp)
            .vexpand(true)
            .build();

        toolbar_view.set_content(Some(&scrolled));

        let (status_widget, status) = Self::build_status_section();

        let drives_group = Self::build_drives_section();
        main_box.append(&drives_group);
        main_box.append(&status_widget);

        // Fallback re-mount of persistent network shares (in case the systemd --user
        // unit hasn't run/isn't enabled) before the first drive listing.
        let drives_group_startup = drives_group.clone();
        let status_startup = status.clone();
        glib::spawn_future_local(async move {
            dock::shares::remount_persistent_shares().await;
            Self::refresh_drives(&drives_group_startup, &status_startup).await;
        });

        let drives_group_clone = drives_group.clone();
        let status_clone = status.clone();
        refresh_btn.connect_clicked(move |btn| {
            btn.set_sensitive(false);
            let drives_group = drives_group_clone.clone();
            let status = status_clone.clone();
            let btn_clone = btn.clone();
            glib::spawn_future_local(async move {
                Self::refresh_drives(&drives_group, &status).await;
                btn_clone.set_sensitive(true);
            });
        });

        toolbar_view.upcast()
    }

    /// Build the (initially empty) Drives section.
    fn build_drives_section() -> adw::PreferencesGroup {
        adw::PreferencesGroup::builder()
            .title("Drives")
            .description("Local drives and network shares")
            .margin_top(24)
            .margin_bottom(12)
            .margin_start(12)
            .margin_end(12)
            .build()
    }

    /// Build the Status section: a single feedback row updated in place after every
    /// operation (mount, unmount, permanent-dock toggle, etc).
    fn build_status_section() -> (gtk::Widget, StatusHandle) {
        let group = adw::PreferencesGroup::builder()
            .title("Status")
            .description("Operation messages and feedback")
            .margin_top(12)
            .margin_bottom(24)
            .margin_start(12)
            .margin_end(12)
            .build();

        let status_box = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(8)
            .margin_top(8)
            .margin_bottom(8)
            .margin_start(8)
            .margin_end(8)
            .build();

        let icon = gtk::Image::from_icon_name("emblem-ok-symbolic");
        icon.set_valign(gtk::Align::Start);

        let label = gtk::Label::builder()
            .label("Ready \u{2014} DriveDock is ready to manage your drives and shares")
            .wrap(true)
            .xalign(0.0)
            .selectable(true)
            .hexpand(true)
            .build();

        status_box.append(&icon);
        status_box.append(&label);
        group.add(&status_box);

        (group.upcast(), StatusHandle { icon, label })
    }

    /// Clear and repopulate the Drives section from both local and network discovery.
    async fn refresh_drives(group: &adw::PreferencesGroup, status: &StatusHandle) {
        let mut child = group.first_child();
        let mut rows_to_remove = Vec::new();
        while let Some(row) = child {
            let next = row.next_sibling();
            rows_to_remove.push(row);
            child = next;
        }
        for row in rows_to_remove {
            group.remove(&row);
        }

        let mut any = false;

        match system::list_local_drives().await {
            Ok(drives) => {
                for drive in drives {
                    group.add(&Self::create_local_drive_row(&drive, group.clone(), status.clone()));
                    any = true;
                }
            }
            Err(e) => {
                tracing::error!("Failed to list local drives: {e}");
                status.set_error(&format!("Failed to list local drives: {e}"));
            }
        }

        match network::scan_network_shares().await {
            Ok(shares) => {
                for share in shares {
                    group.add(&Self::create_network_share_row(&share, group.clone(), status.clone()));
                    any = true;
                }
            }
            Err(e) => {
                tracing::error!("Failed to scan network shares: {e}");
                status.set_error(&format!("Failed to scan network shares: {e}"));
            }
        }

        if !any {
            let placeholder = adw::ActionRow::builder()
                .title("No drives or shares found")
                .subtitle("Connect a drive or scan again once a network share is available")
                .build();
            let icon = gtk::Image::from_icon_name("drive-harddisk-symbolic");
            placeholder.add_prefix(&icon);
            group.add(&placeholder);
        }
    }

    /// Small "kind" badge shown as a row suffix (e.g. "Local", "SMB", "NFS").
    fn kind_badge(text: &str) -> gtk::Label {
        gtk::Label::builder()
            .label(text)
            .css_classes(vec!["caption".to_string(), "dim-label".to_string()])
            .valign(gtk::Align::Center)
            .build()
    }

    /// Create a row for a local drive/partition.
    fn create_local_drive_row(
        drive: &system::MountedDrive,
        group: adw::PreferencesGroup,
        status: StatusHandle,
    ) -> adw::ActionRow {
        let subtitle = if let Some(mount_point) = &drive.mount_point {
            format!("{} \u{2022} {} \u{2022} {}", drive.device, mount_point.display(), drive.fs_type)
        } else {
            format!("{} \u{2022} Not mounted", drive.device)
        };

        let row = adw::ActionRow::builder()
            .title(&drive.name)
            .subtitle(&subtitle)
            .build();

        let icon = gtk::Image::from_icon_name(Self::icon_for_drive(drive));
        row.add_prefix(&icon);
        row.add_suffix(&Self::kind_badge("Local"));

        // Storage usage visualization, when mounted and stats are available.
        if let (Some(total), Some(used)) = (drive.total_size, drive.used_space) {
            let storage_box = gtk::Box::builder()
                .orientation(gtk::Orientation::Vertical)
                .spacing(4)
                .valign(gtk::Align::Center)
                .build();

            let used_fraction = if total > 0 { used as f64 / total as f64 } else { 0.0 };
            let level_bar = gtk::LevelBar::builder()
                .min_value(0.0)
                .max_value(1.0)
                .value(used_fraction)
                .width_request(120)
                .build();
            level_bar.add_offset_value(gtk::LEVEL_BAR_OFFSET_LOW, 0.75);
            level_bar.add_offset_value(gtk::LEVEL_BAR_OFFSET_HIGH, 0.90);
            level_bar.add_offset_value(gtk::LEVEL_BAR_OFFSET_FULL, 1.0);

            let available = total.saturating_sub(used);
            let size_label = gtk::Label::builder()
                .label(format!(
                    "{} available of {}",
                    system::MountedDrive::format_size(available),
                    system::MountedDrive::format_size(total)
                ))
                .css_classes(vec!["caption".to_string(), "dim-label".to_string()])
                .xalign(0.0)
                .build();

            storage_box.append(&level_bar);
            storage_box.append(&size_label);
            row.add_suffix(&storage_box);
        }

        // "Permanently dock" toggle - only actionable once mounted (a mount point
        // and filesystem type are required to write the fstab entry).
        let permanent_check = gtk::CheckButton::builder()
            .tooltip_text("Permanently dock (mount automatically at boot)")
            .valign(gtk::Align::Center)
            .build();
        permanent_check.set_sensitive(drive.is_mounted);

        {
            let device = drive.device.clone();
            let uuid = drive.uuid.clone();
            let mount_point = drive.mount_point.clone();
            let fs_type = drive.fs_type.clone();
            let status = status.clone();
            let check_clone = permanent_check.clone();

            permanent_check.connect_toggled(move |check| {
                let Some(mount_point) = mount_point.clone() else {
                    return;
                };
                let enable = check.is_active();
                let device = device.clone();
                let uuid = uuid.clone();
                let fs_type = fs_type.clone();
                let status = status.clone();
                let check_clone = check_clone.clone();
                check.set_sensitive(false);

                glib::spawn_future_local(async move {
                    let result = async {
                        let udisks = Udisks::new().await?;
                        let object = udisks
                            .find_block_object(&device, uuid.as_deref())
                            .await?;
                        dock::set_permanent_dock(
                            &udisks,
                            &object,
                            &mount_point.display().to_string(),
                            &fs_type,
                            "defaults",
                            enable,
                        )
                        .await
                    }
                    .await;

                    match result {
                        Ok(dock_result) => status.set_ok(&dock_result.message),
                        Err(e) => {
                            tracing::error!("Failed to toggle permanent dock: {e}");
                            status.set_error(&format!("Failed to update permanent dock: {e}"));
                            check_clone.set_active(!enable);
                        }
                    }
                    check_clone.set_sensitive(true);
                });
            });
        }
        row.add_suffix(&permanent_check);

        // Dock/Undock button.
        if drive.is_mounted {
            if drive.is_safe_to_unmount {
                let undock_btn = gtk::Button::builder()
                    .icon_name("media-eject-symbolic")
                    .tooltip_text("Undock (unmount) drive")
                    .valign(gtk::Align::Center)
                    .build();
                undock_btn.add_css_class("flat");

                let device = drive.device.clone();
                let uuid = drive.uuid.clone();
                let mount_point = drive.mount_point.clone().unwrap();
                let status = status.clone();
                let group = group.clone();

                undock_btn.connect_clicked(move |btn| {
                    btn.set_sensitive(false);
                    let device = device.clone();
                    let uuid = uuid.clone();
                    let mount_point = mount_point.clone();
                    let status = status.clone();
                    let group = group.clone();
                    let btn_clone = btn.clone();

                    glib::spawn_future_local(async move {
                        match system::unmount_drive(&mount_point, &device, uuid.as_deref()).await {
                            Ok(()) => {
                                status.set_ok(&format!("Undocked {}", mount_point.display()));
                                Self::refresh_drives(&group, &status).await;
                            }
                            Err(e) => {
                                tracing::error!("Failed to undock {}: {e}", mount_point.display());
                                status.set_error(&format!("Failed to undock drive: {e}"));
                                btn_clone.set_sensitive(true);
                            }
                        }
                    });
                });

                row.add_suffix(&undock_btn);
            } else {
                let lock_icon = gtk::Image::from_icon_name("changes-prevent-symbolic");
                lock_icon.set_tooltip_text(Some("System drive - cannot undock"));
                row.add_suffix(&lock_icon);
            }
        } else {
            let dock_btn = gtk::Button::builder()
                .icon_name("list-add-symbolic")
                .tooltip_text("Dock (mount) drive")
                .valign(gtk::Align::Center)
                .build();
            dock_btn.add_css_class("flat");

            let device = drive.device.clone();
            let uuid = drive.uuid.clone();
            let status = status.clone();
            let group = group.clone();

            dock_btn.connect_clicked(move |btn| {
                btn.set_sensitive(false);
                let device = device.clone();
                let uuid = uuid.clone();
                let status = status.clone();
                let group = group.clone();
                let btn_clone = btn.clone();

                glib::spawn_future_local(async move {
                    let result = async {
                        let udisks = Udisks::new().await?;
                        let object = udisks.find_block_object(&device, uuid.as_deref()).await?;
                        udisks.mount(&object).await
                    }
                    .await;

                    match result {
                        Ok(mount_path) => {
                            status.set_ok(&format!("Docked {device} at {mount_path}"));
                            Self::refresh_drives(&group, &status).await;
                        }
                        Err(e) => {
                            tracing::error!("Failed to dock {device}: {e}");
                            status.set_error(&format!("Failed to dock drive: {e}"));
                            btn_clone.set_sensitive(true);
                        }
                    }
                });
            });

            row.add_suffix(&dock_btn);
        }

        row
    }

    /// Select appropriate icon for drive type
    fn icon_for_drive(drive: &system::MountedDrive) -> &'static str {
        if drive.device.contains("nvme") {
            "drive-harddisk-solidstate-symbolic"
        } else if drive.device.contains("mmcblk") {
            "media-flash-sd-symbolic"
        } else if drive.device.contains("loop") {
            "media-optical-symbolic"
        } else {
            "drive-harddisk-symbolic"
        }
    }

    /// Create a row for a network share.
    fn create_network_share_row(
        share: &network::NetworkShare,
        group: adw::PreferencesGroup,
        status: StatusHandle,
    ) -> adw::ActionRow {
        let subtitle = share.uri.clone();

        let row = adw::ActionRow::builder()
            .title(&share.display_name)
            .subtitle(&subtitle)
            .build();

        let icon = gtk::Image::from_icon_name("folder-remote-symbolic");
        row.add_prefix(&icon);
        row.add_suffix(&Self::kind_badge(&share.protocol));

        // "Permanently dock" toggle - re-mount on login, per §7a (network shares
        // have no fstab entry; see `dock::shares`).
        let permanent_check = gtk::CheckButton::builder()
            .tooltip_text("Permanently dock (re-mount automatically at login)")
            .valign(gtk::Align::Center)
            .active(dock::shares::is_persistent(&share.uri))
            .build();

        {
            let uri = share.uri.clone();
            let status = status.clone();
            let check_clone = permanent_check.clone();

            permanent_check.connect_toggled(move |check| {
                let enable = check.is_active();
                let uri = uri.clone();
                let status = status.clone();
                let check_clone = check_clone.clone();
                check.set_sensitive(false);

                glib::spawn_future_local(async move {
                    let result = dock::shares::set_persistent(&uri, enable);
                    match result {
                        Ok(()) => {
                            if enable {
                                status.set_ok(&format!(
                                    "\"{uri}\" will re-mount at login. Run `systemctl --user enable --now drivedock-remount.service` ({}) to activate this.",
                                    dock::shares::systemd_unit_display_path()
                                ));
                            } else {
                                status.set_ok(&format!("\"{uri}\" will no longer re-mount at login."));
                            }
                        }
                        Err(e) => {
                            tracing::error!("Failed to update persistent share record: {e}");
                            status.set_error(&format!("Failed to update permanent dock: {e}"));
                            check_clone.set_active(!enable);
                        }
                    }
                    check_clone.set_sensitive(true);
                });
            });
        }
        row.add_suffix(&permanent_check);

        // Dock/Undock button.
        if share.mounted {
            let undock_btn = gtk::Button::builder()
                .icon_name("media-eject-symbolic")
                .tooltip_text("Undock (unmount) share")
                .valign(gtk::Align::Center)
                .build();
            undock_btn.add_css_class("flat");

            let uri = share.uri.clone();
            let status = status.clone();
            let group = group.clone();

            undock_btn.connect_clicked(move |btn| {
                btn.set_sensitive(false);
                let uri = uri.clone();
                let status = status.clone();
                let group = group.clone();
                let btn_clone = btn.clone();

                glib::spawn_future_local(async move {
                    match network::unmount_share(&uri).await {
                        Ok(()) => {
                            status.set_ok(&format!("Undocked {uri}"));
                            Self::refresh_drives(&group, &status).await;
                        }
                        Err(e) => {
                            tracing::error!("Failed to undock {uri}: {e}");
                            status.set_error(&format!("Failed to undock share: {e}"));
                            btn_clone.set_sensitive(true);
                        }
                    }
                });
            });

            row.add_suffix(&undock_btn);
        } else {
            let dock_btn = gtk::Button::builder()
                .icon_name("list-add-symbolic")
                .tooltip_text("Dock (mount) share")
                .valign(gtk::Align::Center)
                .build();
            dock_btn.add_css_class("flat");

            let uri = share.uri.clone();
            let status = status.clone();
            let group = group.clone();

            dock_btn.connect_clicked(move |btn| {
                btn.set_sensitive(false);
                let uri = uri.clone();
                let status = status.clone();
                let group = group.clone();
                let btn_clone = btn.clone();
                let parent_window = btn
                    .root()
                    .and_downcast_ref::<gtk::Window>()
                    .cloned();

                glib::spawn_future_local(async move {
                    match network::mount_share(&uri, parent_window.as_ref()).await {
                        Ok(()) => {
                            status.set_ok(&format!("Docked {uri}"));
                            Self::refresh_drives(&group, &status).await;
                        }
                        Err(e) => {
                            tracing::error!("Failed to dock {uri}: {e}");
                            status.set_error(&format!("Failed to dock share: {e}"));
                            btn_clone.set_sensitive(true);
                        }
                    }
                });
            });

            row.add_suffix(&dock_btn);
        }

        row
    }

    /// Present the window to the user
    pub fn present(&self) {
        self.window.present();
    }
}
