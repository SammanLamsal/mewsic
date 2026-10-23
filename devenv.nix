{ pkgs, lib, ... }:

let
  runtimeLibs = with pkgs; [
    wayland
    fontconfig
    libxkbcommon
    libGL
    vulkan-loader
    xorg.libX11
    xorg.libXcursor
    xorg.libXi
    xorg.libXrandr
  ];
in
{
  languages.rust.enable = true;

  packages = [
    pkgs.pkg-config
    pkgs.rsgain
  ];

  env.LD_LIBRARY_PATH = lib.makeLibraryPath runtimeLibs;
}
