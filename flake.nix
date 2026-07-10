{
  description = "DriveDock - a modern Linux drive and network share manager (GTK4/libadwaita)";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
  };

  outputs = { self, nixpkgs, ... }:
    let
      forAllSystems = nixpkgs.lib.genAttrs [ "x86_64-linux" "aarch64-linux" ];
    in
    {
      packages = forAllSystems (system:
        let pkgs = nixpkgs.legacyPackages.${system};
        in {
          default = pkgs.callPackage ./nix/package.nix { };
          # Aliased name for clarity when this flake is consumed as an input.
          drivedock = self.packages.${system}.default;
        });

      nixosModules = {
        default = import ./nix/module.nix self;
        drivedock = self.nixosModules.default;
      };

      devShells = forAllSystems (system:
        let pkgs = nixpkgs.legacyPackages.${system};
        in {
          default = pkgs.callPackage ./nix/shell.nix {
            drivedock = self.packages.${system}.default;
          };
        });
    };
}
