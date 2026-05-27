# AI generated and untested because I don't know nix.

{
  description = "A fuzzy finder for the terminal, powered by nucleo";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    crane = {
      url = "github:ipetkov/crane";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { self, nixpkgs, flake-utils, rust-overlay, crane }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs {
          inherit system;
          overlays = [ (import rust-overlay) ];
        };

        inherit (pkgs) lib;

        # Use the latest stable Rust toolchain
        rustToolchain = pkgs.rust-bin.stable.latest.default.override {
          extensions = [ "rust-src" "rust-analyzer" ];
        };

        craneLib = (crane.mkLib pkgs).overrideToolchain rustToolchain;

        # Source filtering to include only relevant files
        # This helps avoid unnecessary rebuilds when non-source files change
        src = lib.cleanSourceWith {
          src = ./.;
          filter = path: type:
            (lib.hasSuffix ".rs" path) ||
            (lib.hasSuffix ".toml" path) ||
            (lib.hasSuffix ".lock" path) ||
            (lib.hasInfix "/assets/" path) ||
            (craneLib.filterCargoSources path type);
        };

        # Common arguments for craneLib functions
        commonArgs = {
          inherit src;
          strictDeps = true;

          # Runtime dependencies
          buildInputs = with pkgs; [
            # Add runtime dependencies here
          ] ++ lib.optionals stdenv.isDarwin [
            libiconv
            darwin.apple_sdk.frameworks.Security
            darwin.apple_sdk.frameworks.CoreFoundation
            darwin.apple_sdk.frameworks.AppKit
          ];

          # Build-time dependencies
          nativeBuildInputs = with pkgs; [
            pkg-config
          ];
        };

        # Build *just* the cargo dependencies
        cargoArtifacts = craneLib.buildDepsOnly commonArgs;

        # Build the main package
        matchmaker = craneLib.buildPackage (commonArgs // {
          inherit cargoArtifacts;
          pname = "matchmaker";
          cargoExtraArgs = "-p matchmaker-cli";
        });
      in
      {
        packages.default = matchmaker;
        packages.matchmaker = matchmaker;

        apps.default = flake-utils.lib.mkApp {
          drv = matchmaker;
          name = "mm";
        };

        devShells.default = craneLib.devShell {
          inherit (commonArgs) buildInputs nativeBuildInputs;
          packages = with pkgs; [
            just
          ];
        };

        # Integration with `nix flake check`
        checks = {
          inherit matchmaker;
          matchmaker-clippy = craneLib.cargoClippy (commonArgs // {
            inherit cargoArtifacts;
            cargoClippyExtraArgs = "--all-targets -- --deny warnings";
          });
          matchmaker-fmt = craneLib.cargoFmt {
            inherit src;
          };
        };
      });
}
