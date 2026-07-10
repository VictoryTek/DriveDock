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
    # UDisks2 D-Bus (Polkit-gated); its network share discovery/mount goes through
    # GVfs. Both are hard runtime dependencies of the design (see the Phase 1 spec's
    # risk 1) - not optional extras.
    services.udisks2.enable = true;
    services.gvfs.enable = true;
  };
}
