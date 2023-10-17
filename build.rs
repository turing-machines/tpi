// Copyright 2023 Turing Machines
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use std::fs::File;
use std::io::Write;
use std::{env, io};

fn main() -> io::Result<()> {
    generate_pgkbuild()
}

fn generate_pgkbuild() -> io::Result<()> {
    let pkg_name = format!("{}-{}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"));
    let mut pkgbuild = File::create(format!("{}/PKGBUILD", std::env::var("OUT_DIR").unwrap()))?;
    writeln!(pkgbuild, "# Maintainer: {}\n", env!("CARGO_PKG_AUTHORS"))?;
    writeln!(pkgbuild, "pkgname={}", env!("CARGO_PKG_NAME"))?;
    writeln!(pkgbuild, "pkgver={}", env!("CARGO_PKG_VERSION"))?;
    writeln!(pkgbuild, "pkgrel=1")?;
    writeln!(pkgbuild, "pkgdesc='{}'", env!("CARGO_PKG_DESCRIPTION"))?;
    writeln!(pkgbuild, "url={}", env!("CARGO_PKG_HOMEPAGE"))?;
    writeln!(pkgbuild, "license=('Apache')")?;
    writeln!(pkgbuild, "makedepends=('cargo')")?;
    writeln!(pkgbuild, "arch=('any')")?;
    writeln!(
        pkgbuild,
        r"source=('{}/archive/refs/tags/v{}.tar.gz')",
        env!("CARGO_PKG_REPOSITORY"),
        env!("CARGO_PKG_VERSION")
    )?;
    writeln!(
        pkgbuild,
        r#"prepare() {{
    cd $srcdir/{}
    export RUSTUP_TOOLCHAIN=stable
    cargo fetch --locked --target "$CARCH-unknown-linux-gnu"
}}
"#,
        pkg_name
    )?;
    writeln!(
        pkgbuild,
        r#"build() {{
    export RUSTUP_TOOLCHAIN=stable
    export CARGO_TARGET_DIR=target
    cd $srcdir/{} && cargo build --frozen --release 
}}"#,
        pkg_name
    )?;
    writeln!(
        pkgbuild,
        r#"check() {{
    export RUSTUP_TOOLCHAIN=stable
    cd $srcdir/{} && cargo test --frozen 
}}"#,
        pkg_name
    )?;
    writeln!(
        pkgbuild,
        r#"package() {{
    install -Dm0755 -t "$pkgdir/usr/bin/" "$srcdir/{}/target/release/{}"
}}"#,
        pkg_name,
        env!("CARGO_PKG_NAME")
    )
}
