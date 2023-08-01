use std::fs::File;
use std::io;
use std::io::Write;

fn main() -> io::Result<()> {
    let mut pkgbuild = File::create(format!("{}/PKGBUILD", std::env::var("OUT_DIR").unwrap()))?;
    writeln!(pkgbuild, "# Maintainer: {}\n", env!("CARGO_PKG_AUTHORS"))?;
    writeln!(pkgbuild, "pkgname={}-git", env!("CARGO_PKG_NAME"))?;
    writeln!(pkgbuild, "pkgver={}", env!("CARGO_PKG_VERSION"))?;
    writeln!(pkgbuild, "pkgrel=1")?;
    writeln!(pkgbuild, "pkgdesc='{}'", env!("CARGO_PKG_DESCRIPTION"))?;
    writeln!(pkgbuild, "url={}", env!("CARGO_PKG_HOMEPAGE"))?;
    writeln!(pkgbuild, "license=('Apache')")?;
    writeln!(pkgbuild, "makedepends=('cargo')")?;
    writeln!(pkgbuild, "arch=('any')")?;
    writeln!(
        pkgbuild,
        r"source=('{}-{}.tar.gz::{}/archive/refs/heads/master.tar.gz')",
        env!("CARGO_PKG_NAME"),
        env!("CARGO_PKG_VERSION"),
        env!("CARGO_PKG_REPOSITORY")
    )?;
    writeln!(
        pkgbuild,
        r"sha256sums=('899f038bcf3d2baa99d6e21116aed3636cf2f0c2907252dcbbc0b522a212d97a')",
    )?;
    writeln!(
        pkgbuild,
        r#"prepare() {{
    export RUSTUP_TOOLCHAIN=stable
    cargo fetch --locked --target "$CARCH-unknown-linux-gnu"
}}
"#
    )?;
    writeln!(
        pkgbuild,
        r#"build() {{
    export RUSTUP_TOOLCHAIN=stable
    export CARGO_TARGET_DIR=target
    cargo build --frozen --release --all-features
}}"#
    )?;
    writeln!(
        pkgbuild,
        r#"check() {{
    export RUSTUP_TOOLCHAIN=stable
    cargo test --frozen --all-features
}}"#
    )?;
    writeln!(
        pkgbuild,
        r#"package() {{
    install -Dm0755 -t "$pkgdir/usr/bin/" "target/release/{}"
}}"#,
        env!("CARGO_PKG_NAME")
    )?;
    Ok(())
}
