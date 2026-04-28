#!/usr/bin/env bash

# Print an error message and exit with 1
unsupported_os () {
    echo "Detected OS ($1) is unsupported."
    echo "Please open an issue (PRs welcome ❤️) on:"
    echo "    https://github.com/starkware-libs/cairo-vm/issues"
    echo ""
    echo "NOTE: you can still try installing dependencies manually"
    echo "If your OS differs from the detected one, you can look \
for the installation script for your OS in the install-scripts folder."
    exit 1
}

# Print the detected OS
print_os() {
    echo "Detected OS: $1"
}

# Print a message and run the script
run_script() {
    echo "Running $1..."
    . $1
}

# Detect Linux distro
install_linux() {
    # taken from: https://unix.stackexchange.com/a/6348
    # tries different methods to detect the Linux distro
    if [ -f /etc/os-release ]; then
        # freedesktop.org and systemd
        . /etc/os-release
        OS=$NAME
        VER=$VERSION_ID
    elif type lsb_release >/dev/null 2>&1; then
        # linuxbase.org
        OS=$(lsb_release -si)
        VER=$(lsb_release -sr)
    elif [ -f /etc/lsb-release ]; then
        # For some versions of Debian/Ubuntu without lsb_release command
        . /etc/lsb-release
        OS=$DISTRIB_ID
        VER=$DISTRIB_RELEASE
    elif [ -f /etc/debian_version ]; then
        # Older Debian/Ubuntu/etc.
        OS=Debian
        VER=$(cat /etc/debian_version)
    elif [ -f /etc/SuSe-release ]; then
        # Older SuSE/etc.
        OS="Old SuSE"
    elif [ -f /etc/redhat-release ]; then
        # Older Red Hat, CentOS, etc.
        OS="Old RedHat"
    else
        # Fall back to uname, e.g. "Linux <version>", also works for BSD, etc.
        OS=$(uname -s)
        VER=$(uname -r)
    fi

    print_os $OS

    # NOTE: we don't use $VER for now, but this might change
    case "$OS" in
        Ubuntu*|Linux\ Mint*) run_script "install-scripts/install-ubuntu.sh" ;;
        Debian*)              run_script "install-scripts/install-debian.sh" ;;
        *)                    unsupported_os "linux: $OS" ;;
    esac

}

install_macos() {
    print_os "MacOS"
    run_script install-scripts/install-macos.sh 
}

case "$OSTYPE" in
  linux*)           install_linux ;;
  darwin*)          install_macos ;; 
  msys*|cygwin*)    unsupported_os "Windows" ;;
  solaris*)         unsupported_os "Solaris" ;;
  bsd*)             unsupported_os "BSD" ;;
  *)                unsupported_os "unknown: ${OSTYPE}" ;;
esac
