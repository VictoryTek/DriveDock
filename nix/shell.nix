{ mkShell
, pkg-config
, wrapGAppsHook4
, gtk4
, libadwaita
, glib
, gvfs
, rust-analyzer
, clippy
, rustfmt
, cargo
, rustc
, drivedock
}:

mkShell {
  inputsFrom = [ drivedock ];

  nativeBuildInputs = [
    pkg-config
    wrapGAppsHook4
  ];

  buildInputs = [
    gtk4
    libadwaita
    glib
    gvfs
  ];

  packages = [
    cargo
    rustc
    rust-analyzer
    clippy
    rustfmt
  ];
}
