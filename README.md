# `tpi` command-line tool

This is a tool to control your Turing Pi board. It can be used from within the BMC (for example, via
SSH) or outside, like your personal computer. In the latter case, it will try to connect to your
board at the default hostname of `turingpi.local` unless `--host` is specified. For more information
and usage, see `--help`.

## Installation

### GitHub CI

The GitHub action runner builds Windows, Linux and macOS packages on every commit to `master`.
Take a look at the artifacts of any [workflow runs](https://github.com/turing-machines/tpi/actions/workflows/build.yml)
to download them.

### Arch User Repository (AUR)

The package is available in the [AUR](https://aur.archlinux.org/packages/tpi).

### Cargo

Install the latest version using the following cargo command:

```shell
cargo install --git https://github.com/turing-machines/tpi
```
