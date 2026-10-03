# Kobo

Kobo aspires to be an open-source Super Mario World ROM editor and build system. It is currently in development and supports rendering a level to a png.

The long-term model:
1. A GUI editor for SMW romhacks
2. Build from a (git-friendly) project of text files and assets, compiled by patching a separately supplied clean ROM
3. A set of command-line build tools for scripting builds/edits/renders/etc.

## What it can do today

- __Render any level to PNG__ - Few limitations, documented in [known gaps](docs/known-gaps.md)
- __Inspect a level__: header and pointers, sprite list, Map16 grid/definitions, etc.
- __Show a level's palette and Map16 tiles__

**It is not yet a working build system or level editor**

## Build and try it

Install [Rust](https://www.rust-lang.org/tools/install). The repository pins its compiler in `rust-toolchain.toml`; Cargo selects that toolchain automatically.

```sh
cargo build --release --workspace
cargo run --release -- rom info -r /path/to/smw.sfc
cargo run --release -- level png 105 /tmp/level-105.png -r /path/to/smw.sfc
```

Level numbers are hexadecimal. Replace the example output path on Windows. Run
`cargo run -- --help` or `cargo run -- level png --help` for available commands.

Supply your own ROM. ROMs, exported Nintendo assets, and emulator dumps must never be
committed or distributed. A copier header is accepted and stripped for reads
and identity checks. The vanilla reference is Super Mario World (USA), headerless SHA-1
`6b47bb75d16514b6a476aa0c73a683a2a4c18765`.

To omit `-r`, set `KOBO_SMW_ROM` to your vanilla ROM path, or create `kobo/config.toml`
in your platform's user configuration directory (`$XDG_CONFIG_HOME`, normally
`~/.config`, on Linux):

```toml
[roms]
smw = "/path/to/smw.sfc"
```

Other useful commands:

```sh
cargo run --release -- level png 105 level.png --no-player
cargo run --release -- level png 105 markers.png --markers
cargo run --release -- level png 105 level.png --max-instructions 500000000
cargo run -- rom info
cargo run -- level info 105
cargo run -- gfx list
cargo run -- gfx export /path/to/export-directory
cargo run -- palette png --level 105 palette.png
cargo run -- map16 png --level 105 map16.png
```

With `--level`, the palette and Map16 sheet commands show what the ROM's loader produced
for that level. Without it they use the vanilla tables. Rendering is a
static editor view, not full emulation: animation, HDMA, and some interactions differ.
See [known gaps](docs/known-gaps.md). A successful PNG can carry warnings. Inspect them
when assessing compatibility.

## Develop and validate

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Synthetic tests always run. ROM-backed tests skip when no ROM is configured, and
external oracle suites run only when selected, so a green default run **does not
imply that ROM compatibility was tested**. With a vanilla ROM configured,
`KOBO_REQUIRE_ROM=1 cargo test --workspace` fails instead of skipping. See
[testing](docs/testing.md) for the oracle tiers, mutation fuzzing, and known
exceptions. CI runs the Rust checks on Linux, Windows, and macOS.

## Other

Game and compatibility references:
[vanilla SMW](docs/smw.md), [Lunar Magic](docs/lunar-magic.md), [SA-1](docs/sa1.md).

Kobo is licensed under [MPL-2.0](LICENSE).
