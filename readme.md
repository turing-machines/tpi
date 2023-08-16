# Tpi Command

Tool to control your Turing-Pi board. It will use a default host address to find
the board on the network (`turingpi.local`). Pass `-host <your-hostname/ip>` to
change the default host address. For more information on usage, execute the help
command: `tpi --help`

## Install
 
### GitHub CI

The `master` branch builds Windows, Linux, and Apple packages on every commit.
Take a look at the artifacts of a workflow run to download these packages :
https://github.com/turing-machines/tpi/actions/workflows/build.yml

### AUR

This package is available in the AUR repository.
[see](https://aur.archlinux.org/packages/tpi)


### Cargo

Install the latest master using the following cargo command.

``` shell 
cargo install --git https://github.com/turing-machines/tpi --branch
```
