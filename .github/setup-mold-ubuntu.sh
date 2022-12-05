#!/bin/sh

export DEBIAN_FRONTEND=noninteractive
dpkg --add-architecture $CROSS_DEB_ARCH
apt-get install -y wget clang clang:$CROSS_DEB_ARCH build-essential build-essential:$CROSS_DEB_ARCH libgtk-3-dev:$CROSS_DEB_ARCH libasound2-dev:$CROSS_DEB_ARCH libfontconfig1-dev:$CROSS_DEB_ARCH
ARCH=$(uname -m)
wget https://github.com/rui314/mold/releases/download/v1.7.1/mold-1.7.1-${ARCH}-linux.tar.gz
tar xzf mold-1.7.1-${ARCH}-linux.tar.gz
ls -lhFA mold-1.7.1-${ARCH}-linux
cp mold-1.7.1-${ARCH}-linux/bin/mold /usr/bin
