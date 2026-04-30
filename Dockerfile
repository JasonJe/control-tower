FROM ubuntu:latest

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    ca-certificates \
    curl \
    tzdata \
    && rm -rf /var/lib/apt/lists/* \
    && useradd -m -s /bin/bash ct

# Create app directories with proper permissions
RUN mkdir -p /tmp/logs /app/config /app/profiles /app/mihomo && \
    chown -R ct:ct /tmp /app

# Copy binaries and config from dist package
COPY target/dist/control-tower-20260415/ctctl /usr/local/bin/ctctl
COPY target/dist/control-tower-20260415/ctsvc /usr/local/bin/ctsvc
COPY target/dist/control-tower-20260415/mihomo/ /app/mihomo/
COPY target/dist/control-tower-20260415/config/ /app/config/
COPY target/dist/control-tower-20260415/settings.yaml /app/settings.yaml

# Make binaries executable
RUN chmod +x /usr/local/bin/ctctl /usr/local/bin/ctsvc /app/mihomo/mihomo

# Set ownership
RUN chown -R ct:ct /app /tmp

USER ct
WORKDIR /app

# Clean up any existing socket file
RUN rm -f /tmp/ctsvc.sock

# Expose ports
# 9090 - Mihomo API
# 7890 - HTTP Proxy
# 7891 - SOCKS5 Proxy
# 8080 - Web UI
EXPOSE 9090 7890 7891 8080

# Default command: start service and keep container running
# Configure timezone at runtime using TZ environment variable
CMD ["sh", "-c", "if [ -n \"$TZ\" ]; then ln -snf /usr/share/zoneinfo/$TZ /etc/localtime 2>/dev/null || true; fi; ctsvc --foreground & sleep infinity"]
