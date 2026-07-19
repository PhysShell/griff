{
  description = "griff cockpit web-test dev shell — full local wasm build + Playwright (3C-B0)";

  # Pinned to nixos-25.11: its `playwright-driver` is 1.56.1 — the exact version
  # cockpit/web-test/package.json pins — so the Nix-provided Chromium revision
  # matches what the npm Playwright expects. Its clang cross-compiles wasm too,
  # so one input covers the whole flow.
  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-25.11";

  outputs =
    { self, nixpkgs }:
    let
      system = "x86_64-linux";
      pkgs = import nixpkgs { inherit system; };
      # Unwrapped clang for the wasm C cross-compile: the standard Nix cc-wrapper
      # forces the host target, which silently compiled zstd's C to x86 objects
      # (undefined ZSTD_* at the wasm link). The wasm target must get a plain
      # `clang --target=wasm32-unknown-unknown`, and only that target.
      wasmClang = pkgs.llvmPackages.clang-unwrapped;
    in
    {
      devShells.${system}.default = pkgs.mkShell {
        packages = [
          pkgs.rustup # the repo's rust-toolchain.toml pins stable + rustfmt/clippy
          pkgs.nodejs_22 # match CI
          pkgs.clang # host cc for native cargo builds inside the shell
          pkgs.lld
          pkgs.llvm # llvm-ar for the wasm archive
          pkgs.pkg-config
          pkgs.cacert # cargo install / npm fetch over TLS
          pkgs.git
          wasmClang
        ];

        # Target-scoped so host (x86_64) builds are untouched — cc-rs reads the
        # `*_wasm32_unknown_unknown` triples only when building for that target.
        CC_wasm32_unknown_unknown = "${wasmClang}/bin/clang";
        AR_wasm32_unknown_unknown = "${pkgs.llvm}/bin/llvm-ar";
        CFLAGS_wasm32_unknown_unknown = "--target=wasm32-unknown-unknown";

        # Playwright runs the Nix-provided browser instead of downloading a
        # dynamically-linked one that will not run on NixOS.
        PLAYWRIGHT_BROWSERS_PATH = "${pkgs.playwright-driver.browsers}";
        PLAYWRIGHT_SKIP_VALIDATE_HOST_REQUIREMENTS = "true";
        PLAYWRIGHT_SKIP_BROWSER_DOWNLOAD = "1";

        shellHook = ''
          # rustup/cargo installs land here; put it ahead of the Nix rustup shim.
          export PATH="$HOME/.cargo/bin:$PATH"
          # wasm-bindgen-cli must match the crate version build-web.sh checks. We
          # do not pin it in the flake (that would duplicate cockpit/Cargo.toml);
          # we install exactly what the repo asks for, once.
          want=$(grep -m1 -E '^wasm-bindgen[[:space:]]*=' cockpit/Cargo.toml | grep -oE '[0-9]+\.[0-9]+\.[0-9]+' || true)
          have=$(wasm-bindgen --version 2>/dev/null | grep -oE '[0-9]+\.[0-9]+\.[0-9]+' | head -n1 || true)
          if [ -n "$want" ] && [ "$have" != "$want" ]; then
            echo "flake: installing wasm-bindgen-cli $want (matching cockpit/Cargo.toml)…"
            cargo install wasm-bindgen-cli --version "$want" --locked
          fi
        '';
      };
    };
}
