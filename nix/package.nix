{ lib
, rustPlatform
, pkg-config
, wrapGAppsHook4
, gtk4
, libadwaita
, glib
, gvfs
, cifs-utils
, nfs-utils
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
  # DriveDock's network share discovery depends on at runtime. cifs-utils/nfs-utils
  # provide mount.cifs/mount.nfs, which the privileged helper execs by absolute path
  # (see src/bin/drivedock-mount-helper.rs's fixed candidate-path resolution) to
  # perform the real kernel mount that "dock"/"permanently dock" now creates for
  # network shares.
  propagatedBuildInputs = [
    gvfs
    cifs-utils
    nfs-utils
  ];

  # `cargo`'s default buildRustPackage install phase installs every [[bin]] target
  # (both `drivedock` and `drivedock-mount-helper`) into $out/bin. The helper must
  # never live there - it's meant to be run only as root via `pkexec`, never
  # directly/unprivileged-usefully - so move it out to $out/libexec/drivedock/
  # before wrapGAppsHook4's fixupPhase gets a chance to wrap it (it has no GTK deps
  # to wrap, and wrapping is intentionally GUI-binary-only).
  #
  # The Polkit `.policy` template's `@HELPER_PATH@` placeholder must be substituted
  # with the helper's actual (build-time-known, since Nix store paths are
  # content-addressed/fixed before the build starts) `$out/libexec/...` path -
  # pkexec resolves the action's target binary via the
  # `org.freedesktop.policykit.exec.path` annotation, not any argv path the caller
  # passes. `substitute` (from nixpkgs' setup.sh, always available in a derivation's
  # build environment) is used here rather than `replaceVars`/`substituteAll`
  # because those produce a *separate* derivation evaluated before $out is known -
  # they can't reference this package's own output path. `substitute` runs inside
  # this derivation's own postInstall, where $out is already a concrete, known path.
  postInstall = ''
    mkdir -p $out/libexec/drivedock
    mv $out/bin/drivedock-mount-helper $out/libexec/drivedock/drivedock-mount-helper

    mkdir -p $out/share/polkit-1/actions
    substitute ${../data/polkit/org.example.DriveDock.mount-helper.policy.in} \
      $out/share/polkit-1/actions/org.example.DriveDock.mount-helper.policy \
      --replace '@HELPER_PATH@' "$out/libexec/drivedock/drivedock-mount-helper"
  '';

  meta = with lib; {
    description = "A modern Linux drive and network share manager with GTK4";
    homepage = "https://github.com/victorytek/DriveDock";
    license = licenses.gpl3Only;
    mainProgram = "drivedock";
    platforms = platforms.linux;
  };
}
