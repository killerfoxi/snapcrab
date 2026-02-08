{ pkgs, ... }:

{
  packages = [
    pkgs.cargo-edit
    # The modern way to cross-compile for Windows (MSVC)
    pkgs.cargo-xwin
    
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
}
