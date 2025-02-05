# `tpi` command-line tool

This is a tool to control your Turing Pi board. It can be used from within the
BMC (for example, via SSH) or outside, like your personal computer. In the
latter case, it will try to connect to your board at the default hostname of
`turingpi.local` unless `--host` is specified. For more information and usage,
see `--help`.

## Installation

`tpi` binaries can be downloaded from various channels depending on
which OS you are running. Choose one of the following:

### pre build 

prebuild binaries can be downloaded from the following URL:
https://firmware.turingpi.com/tpi

We have binaries for Windows, Mac and Linux

### Debian

Add turing-machines debian repository to your sources.list once:

```shell
deb https://firmware.turingpi.com/tpi/debian ./ >> /etc/apt/sources.list
sudo apt-get update
```

You can now install the tpi package with command:

```shell
sudo apt-get install tpi
```

### Cargo

Rust users can install from source using the following command:

```shell
cargo install tpi
```

### Arch User Repository (AUR)

The package is available in the [AUR](https://aur.archlinux.org/packages/tpi).
Use your favourite AUR helper to install:

e.g. with yay:

```shell
yay -S tpi
```

### NixOS

The package is available in [Nixpkgs](https://search.nixos.org/packages?show=tpi). To try it out, you can run `tpi` in a nix-shell:

```
nix-shell -p tpi
```

### getting Master

The GitHub action runner builds Windows, Linux and macOS packages on every
commit to `master`. Take a look at the artifacts of any [workflow
runs](https://github.com/turing-machines/tpi/actions/workflows/build.yml) to
download them.

