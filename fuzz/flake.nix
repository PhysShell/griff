{
  # Optional convenience dev shell for fuzzing. NOT required: CI installs the
  # nightly toolchain and cargo-fuzz via rustup directly. See docs/fuzzing.md
  # and ADR-0010. Provides `rustup` + `cargo-fuzz`; the nightly toolchain is
  # selected by fuzz/rust-toolchain.toml.
  description = "griff fuzzing dev shell (nightly + cargo-fuzz)";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs = { self, nixpkgs }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
        "x86_64-darwin"
        "aarch64-darwin"
      ];
      forAll = f: nixpkgs.lib.genAttrs systems
        (system: f nixpkgs.legacyPackages.${system});
    in
    {
      devShells = forAll (pkgs: {
        default = pkgs.mkShell {
          packages = [ pkgs.rustup pkgs.cargo-fuzz ];
          shellHook = ''
            rustup toolchain install nightly --profile minimal >/dev/null 2>&1 || true
            echo "griff fuzz shell — try: cargo +nightly fuzz run midi_import"
          '';
        };
      });
    };
}
