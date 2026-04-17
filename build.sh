#!/bin/bash
#
# Control Tower Build Script
# Builds all binaries for release
#

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

echo "========================================"
echo "  Control Tower Build Script"
echo "========================================"
echo ""

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Functions
info() { echo -e "${GREEN}[INFO]${NC} $1"; }
warn() { echo -e "${YELLOW}[WARN]${NC} $1"; }
error() { echo -e "${RED}[ERROR]${NC} $1"; }

# Check for required tools
check_tool() {
    if ! command -v $1 &> /dev/null; then
        error "$1 is required but not installed."
        exit 1
    fi
}

# Clean previous builds
clean() {
    info "Cleaning previous builds..."
    cargo clean
    rm -rf target/release/ctctl target/release/ctsvc 2>/dev/null || true
}

# Build all packages
build_all() {
    info "Building all packages..."
    cargo build --release --workspace
}

# Download Mihomo and geoip database
download_mihomo() {
    local dist="$1"
    local arch_suffix
    local mihomo_version="v1.19.0"
    local cache_dir="target/cache"

    # Create cache directory
    mkdir -p "$cache_dir"

    # Detect architecture
    case "$(uname -m)" in
        x86_64)
            arch_suffix="amd64-compatible"
            ;;
        aarch64|arm64)
            arch_suffix="arm64"
            ;;
        *)
            warn "Unsupported architecture: $(uname -m), skipping Mihomo download"
            return 0
            ;;
    esac

    # Create mihomo directory
    mkdir -p "$dist/mihomo"

    # Try to use cached files first
    local cached_mihomo="$cache_dir/mihomo-${arch_suffix}-${mihomo_version}"
    local cached_geoip="$cache_dir/geoip.metadb"
    local cached_geosite="$cache_dir/geosite.db"

    # Copy Mihomo from cache if exists
    if [ -f "$cached_mihomo" ]; then
        cp "$cached_mihomo" "$dist/mihomo/mihomo"
        chmod +x "$dist/mihomo/mihomo"
        info "Mihomo copied from cache"
    else
        info "Downloading Mihomo ${mihomo_version} for ${arch_suffix}..."

        # Download Mihomo binary
        local mihomo_url="https://github.com/MetaCubeX/mihomo/releases/download/${mihomo_version}/mihomo-linux-${arch_suffix}-${mihomo_version}.gz"
        local temp_file=$(mktemp)

        if curl -L --fail -o "${temp_file}" "${mihomo_url}" 2>/dev/null; then
            mv "${temp_file}" "$dist/mihomo/mihomo.gz"
            gunzip -f "$dist/mihomo/mihomo.gz"
            chmod +x "$dist/mihomo/mihomo"
            # Cache the downloaded file
            cp "$dist/mihomo/mihomo" "$cached_mihomo"
            info "Mihomo downloaded successfully"
        else
            warn "Failed to download Mihomo from ${mihomo_url}"
        fi

        rm -f "${temp_file}"
    fi

    # Copy geoip from cache if exists
    if [ -f "$cached_geoip" ]; then
        cp "$cached_geoip" "$dist/mihomo/geoip.metadb"
        info "geoip.metadb copied from cache"
    else
        info "Downloading geoip database..."
        local geoip_url="https://github.com/MetaCubeX/meta-rules-dat/releases/download/latest/geoip.metadb"

        if curl -L --fail -o "$dist/mihomo/geoip.metadb" "${geoip_url}" 2>/dev/null; then
            cp "$dist/mihomo/geoip.metadb" "$cached_geoip"
            info "geoip.metadb downloaded successfully"
        else
            warn "Failed to download geoip.metadb"
        fi
    fi

    # Copy geosite from cache if exists
    if [ -f "$cached_geosite" ]; then
        cp "$cached_geosite" "$dist/mihomo/geosite.db"
        info "geosite.db copied from cache"
    else
        info "Downloading geosite database..."
        local geosite_url="https://github.com/MetaCubeX/meta-rules-dat/releases/download/latest/geosite.db"

        if curl -L --fail -o "$dist/mihomo/geosite.db" "${geosite_url}" 2>/dev/null; then
            cp "$dist/mihomo/geosite.db" "$cached_geosite"
            info "geosite.db downloaded successfully"
        else
            warn "Failed to download geosite.db"
        fi
    fi

    # Create .mihomo directory in user's home and copy geoip/geosite files
    mkdir -p "${HOME}/.config/mihomo"
    if [ ! -f "${HOME}/.config/mihomo/geoip.metadb" ] && [ -f "$dist/mihomo/geoip.metadb" ]; then
        cp -f "$dist/mihomo/geoip.metadb" "${HOME}/.config/mihomo/"
    fi
    if [ ! -f "${HOME}/.config/mihomo/geosite.db" ] && [ -f "$dist/mihomo/geosite.db" ]; then
        cp -f "$dist/mihomo/geosite.db" "${HOME}/.config/mihomo/"
    fi
}

# Build specific package
build_package() {
    local pkg=$1
    info "Building $pkg..."
    cargo build --release -p $pkg
}

# Run tests
test_all() {
    info "Running tests..."
    cargo test --workspace
}

# Create distribution directory
create_dist() {
    local dist="target/dist/control-tower"

    info "Creating distribution at $dist..."

    mkdir -p "$dist"

    # Copy binaries
    cp target/release/ctctl "$dist/"
    cp target/release/ctsvc "$dist/"
    # Note: control-tower-web is integrated into the CLI via `control-tower web` command

    # Copy profiles directory if exists
    if [ -d "profiles" ]; then
        cp -r profiles "$dist/"
    fi
    if [ -f "profiles.yaml" ]; then
        cp profiles.yaml "$dist/"
    fi

    # Download Mihomo and geoip database
    download_mihomo "$dist"

    # Copy service script
    cat > "$dist/install-service.sh" << 'EOF'
#!/bin/bash
# Control Tower Service Installer
# Run with: sudo ./install-service.sh

echo "Installing Control Tower service..."

# Get the directory where this script is located
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BINARY="$SCRIPT_DIR/ctsvc"
CONFIG_DIR="${HOME}/.config/control-tower"

# Check if running as root
if [ "$EUID" -ne 0 ]; then
    echo "Please run as root: sudo $0"
    exit 1
fi

# Create config directory
mkdir -p "$CONFIG_DIR"

# Create systemd service file
cat > /etc/systemd/system/control-tower.service << SERVICE
[Unit]
Description=Control Tower Service
After=network.target

[Service]
Type=simple
ExecStart=${BINARY} --foreground --socket ${CONFIG_DIR}/ctsvc.sock
Restart=on-failure
RestartSec=5
User=${USER}

[Install]
WantedBy=multi-user.target
SERVICE

# Reload systemd and enable service
systemctl daemon-reload
systemctl enable control-tower.service

echo ""
echo "Service installed successfully!"
echo ""
echo "To start:   sudo systemctl start control-tower"
echo "To stop:    sudo systemctl stop control-tower"
echo "To status:  sudo systemctl status control-tower"
echo ""
EOF

    chmod +x "$dist/install-service.sh"

    # Create uninstall script
    cat > "$dist/uninstall-service.sh" << 'EOF'
#!/bin/bash
# Control Tower Service Uninstaller
# Run with: sudo ./uninstall-service.sh

echo "Uninstalling Control Tower service..."

if [ "$EUID" -ne 0 ]; then
    echo "Please run as root: sudo $0"
    exit 1
fi

systemctl stop control-tower.service 2>/dev/null || true
systemctl disable control-tower.service 2>/dev/null || true
rm /etc/systemd/system/control-tower.service
systemctl daemon-reload

echo ""
echo "Service uninstalled successfully!"
echo "Config files preserved at ~/.config/control-tower/"
EOF

    chmod +x "$dist/uninstall-service.sh"

    # Create README
    cat > "$dist/README.md" << 'EOF'
# Control Tower

A command-line proxy management tool.

## Binaries

- `ctctl` - CLI tool with integrated Web UI server
- `ctsvc` - Background service daemon

## Quick Start

1. Start the service:
   ```
   ./ctctl service start
   ```

2. List proxies:
   ```
   ./ctctl proxy list
   ```

3. Select a proxy:
   ```
   ./ctctl proxy select "香港 101"
   ```

## Web UI

Start the web interface:
```
./ctctl web
```

Then open http://127.0.0.1:8080

## Service Installation

To install as a system service (requires root):
```
sudo ./install-service.sh
```

## Commands

```
# Service management
ctctl service start/stop/restart/status/logs

# Proxy management
ctctl proxy list/select/test/stats

# Mode management
ctctl mode get/set tun-enable/tun-disable

# Rule management
ctctl rule list/add/remove/import/export

# Profile management
ctctl profile list/add/remove/update/activate/cron

# Connection management
ctctl connections list/close/detail

# Web UI
ctctl web
```

For full documentation, see the docs/ directory.
EOF

    # Create tarball
    info "Creating tarball..."
    cd target/dist
    tar -czvf "control-tower.tar.gz" "control-tower"
    cd ../..

    info "Distribution created at target/dist/"
    ls -la target/dist/
}

# Show help
show_help() {
    cat << 'EOF'
Usage: ./build.sh [COMMAND]

Commands:
    all         Build everything (default)
    clean       Clean previous builds
    test        Run tests
    cli         Build CLI only (includes web server)
    service     Build service only
    dist        Create distribution package
    help        Show this help

Examples:
    ./build.sh           # Build all
    ./build.sh test      # Run tests
    ./build.sh dist      # Create distribution

EOF
}

# Main
case "${1:-all}" in
    all)
        info "Building Control Tower..."
        build_all
        echo ""
        info "Build complete!"
        info "Binaries:"
        ls -la target/release/ct* 2>/dev/null || true
        ;;
    clean)
        clean
        ;;
    test)
        test_all
        ;;
    cli)
        build_package control-tower-cli
        ;;
    service)
        build_package control-tower-service
        ;;
    dist)
        build_all
        create_dist
        ;;
    help|--help|-h)
        show_help
        ;;
    *)
        error "Unknown command: $1"
        show_help
        exit 1
        ;;
esac
