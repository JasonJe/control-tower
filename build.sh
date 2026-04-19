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

    # Copy settings.yaml if exists, otherwise create default
    if [ -f "settings.yaml" ]; then
        cp settings.yaml "$dist/"
    else
        cat > "$dist/settings.yaml" << 'SETTINGS'
api_host: 127.0.0.1
api_port: 9090
http_port: 7890
socks_port: 7891
service_port: 8080
tun_enabled: false
log_level: info
mode: rule
SETTINGS
    fi

    # Download Mihomo and geoip database
    download_mihomo "$dist"

    # Create install service script
    cat > "$dist/install-service.sh" << 'INSTALL_EOF'
#!/bin/bash
# ctsvc Installer (systemd service)
# Run with: sudo ./install-service.sh

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
INSTALL_DIR="/opt/ctsvc"
BINARY="$INSTALL_DIR/ctsvc"
SERVICE_NAME="ctsvc"
SERVICE_FILE="/etc/systemd/system/${SERVICE_NAME}.service"

echo "Installing ${SERVICE_NAME} service to $INSTALL_DIR..."

# Check if running as root
if [ "$EUID" -ne 0 ]; then
    echo "ERROR: Please run as root: sudo $0"
    exit 1
fi

# Check if binary exists in install source
if [ ! -f "$SCRIPT_DIR/ctsvc" ]; then
    echo "ERROR: Binary not found: $SCRIPT_DIR/ctsvc"
    exit 1
fi

# Stop existing service if running
if systemctl is-active --quiet ${SERVICE_NAME} 2>/dev/null; then
    echo "Stopping existing ${SERVICE_NAME}..."
    systemctl stop ${SERVICE_NAME} 2>/dev/null || true
fi

# Create install directory
echo "Creating install directory..."
mkdir -p "$INSTALL_DIR"

# Copy files to install directory
echo "Copying files to $INSTALL_DIR..."
cp "$SCRIPT_DIR/ctsvc" "$INSTALL_DIR/"
cp "$SCRIPT_DIR/ctctl" "$INSTALL_DIR/"
if [ -f "$SCRIPT_DIR/settings.yaml" ]; then
    cp "$SCRIPT_DIR/settings.yaml" "$INSTALL_DIR/"
fi
if [ -d "$SCRIPT_DIR/mihomo" ]; then
    cp -r "$SCRIPT_DIR/mihomo" "$INSTALL_DIR/"
fi

# Make binary executable
chmod +x "$BINARY"

echo "Running pre-installation checks..."

# Check if ctsvc is already running
if pgrep -x "ctsvc" > /dev/null 2>&1; then
    echo "WARNING: ctsvc is already running!"
    echo "Stopping existing instance..."
    systemctl stop ${SERVICE_NAME} 2>/dev/null || true
    sleep 1
fi

# Check if socket file exists (stale from previous run)
SOCKET_PATH="/tmp/ctsvc.sock"
if [ -S "$SOCKET_PATH" ]; then
    echo "Removing stale socket file: $SOCKET_PATH"
    rm -f "$SOCKET_PATH"
fi

# Check if service port (8080) is available
SERVICE_PORT=8080
if netstat -tuln 2>/dev/null | grep -q ":${SERVICE_PORT} " || ss -tuln 2>/dev/null | grep -q ":${SERVICE_PORT} "; then
    echo "WARNING: Port ${SERVICE_PORT} is already in use!"
fi

# Create systemd service file
cat > "$SERVICE_FILE" << SERVICE
[Unit]
Description=ctsvc - Control Tower Service
After=network.target

[Service]
Type=simple
ExecStart="${BINARY}"
Restart=on-failure
RestartSec=5
User=${USER}
Environment="RUST_LOG=info"
NoNewPrivileges=true
AmbientCapabilities=CAP_NET_ADMIN
ProtectSystem=strict
ProtectHome=false
PrivateTmp=true
ReadWritePaths=/opt/ctsvc
StandardOutput=journal
StandardError=journal
SyslogIdentifier=ctsvc

[Install]
WantedBy=multi-user.target
SERVICE

echo "Enabling and starting service..."
systemctl daemon-reload
systemctl enable ${SERVICE_NAME}

if systemctl start ${SERVICE_NAME}; then
    echo ""
    echo "========================================"
    echo "  ${SERVICE_NAME} installed successfully!"
    echo "========================================"
    echo ""
    echo "Commands:"
    echo "  Start:    sudo systemctl start ${SERVICE_NAME}"
    echo "  Stop:     sudo systemctl stop ${SERVICE_NAME}"
    echo "  Status:   sudo systemctl status ${SERVICE_NAME}"
    echo "  Logs:     sudo journalctl -u ${SERVICE_NAME} -f"
    echo ""
    echo "Web UI: http://127.0.0.1:${SERVICE_PORT}"
    echo ""
else
    echo ""
    echo "ERROR: Failed to start ${SERVICE_NAME}"
    echo "Check logs with: sudo journalctl -u ${SERVICE_NAME} -xe"
    exit 1
fi
INSTALL_EOF

    chmod +x "$dist/install-service.sh"

    # Create uninstall service script
    cat > "$dist/uninstall-service.sh" << 'UNINSTALL_EOF'
#!/bin/bash
# ctsvc Uninstaller (systemd service)
# Run with: sudo ./uninstall-service.sh

set -e

SERVICE_NAME="ctsvc"
INSTALL_DIR="/opt/ctsvc"

echo "Uninstalling ${SERVICE_NAME} service..."

if [ "$EUID" -ne 0 ]; then
    echo "ERROR: Please run as root: sudo $0"
    exit 1
fi

if systemctl is-active --quiet ${SERVICE_NAME} 2>/dev/null; then
    echo "Stopping ${SERVICE_NAME}..."
    systemctl stop ${SERVICE_NAME} 2>/dev/null || true
fi

if systemctl is-enabled --quiet ${SERVICE_NAME} 2>/dev/null; then
    echo "Disabling ${SERVICE_NAME}..."
    systemctl disable ${SERVICE_NAME} 2>/dev/null || true
fi

if [ -f "/etc/systemd/system/${SERVICE_NAME}.service" ]; then
    echo "Removing systemd service file..."
    rm -f /etc/systemd/system/${SERVICE_NAME}.service
    systemctl daemon-reload
fi

# Remove installed files
if [ -d "$INSTALL_DIR" ]; then
    echo "Removing installed files from $INSTALL_DIR..."
    rm -rf "$INSTALL_DIR"
fi

echo ""
echo "========================================"
echo "  ${SERVICE_NAME} uninstalled successfully!"
echo "========================================"
UNINSTALL_EOF

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
