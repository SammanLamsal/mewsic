{
  description = "Minimal MPD frontend";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs =
    { nixpkgs, ... }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
      ];
      forEachSystem = nixpkgs.lib.genAttrs systems;
    in
    {
      packages = forEachSystem (system: {
        default = nixpkgs.legacyPackages.${system}.callPackage ./package.nix { };
      });
    };
}
