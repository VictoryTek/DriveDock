self:
{ config, lib, pkgs, ... }:

let
  cfg = config.programs.drivedock;
  package = self.packages.${pkgs.system}.default;
in
{
  options.programs.drivedock = {
    enable = lib.mkEnableOption "DriveDock, a GTK4/libadwaita drive and network share manager";
  };

  config = lib.mkIf cfg.enable {
    environment.systemPackages = [ package ];

    # DriveDock's local drive mount/unmount and fstab persistence goes through
    # UDisks2 D-Bus (Polkit-gated); its network share *discovery* goes through GVfs.
    # Both are hard runtime dependencies of the design (see the Phase 1 spec's
    # risk 1) - not optional extras.
    services.udisks2.enable = true;
    services.gvfs.enable = true;

    # Network share mounting now goes through drivedock-mount-helper (a privileged
    # helper invoked via `pkexec`, gated by the Polkit action installed at
    # $out/share/polkit-1/actions/org.example.DriveDock.mount-helper.policy - see
    # nix/package.nix) rather than GVfs. That helper execs mount.cifs/mount.nfs and
    # relies on the kernel's cifs/nfs filesystem support, which NixOS does not
    # enable by default in minimal configs - see nixpkgs'
    # nixos/modules/tasks/filesystems/cifs.nix.
    boot.supportedFilesystems.cifs = lib.mkDefault true;
    boot.supportedFilesystems.nfs = lib.mkDefault true;

    # Already true on most desktop NixOS configs transitively via
    # services.udisks2.enable, but made explicit here since network-share mounting
    # now depends on Polkit directly for its own action, not only transitively
    # through UDisks2.
    security.polkit.enable = lib.mkDefault true;
  };
}
