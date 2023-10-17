# Copyright 2023 Turing Machines
#
# Licensed under the Apache License, Version 2.0 (the "License");
# you may not use this file except in compliance with the License.
# You may obtain a copy of the License at
#
#     http://www.apache.org/licenses/LICENSE-2.0
#
# Unless required by applicable law or agreed to in writing, software
# distributed under the License is distributed on an "AS IS" BASIS,
# WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
# See the License for the specific language governing permissions and
# limitations under the License.

#!/bin/bash
set -x

packager=$1 

package_win() {
    # no packager yet
    arch=echo $1|cut -d "-" -f 1
    mkdir -p target/win/$arch
    mv target/$1/release/tpi.exe target/win/$arch/
}

package_macos() {
    # no packager yet
    arch=echo $1|cut -d "-" -f 1
    mkdir -p target/apple/$arch
    mv target/$1/release/tpi target/apple/$arch/
}

package_deb() {
    cargo install cargo-deb
    cargo-deb --target $1 --no-build --no-strip
    mkdir -p target/debian
    mv target/$1/debian/*.deb target/debian/
}

package_arch() { 
    mkdir -p target/arch
    # PKGBUILD is build using build-script of the tpi crate
    cp target/$1/release/build/*/out/PKGBUILD target/arch/
}

for target in $(ls target| grep linux)
do
    target_name=$(echo $target | cut -d "/" -f 2)
    case $packager in
        "debian")
            package_deb $target_name
            ;;
        "arch")
            package_arch $target_name
            ;;
    esac
done

for target in $(ls target| grep windows)
do
    target_name=$(echo $target | cut -d "/" -f 2)
    case $packager in
        "windows")
            package_win $target_name
            ;;
    esac
done

for target in $(ls target| grep apple)
do
    target_name=$(echo $target | cut -d "/" -f 2)
    case $packager in
        "macos")
            package_macos $target_name
            ;;
    esac
done

