{
  description = "Investigation into NixOS build sizes";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

    # Nix language server used with VSCode.
    nil = {
      url = "github:oxalica/nil";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { nixpkgs, nil, ... }:
    let
      pkgs = nixpkgs.legacyPackages.x86_64-linux;
    in
    {
      devShells.x86_64-linux = {
        default = pkgs.mkShell {
          packages = with pkgs; [
            git
            cargo
            rustc
            rust-analyzer
            rustfmt
            # Both of these used with VSCode.
            nixpkgs-fmt
            nil.packages.${system}.default
          ];

          env = {
            RUST_BACKTRACE = "full";
          };
        };
      };

      packages.x86_64-linux = {
        default = pkgs.rustPlatform.buildRustPackage {
          pname = "nix-tree-sizes";
          version = "0.1.0";

          src = ./.;
          cargoHash = "sha256-9qTWAimy+GUVHqiPQ3jgvIUYUoOdVsWTQMbkPO8UfgM=";
        };
      };

      systems = {
        bare =
          let
            builtSystem = nixpkgs.lib.nixosSystem {
              system = "x86_64-linux";
              modules = [
                (nixpkgs.outPath + "/nixos/modules/profiles/minimal.nix")
                (nixpkgs.outPath + "/nixos/modules/profiles/headless.nix")
                (nixpkgs.outPath + "/nixos/modules/profiles/perlless.nix")
                ({ lib, pkgs, ... }: {
                  disabledModules = [ "security/wrappers/default.nix" ];

                  options.security = {
                    wrappers = lib.mkOption {
                      type = lib.types.attrs;
                      default = { };
                    };
                    wrapperDir = lib.mkOption {
                      type = lib.types.path;
                      default = "/run/wrappers/bin";
                    };
                  };

                  config = {
                    fileSystems."/".device = "/dev/sda1";
                    boot.loader.systemd-boot.enable = true;

                    nix.enable = false;
                    services.udev.enable = false;
                    services.lvm.enable = false;
                    security.sudo.enable = false;

                    nixpkgs.overlays = [
                      (
                        self: super: {
                          dbus = super.dbus.override {
                            systemdMinimal = self.systemd;
                          };
                          fuse3 = (self.lib.dontRecurseIntoAttrs (self.callPackage (nixpkgs.outPath + "/pkgs/os-specific/linux/fuse") { })).fuse_3;
                        }
                      )
                    ];
                  };
                })
              ];
            };
          in
          {
            inherit (builtSystem.config.system.build) toplevel;
            # This is here just for debugging. You can either load the flake on a nix repl and investigate values, or run something like the following:
            # `nix eval --json .#systems.normal-bare.options.environment.systemPackages.definitionsWithLocations`
            inherit (builtSystem) options config;
          };
      };
    };
}
