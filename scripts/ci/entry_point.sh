#!/bin/bash
work_dir=/home/builder/gh-action

echo "::group::Copying files from /workspace to $work_dir"
mkdir -p $work_dir
cp -rfv /workspace/* $work_dir
cd $work_dir
echo "::endgroup::"

echo "::group::Updating PKGBUID"
updpkgsums
makepkg -g >> PKGBUILD
makepkg --printsrcinfo >.SRCINFO

sudo cp -f PKGBUILD /workspace
sudo cp -f .SRCINFO /workspace
echo "::endgroup::"
