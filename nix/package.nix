{ lib
, rustPlatform
, pkg-config
, wrapGAppsHook4
, gtk4
, libadwaita
, glib
, gvfs
}:

rustPlatform.buildRustPackage {
  pname = "drivedock";
  version = "0.1.0";

  src = lib.cleanSource ../.;

  cargoLock = {
    lockFile = ../Cargo.lock;
  };

  nativeBuildInputs = [
    pkg-config
    # GTK4-specific wrap hook (not the GTK3 `wrapGAppsHook`) - without it, GSettings
    # schemas, icon themes, and GVfs's GIO modules may not resolve correctly at
    # runtime for the packaged binary.
    wrapGAppsHook4
  ];

  buildInputs = [
    gtk4
    libadwaita
    glib
  ];

  # gvfs provides the GIO modules (gvfsd, gvfsd-network, gvfsd-smb, gvfsd-nfs) that
  # DriveDock's network share discovery depends on at runtime.
  propagatedBuildInputs = [
    gvfs
  ];

  meta = with lib; {
    description = "A modern Linux drive and network share manager with GTK4";
    homepage = "https://github.com/yourusername/DriveDock";
    license = licenses.gpl3Only;
    mainProgram = "drivedock";
    platforms = platforms.linux;
  };
}
