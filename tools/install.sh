#!/usr/bin/env bash
# `make install`: install the tools Qorebit needs, then `make setup` does the project itself.
#
# Installs what is missing and needs no root, with each tool's official installer:
#   Rust (rustup), Node.js 22 (nvm).
# For tools that need root (git, curl, Python 3 + venv, the Dabao's USB rule) it prints the command
# for your system instead of running sudo itself. Re-running it is safe: present tools are
# left alone. There is no Docker or Foundry here: the devnet is one QoreChain node binary.
set -euo pipefail

say() { printf '\033[1m%s\033[0m\n' "$*"; }
have() { command -v "$1" >/dev/null 2>&1; }
os=$(uname)
todo=()

for t in git curl; do
    have "$t" || { echo "$t is required first"; exit 1; }
done

# ---- Rust
if have rustup || [ -x "$HOME/.cargo/bin/rustup" ]; then
    echo "  ok       Rust"
else
    say "installing Rust (rustup)"
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --no-modify-path
fi
export PATH="$HOME/.cargo/bin:$PATH"

# ---- Node.js (22 or newer: the companion's tests run TypeScript directly)
node_ok() { have node && [ "$(node -p 'process.versions.node.split(".")[0]')" -ge 22 ]; }
if node_ok; then
    echo "  ok       Node.js $(node --version)"
else
    say "installing Node.js 22 (nvm)"
    [ -s "$HOME/.nvm/nvm.sh" ] || curl -so- https://raw.githubusercontent.com/nvm-sh/nvm/v0.40.1/install.sh | bash
    # shellcheck disable=SC1091
    . "$HOME/.nvm/nvm.sh"
    nvm install 22
fi

# ---- Python 3 with venv (the bridge and the device client)
if have python3 && python3 -c 'import venv, ensurepip' 2>/dev/null; then
    echo "  ok       Python $(python3 --version | cut -d' ' -f2)"
else
    if [ "$os" = Darwin ]; then todo+=("brew install python"); else todo+=("sudo apt install python3 python3-venv python3-pip   # or your distro's packages"); fi
fi

# ---- the local devnet runs the official QoreChain node release, which is built for x86-64 Linux
if [ "$os" != Linux ] || [ "$(uname -m)" != x86_64 ]; then
    echo "  note     the local devnet needs the Linux x86-64 qorechaind release; on this system use the testnet"
    echo "           (or run the devnet on a Linux machine and point the bridge at it: tools/networks.json)"
fi

# ---- the Dabao's USB HID device for the logged-in user, without root (Linux)
if [ "$os" = Linux ] && ! grep -qs '1d50.*6197' /etc/udev/rules.d/* /lib/udev/rules.d/*; then
    todo+=("echo 'SUBSYSTEM==\"hidraw\", ATTRS{idVendor}==\"1d50\", ATTRS{idProduct}==\"6197\", MODE=\"0660\", TAG+=\"uaccess\"' | sudo tee /etc/udev/rules.d/70-dabao.rules && sudo udevadm control --reload   # Dabao USB for your user")
fi

if [ ${#todo[@]} -gt 0 ]; then
    echo
    say "Still needed (these need root or a desktop installer, so run them yourself):"
    for t in "${todo[@]}"; do echo "  $t"; done
    echo
fi
echo "New tools went to ~/.cargo/bin and ~/.nvm: open a new terminal (or source your"
echo "shell profile) so they are on your PATH. make install carries on with the project setup."
