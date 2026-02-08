{ pkgs, ... }:

{
  packages = [
    pkgs.cargo-edit
    pkgs.cargo-xwin
    pkgs.imagemagick
    pkgs.icoutils
    # Provides llvm-rc
    pkgs.llvm
    # Provides lld
    pkgs.lld
    
    # GUI dependencies for the Linux host
    pkgs.libxkbcommon
    pkgs.libGL
    pkgs.wayland
    pkgs.xorg.libX11
    pkgs.xorg.libXcursor
    pkgs.xorg.libXi
    pkgs.xorg.libXrandr
  ];

  languages.rust = {
    enable = true;
    channel = "stable";
    # We target MSVC now for better compatibility
    targets = [ "x86_64-pc-windows-msvc" ];
  };

  env = {
    # Ensure Linux host can find GUI libs for build scripts
    LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath [
      pkgs.libxkbcommon
      pkgs.libGL
      pkgs.wayland
    ];
  };

  scripts.gen-icon.exec = "magick assets/snapcrab.png -define icon:auto-resize=16,32,48,64,256 assets/snapcrab.ico";
  scripts.verify-exe.exec = "wrestool -l target/x86_64-pc-windows-msvc/release/snapcrab.exe";
}
