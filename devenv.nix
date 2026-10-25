{ pkgs, lib, ... }:

let
  runtimeLibs = with pkgs; [
    wayland
    fontconfig
    libxkbcommon
    libGL
    vulkan-loader
    libx11
    libxcursor
    libxi
    libxrandr
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
