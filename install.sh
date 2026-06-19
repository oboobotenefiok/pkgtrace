#!/bin/bash
set -e

# pkgtrace installer for Termux
# One script to install everything

# Colors for better readability
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
CYAN='\033[0;36m'
BOLD='\033[1m'
NC='\033[0m' # No Color

# Log file for troubleshooting
LOG_FILE="/tmp/pkgtrace-install.log"
exec > >(tee -a "$LOG_FILE") 2>&1

echo -e "${BLUE}${BOLD}pkgtrace Installer${NC}"
echo -e "${BLUE}==================${NC}"
echo ""
echo -e "${CYAN}Log file: $LOG_FILE${NC}"
echo ""

# Determine installation directory
if [ -d "/data/data/com.termux/files/usr/bin" ]; then
    INSTALL_DIR="/data/data/com.termux/files/usr/bin"
    PREFIX="/data/data/com.termux/files/usr"
elif [ -d "$HOME/.local/bin" ]; then
    INSTALL_DIR="$HOME/.local/bin"
    PREFIX="$HOME/.local"
else
    INSTALL_DIR="/usr/local/bin"
    PREFIX="/usr/local"
fi

echo -e "${CYAN}Installation directory: $INSTALL_DIR${NC}"
echo ""

# Function to check if we're in the project directory
in_project_dir() {
    [ -f "Cargo.toml" ] && [ -f "src/main.rs" ]
}

# Function to clone the repository
clone_repo() {
    echo -e "${YELLOW}Cloning pkgtrace repository...${NC}"
    if ! command -v git &> /dev/null; then
        echo -e "${YELLOW}Git is not installed. Installing git...${NC}"
        pkg install git -y || apt install git -y || true
    fi

    git clone https://github.com/oboobotenefiok/pkgtrace
    cd pkgtrace
    echo ""
}

# Function to build from source
build_from_source() {
    echo -e "${YELLOW}Building pkgtrace from source...${NC}"
    echo -e "${CYAN}This may take 2-3 minutes depending on your device.${NC}"
    echo ""

    if cargo build --release; then
        echo ""
        echo -e "${GREEN}Build complete!${NC}"
    else
        echo -e "${RED}Error: Build failed.${NC}"
        echo -e "${YELLOW}Please check the log file at $LOG_FILE${NC}"
        exit 1
    fi

    if [ ! -f "target/release/pkgtrace" ]; then
        echo -e "${RED}Error: Build failed - binary not found.${NC}"
        exit 1
    fi
}

# Function to install the binary
install_binary() {
    mkdir -p "$INSTALL_DIR"
    cp target/release/pkgtrace "$INSTALL_DIR/"
    chmod 755 "$INSTALL_DIR/pkgtrace"

    echo -e "${GREEN}pkgtrace installed successfully!${NC}"
    echo ""
}

# Function to download pre-built binary
download_binary() {
    ARCH=$(uname -m)
    case "$ARCH" in
        aarch64)
            BINARY="pkgtrace-aarch64-linux-android"
            ;;
        armv7l)
            BINARY="pkgtrace-armv7-linux-androideabi"
            ;;
        i686)
            BINARY="pkgtrace-i686-linux-android"
            ;;
        x86_64)
            BINARY="pkgtrace-x86_64-linux-android"
            ;;
        *)
            echo -e "${YELLOW}Unsupported architecture: $ARCH${NC}"
            echo ""
            echo -e "${YELLOW}We don't have a pre-built binary for your device.${NC}"
            return 2
            ;;
    esac

    echo -e "${CYAN}Checking for latest release...${NC}"
    LATEST_VERSION=$(curl -s https://api.github.com/repos/oboobotenefiok/pkgtrace/releases/latest | grep tag_name | cut -d '"' -f 4)

    if [ -z "$LATEST_VERSION" ]; then
        echo -e "${YELLOW}Could not find latest release on GitHub.${NC}"
        echo -e "${CYAN}This could happen if:${NC}"
        echo "  - No releases have been published yet"
        echo "  - GitHub API is rate-limited"
        echo "  - You are offline"
        echo ""
        return 3
    fi

    DOWNLOAD_URL="https://github.com/oboobotenefiok/pkgtrace/releases/download/$LATEST_VERSION/$BINARY"
    echo -e "${CYAN}Downloading $BINARY version $LATEST_VERSION...${NC}"

    mkdir -p "$INSTALL_DIR"
    if curl -L -o "$INSTALL_DIR/pkgtrace" "$DOWNLOAD_URL"; then
        chmod 755 "$INSTALL_DIR/pkgtrace"
        echo ""
        echo -e "${GREEN}Binary downloaded and installed!${NC}"
        echo ""
        return 0
    else
        echo -e "${YELLOW}Download failed. Network issue or binary not found.${NC}"
        return 4
    fi
}

# Function to handle download failure when Rust is installed
handle_download_failure_with_rust() {
    local exit_code=$1
    local message=""

    case $exit_code in
        2)
            message="Architecture not supported for pre-built binaries."
            ;;
        3)
            message="No release found on GitHub."
            ;;
        4)
            message="Download failed."
            ;;
        *)
            message="Unknown error occurred."
            ;;
    esac

    echo ""
    echo -e "${YELLOW}$message${NC}"
    echo ""
    echo -e "${CYAN}Since Rust is already installed, you can build from source instead.${NC}"
    echo ""
    echo -e "${BOLD}Would you like to build from source?${NC}"
    echo "  [Y] Yes, build from source (recommended)"
    echo "  [N] Abort installation"
    echo ""
    echo -n "Choose (Y/n): "
    read -r retry_response

    if [[ "$retry_response" =~ ^[Yy]$ ]] || [[ -z "$retry_response" ]]; then
        echo ""
        echo -e "${CYAN}Building from source...${NC}"
        echo ""

        if ! in_project_dir; then
            clone_repo
        fi

        build_from_source
        install_binary
        return 0
    else
        echo -e "${RED}Installation aborted.${NC}"
        exit 1
    fi
}

# Function to handle download failure when Rust is NOT installed
handle_download_failure_without_rust() {
    local exit_code=$1
    local message=""

    case $exit_code in
        2)
            message="Architecture not supported for pre-built binaries."
            ;;
        3)
            message="No release found on GitHub."
            ;;
        4)
            message="Download failed."
            ;;
        *)
            message="Unknown error occurred."
            ;;
    esac

    echo ""
    echo -e "${YELLOW}$message${NC}"
    echo ""
    echo -e "${CYAN}Would you like to install Rust and build from source instead?${NC}"
    echo ""
    echo -e "${BOLD}Choose an option:${NC}"
    echo "  [Y] Yes, install Rust and build from source (recommended)"
    echo "  [N] Abort installation"
    echo ""
    echo -n "Choose (Y/n): "
    read -r retry_response

    if [[ "$retry_response" =~ ^[Yy]$ ]] || [[ -z "$retry_response" ]]; then
        echo ""
        echo -e "${CYAN}Installing Rust...${NC}"
        echo -e "${CYAN}This will download and run rustup, the official Rust installer.${NC}"
        echo ""

        curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y

        # Source cargo environment
        if [ -f "$HOME/.cargo/env" ]; then
            source "$HOME/.cargo/env"
        elif [ -f "$PREFIX/etc/profile" ]; then
            source "$PREFIX/etc/profile"
        else
            echo ""
            echo -e "${YELLOW}Rust installed but cargo is not in PATH.${NC}"
            echo -e "${CYAN}Please run: source \$HOME/.cargo/env${NC}"
            echo -e "${CYAN}Then run this installer again.${NC}"
            exit 1
        fi

        echo ""
        echo -e "${GREEN}Rust installed successfully!${NC}"
        echo ""

        if ! in_project_dir; then
            clone_repo
        fi

        build_from_source
        install_binary
        return 0
    else
        echo -e "${RED}Installation aborted.${NC}"
        exit 1
    fi
}

# Function to handle Rust installation and build flow
install_rust_and_build() {
    echo ""
    echo -e "${CYAN}Installing Rust...${NC}"
    echo -e "${CYAN}This will download and run rustup, the official Rust installer.${NC}"
    echo ""

    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y

    # Source cargo environment
    if [ -f "$HOME/.cargo/env" ]; then
        source "$HOME/.cargo/env"
    elif [ -f "$PREFIX/etc/profile" ]; then
        source "$PREFIX/etc/profile"
    else
        echo ""
        echo -e "${YELLOW}Rust installed but cargo is not in PATH.${NC}"
        echo -e "${CYAN}Please run: source \$HOME/.cargo/env${NC}"
        echo -e "${CYAN}Then run this installer again.${NC}"
        exit 1
    fi

    echo ""
    echo -e "${GREEN}Rust installed successfully!${NC}"
    echo ""

    if ! in_project_dir; then
        clone_repo
    fi

    build_from_source
    install_binary
}

# Function to check for existing installation
check_existing_install() {
    if [ -f "$INSTALL_DIR/pkgtrace" ]; then
        local version=$("$INSTALL_DIR/pkgtrace" --version 2>/dev/null | head -n1 || echo "unknown")
        echo -e "${YELLOW}pkgtrace is already installed.${NC}"
        echo -e "${CYAN}  Version: $version${NC}"
        echo ""
        echo -e "${BOLD}What would you like to do?${NC}"
        echo "  [1] Update to latest version (recommended)"
        echo "  [2] Reinstall (keep configuration)"
        echo "  [3] Uninstall first, then install fresh"
        echo "  [4] Exit (keep current version)"
        echo ""
        echo -n "Choose (1/2/3/4): "
        read -r upgrade_choice

        case "$upgrade_choice" in
            1|2)
                echo -e "${CYAN}Proceeding with installation...${NC}"
                echo ""
                return 0
                ;;
            3)
                echo -e "${CYAN}Uninstalling previous version...${NC}"
                rm -f "$INSTALL_DIR/pkgtrace"
                echo -e "${GREEN}Uninstalled.${NC}"
                echo ""
                return 0
                ;;
            4)
                echo -e "${CYAN}Keeping current version. Exiting.${NC}"
                exit 0
                ;;
            *)
                echo -e "${YELLOW}Invalid choice. Proceeding with update.${NC}"
                return 0
                ;;
        esac
    fi
    return 0
}

# Function to check network connectivity
check_network() {
    echo -e "${CYAN}Checking network connectivity...${NC}"
    if curl -s --head --connect-timeout 5 https://github.com > /dev/null; then
        echo -e "${GREEN}✓ GitHub reachable${NC}"
        return 0
    else
        echo -e "${YELLOW}✗ GitHub unreachable (you may be offline)${NC}"
        echo -e "${CYAN}Will build from source if Rust is available.${NC}"
        return 1
    fi
}

# Function to present simplified options
quick_or_custom() {
    echo -e "${BOLD}Quick install or customize?${NC}"
    echo "  [Q] Quick install (recommended) - No more questions"
    echo "  [C] Customize - Choose your installation method"
    echo ""
    echo -n "Choose (Q/c): "
    read -r install_mode

    if [[ "$install_mode" =~ ^[Qq]$ ]] || [[ -z "$install_mode" ]]; then
        return 0  # Quick mode
    else
        return 1  # Custom mode
    fi
}

# Function to show progress
show_progress() {
    local message=$1
    echo -e "${CYAN}▶ $message${NC}"
}

# ============ MAIN INSTALLATION LOGIC ============

# Check for existing installation
check_existing_install

# Check network
check_network

# Check if Rust is installed
RUST_INSTALLED=false
if command -v cargo &> /dev/null; then
    RUST_INSTALLED=true
    CARGO_VERSION=$(cargo --version | cut -d ' ' -f 2)
fi

# Determine if quick or custom mode
if quick_or_custom; then
    # Quick mode - automatic decision
    echo ""
    echo -e "${CYAN}Quick install selected. Detecting best method...${NC}"
    echo ""

    if [ "$RUST_INSTALLED" = true ]; then
        echo -e "${GREEN}✓ Rust detected (version $CARGO_VERSION)${NC}"
        echo -e "${CYAN}Building from source for best performance...${NC}"
        echo ""

        if ! in_project_dir; then
            clone_repo
        fi

        build_from_source
        install_binary
    else
        echo -e "${YELLOW}✗ Rust not detected.${NC}"
        echo -e "${CYAN}Checking for pre-built binary...${NC}"
        echo ""

        if download_binary; then
            # Success
            :
        else
            # Download failed, fallback to Rust
            echo -e "${CYAN}Binary download not available. Installing Rust and building from source...${NC}"
            echo ""
            install_rust_and_build
        fi
    fi
else
    # Custom mode - full user control
    echo ""
    echo -e "${CYAN}Custom install selected.${NC}"
    echo ""

    if [ "$RUST_INSTALLED" = true ]; then
        # Rust is installed
        echo -e "${GREEN}Rust is installed.${NC}"
        echo -e "${CYAN}  Version: $CARGO_VERSION${NC}"
        echo ""

        echo -e "${BOLD}What would you like to do?${NC}"
        echo ""
        echo -e "${CYAN}Option 1: Build from source (recommended)${NC}"
        echo "  ✓ Optimized for your device"
        echo "  ✓ Latest code with all features"
        echo "  ✓ You can modify and contribute"
        echo "  ✗ Takes 2-3 minutes to compile"
        echo ""
        echo -e "${CYAN}Option 2: Download pre-built binary${NC}"
        echo "  ✓ Ready to use immediately"
        echo "  ✓ No compilation needed"
        echo "  ✓ Works for most devices"
        echo "  ✗ May be older than source"
        echo ""
        echo -e "${BOLD}Choose an option:${NC}"
        echo "  [1] Build from source (recommended)"
        echo "  [2] Download pre-built binary"
        echo "  [3] Exit"
        echo ""
        echo -n "Choose (1/2/3): "
        read -r choice

        case "$choice" in
            1)
                echo ""
                show_progress "Building from source..."
                echo ""

                if ! in_project_dir; then
                    clone_repo
                fi

                build_from_source
                install_binary
                ;;

            2)
                echo ""
                show_progress "Downloading pre-built binary..."
                echo ""

                if download_binary; then
                    # Success
                    :
                else
                    handle_download_failure_with_rust $?
                fi
                ;;

            3)
                echo -e "${CYAN}Exiting.${NC}"
                exit 0
                ;;

            *)
                echo -e "${YELLOW}Invalid choice. Exiting.${NC}"
                exit 1
                ;;
        esac
    else
        # Rust is NOT installed
        echo -e "${YELLOW}Rust is not installed.${NC}"
        echo ""
        echo -e "${CYAN}Why do you need Rust?${NC}"
        echo "  • Rust is required to build pkgtrace from source"
        echo "  • Building from source ensures optimal performance for your device"
        echo "  • You get the latest features and bug fixes"
        echo "  • No need to wait for pre-built binaries"
        echo ""
        echo -e "${CYAN}Alternative: You can download a pre-built binary instead.${NC}"
        echo ""
        echo -e "${BOLD}Choose an option:${NC}"
        echo "  [1] Install Rust and build from source (recommended)"
        echo "  [2] Download pre-built binary (faster, but may be older)"
        echo "  [3] Exit and install Rust manually later"
        echo ""
        echo -n "Choose (1/2/3): "
        read -r choice

        case "$choice" in
            1)
                install_rust_and_build
                ;;

            2)
                echo ""
                show_progress "Downloading pre-built binary..."
                echo ""

                if download_binary; then
                    # Success
                    :
                else
                    handle_download_failure_without_rust $?
                fi
                ;;

            3)
                echo ""
                echo -e "${CYAN}Please install Rust manually:${NC}"
                echo "  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
                echo "  source \$HOME/.cargo/env"
                echo ""
                echo -e "${CYAN}Then run this installer again.${NC}"
                echo ""
                echo -e "${YELLOW}Why Rust?${NC}"
                echo "  • pkgtrace is written in Rust"
                echo "  • Building from source gives you the best performance"
                echo "  • You'll always have the latest version"
                echo "  • Rust is memory-safe and fast"
                exit 0
                ;;

            *)
                echo -e "${YELLOW}Invalid choice. Exiting.${NC}"
                exit 1
                ;;
        esac
    fi
fi

# ============ POST-INSTALLATION ============

# Check PATH
if [[ ":$PATH:" != *":$INSTALL_DIR:"* ]]; then
    echo -e "${YELLOW}Note: $INSTALL_DIR is not in your PATH.${NC}"
    echo -e "${CYAN}Add this to your .bashrc or .zshrc:${NC}"
    echo "  export PATH=\"\$PATH:$INSTALL_DIR\""
    echo ""
fi

# Show where the log is
echo -e "${CYAN}Installation log saved to: $LOG_FILE${NC}"
echo ""

# Run initial scan if requested
echo -e "${BOLD}Would you like to run an initial scan?${NC}"
echo -e "${CYAN}The program will create its configuration automatically.${NC}"
echo ""
echo -n "Run initial scan? (Y/n): "
read -r response

if [[ "$response" =~ ^[Yy]$ ]] || [[ -z "$response" ]]; then
    echo ""
    echo -e "${CYAN}Running initial scan...${NC}"
    "$INSTALL_DIR/pkgtrace" scan
    echo ""
fi

# Final instructions
echo -e "${GREEN}${BOLD}Installation complete!${NC}"
echo ""
echo -e "${CYAN}Quick start:${NC}"
echo "  pkgtrace scan          # Scan all packages"
echo "  pkgtrace list          # List installed packages"
echo "  pkgtrace unused        # Find unused packages"
echo "  pkgtrace clean         # Clean up unused packages"
echo "  pkgtrace stats         # Show package statistics"
echo ""
echo -e "${CYAN}For more help: pkgtrace --help${NC}"
echo ""
echo -e "${GREEN}Happy package tracking!${NC}"
