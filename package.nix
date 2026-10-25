{
  lib,
  rustPlatform,
  makeWrapper,
  copyDesktopItems,
  makeDesktopItem,
  pkg-config,
  wayland,
  libxkbcommon,
  libGL,
  vulkan-loader,
  fontconfig,
  libx11,
  libxcursor,
  libxi,
  libxrandr,
  rsgain,
}:

let
  # winit and the GPU backends load these with dlopen, so they are not linked in.
  runtimeLibs = [
    wayland
    libxkbcommon
    libGL
    vulkan-loader
    fontconfig
    libx11
    libxcursor
    libxi
    libxrandr
  ];
in
rustPlatform.buildRustPackage {
  pname = "mewsic";
  version = "0.1.0";

  src = lib.fileset.toSource {
    root = ./.;
    fileset = lib.fileset.unions [
      ./Cargo.toml
      ./Cargo.lock
      ./src
      ./icon.png
    ];
  };
  cargoLock.lockFile = ./Cargo.lock;

  nativeBuildInputs = [
    pkg-config
    makeWrapper
    copyDesktopItems
  ];

  postInstall = ''
    install -Dm644 icon.png $out/share/icons/hicolor/512x512/apps/mewsic.png
  '';

  postFixup = ''
    wrapProgram $out/bin/mewsic \
      --prefix LD_LIBRARY_PATH : ${lib.makeLibraryPath runtimeLibs} \
      --prefix PATH : ${lib.makeBinPath [ rsgain ]}
  '';

  desktopItems = [
    (makeDesktopItem {
      name = "mewsic";
      desktopName = "mewsic";
      comment = "Minimal MPD client";
      exec = "mewsic";
      icon = "mewsic";
      categories = [
        "AudioVideo"
        "Audio"
        "Player"
      ];
      startupWMClass = "mewsic";
      actions = {
        toggle = {
          name = "Play / Pause";
          exec = "mewsic --toggle";
        };
        next = {
          name = "Next";
          exec = "mewsic --next";
        };
        previous = {
          name = "Previous";
          exec = "mewsic --previous";
        };
      };
    })
  ];

  meta = {
    description = "Minimal MPD frontend";
    mainProgram = "mewsic";
    platforms = lib.platforms.linux;
  };
}
