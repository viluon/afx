#!/bin/sh

DEBIAN_FRONTEND=noninteractive
sudo apt-get install -y wget build-essential libgtk-3-dev libasound2-dev libfontconfig1-dev
ARCH=$(uname -m)
wget https://github.com/rui314/mold/releases/download/v1.7.1/mold-1.7.1-${ARCH}-linux.tar.gz
tar xzf mold-1.7.1-${ARCH}-linux.tar.gz
ls -lhFA mold-1.7.1-${ARCH}-linux
sudo cp mold-1.7.1-${ARCH}-linux/bin/mold /usr/bin
