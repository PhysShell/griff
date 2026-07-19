# cockpit web-test — running it locally

The `cockpit-web-test` suite builds the cockpit to wasm, serves it, and drives
the **real** cockpit in headless Chromium (Playwright). CI runs it on every PR;
the repo-root `flake.nix` makes the *exact same* run reproducible locally, so
browser acceptance is a normal local check rather than a cloud-only oracle.

## Run it

From the repository root, with a flakes-enabled Nix:

```sh
nix develop                 # the toolchain shell (see "What the shell provides")
./cockpit/build-web.sh      # cargo build wasm + version-matched wasm-bindgen → cockpit/dist
cd cockpit/web-test
npm ci
npm test                    # Playwright + Chromium against cockpit/dist
```

The flake does **not** replace those commands or duplicate any version — it only
provides an environment in which the existing scripts work unchanged.

## What the shell provides

- **Rust via `rustup`** — the toolchain and components come from the repo's
  `rust-toolchain.toml`; `build-web.sh` adds the `wasm32-unknown-unknown` target.
- **`wasm-bindgen-cli`** — the shell hook installs *exactly* the version pinned
  in `cockpit/Cargo.toml` (read from there, not re-pinned here); `build-web.sh`
  still enforces the version match itself.
- **Node 22** and the **Playwright browser** — `nixpkgs` is pinned to
  `nixos-25.11`, whose `playwright-driver` is **1.56.1**, the exact version in
  `cockpit/web-test/package.json`. So the Nix-provided Chromium revision matches
  what the npm Playwright expects, and the downloaded (non-NixOS-runnable) browser
  is skipped (`PLAYWRIGHT_BROWSERS_PATH`).
- **A target-scoped wasm C toolchain.** The C in the Guitar Pro reader's `zstd`
  subtree must be cross-compiled to wasm. The standard Nix cc-wrapper forces the
  *host* target, which silently produces x86 objects and an undefined-`ZSTD_*`
  link failure. The fix is an *unwrapped* `clang` driven only for the wasm
  target:

  ```
  CC_wasm32_unknown_unknown     = clang
  AR_wasm32_unknown_unknown     = llvm-ar
  CFLAGS_wasm32_unknown_unknown = --target=wasm32-unknown-unknown
  ```

  These are **target-scoped** (`*_wasm32_unknown_unknown`), so a plain native
  `cargo test` / `cargo clippy` in the same shell uses the host `gcc` with no
  wasm flags — the host build is untouched.

## Notes

- CI is **not** on Nix; it keeps its own working setup. The flake is a local-dev
  convenience, not a CI rewrite.
- `cockpit/dist/`, `node_modules/`, and `target/` are gitignored build outputs.
- Without Nix, replicate the shell by hand: a matching Rust toolchain, the pinned
  `wasm-bindgen-cli`, Node 22, a Playwright-1.56 Chromium, and a wasm-target C
  compiler.
