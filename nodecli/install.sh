#!/bin/bash

set -e

cargo build --release

mkdir -p ~/.local/bin
cp target/release/nodecli ~/.local/bin/nodecli
chmod +x ~/.local/bin/nodecli
echo "nodecli installed to ~/.local/bin/nodecli."
echo "Add ~/.local/bin to your PATH to use it. export PATH=\$PATH:\$HOME/.local/bin"
