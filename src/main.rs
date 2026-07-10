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
