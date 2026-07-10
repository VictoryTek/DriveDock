mod dock;
mod network;
mod system;
mod udisks;
mod ui;

use gtk::prelude::*;
use libadwaita as adw;

const APP_ID: &str = "org.example.DriveDock";

fn main() -> glib::ExitCode {
    // Initialize tracing for logging
    tracing_subscriber::fmt::init();

    // Headless mode: invoked by the generated `systemd --user` unit
    // (see `dock::shares::write_systemd_unit`) to re-mount persistent network
    // shares at login without opening a window.
    if std::env::args().any(|arg| arg == "--remount-shares") {
        tracing::info!("Running headless re-mount of persistent network shares");
        let context = glib::MainContext::default();
        context.block_on(dock::shares::remount_persistent_shares());
        return glib::ExitCode::SUCCESS;
    }

    tracing::info!("Starting DriveDock application");

    // Create a new libadwaita application
    // Using libadwaita::Application ensures proper integration with GNOME
    let app = adw::Application::builder()
        .application_id(APP_ID)
        .build();

    // Connect to the activate signal which is emitted when the application is launched
    app.connect_activate(build_ui);

    // Run the application
    app.run()
}

/// Build the main application UI
/// This function is called when the application is activated
fn build_ui(app: &adw::Application) {
    // Create the main application window using our custom window module
    let window = ui::window::Window::new(app);

    // Present the window to the user
    window.present();

    tracing::info!("DriveDock window presented");
}
