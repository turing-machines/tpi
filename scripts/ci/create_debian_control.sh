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
toml_file=${1}
arch=${2}
output_dir=${3:-$(pwd)}

if [ -z "$2" ]; then
    echo "missing architecture argument" >&2
    exit -1
fi

if [ ! -f "${toml_file}" ]; then
    echo "provided toml file: ${toml_file}, does not exist" >&2
    exit -1
fi

PACKAGE_NAME=$(grep '^name =' ${toml_file} | sed 's/name = "\(.*\)"/\1/')
VERSION=$(grep '^version =' ${toml_file} | sed 's/version = "\(.*\)"/\1/')
MAINTAINER=$(grep '^authors =' ${toml_file} | sed 's/authors = \[\s*"\(.*\)\s*"\]/\1/')
DESCRIPTION=$(grep '^description =' ${toml_file} | sed 's/description = "\(.*\)"/\1/')

mkdir -p ${output_dir}/DEBIAN/
cat <<EOL > "${output_dir}/DEBIAN/control"
Package: $PACKAGE_NAME
Version: $VERSION
Section: base
Priority: optional
Architecture: ${arch}
Maintainer: $MAINTAINER
Description: $DESCRIPTION
EOL

