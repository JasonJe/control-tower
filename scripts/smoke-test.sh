#!/usr/bin/env bash
#
# Smoke test for the compiled Control Tower binaries.
#
# Unlike `cargo test`, nothing here goes through the Rust test harness: it
# starts the real release binary, talks to it over the Unix socket exactly like
# the CLI/UI do, and checks that it shuts down cleanly. Linux only (Unix
# sockets, SIGTERM handling, /proc-free but POSIX-y tools).
#
# Usage:
#   bash scripts/smoke-test.sh
#   BIN_DIR=/usr/bin SMOKE_LOG_DIR=/tmp/smoke bash scripts/smoke-test.sh

set -euo pipefail

ROOT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
BIN_DIR=${BIN_DIR:-$ROOT_DIR/target/release}
CT_SVC=$BIN_DIR/ctsvc
CT_CLI=$BIN_DIR/ctctl
DEFAULT_SOCKET=/tmp/ctsvc.sock

WORK_DIR=$(mktemp -d)
LOG_DIR=${SMOKE_LOG_DIR:-$WORK_DIR}
mkdir -p "$LOG_DIR"

SOCKET=$WORK_DIR/ctsvc.sock
SERVICE_LOG=$LOG_DIR/ctsvc.log
DUP_LOG=$LOG_DIR/ctsvc-duplicate.log
CTCTL_SERVICE_LOG=$LOG_DIR/ctsvc-ctctl.log
SERVICE_PID=

fail() {
  echo "FAIL: $*" >&2
  exit 1
}

cleanup() {
  local status=$?
  trap - EXIT
  if [[ -n $SERVICE_PID ]] && kill -0 "$SERVICE_PID" 2>/dev/null; then
    echo "ctsvc (pid $SERVICE_PID) is still running, killing it" >&2
    kill -9 "$SERVICE_PID" 2>/dev/null || true
    wait "$SERVICE_PID" 2>/dev/null || true
  fi
  if [[ $status -ne 0 ]]; then
    echo "===== last lines of $SERVICE_LOG =====" >&2
    tail -n 100 "$SERVICE_LOG" 2>/dev/null >&2 || true
  fi
  rm -rf "$WORK_DIR"
  exit "$status"
}
trap cleanup EXIT

wait_for_socket() {
  local path=$1
  for _ in $(seq 1 150); do
    [[ -S $path ]] && return 0
    sleep 0.1
  done
  return 1
}

wait_for_exit() {
  local pid=$1
  for _ in $(seq 1 150); do
    kill -0 "$pid" 2>/dev/null || return 0
    sleep 0.1
  done
  return 1
}

echo "==> binaries"
[[ -x $CT_SVC ]] || fail "missing executable: $CT_SVC (run cargo build --release --workspace)"
[[ -x $CT_CLI ]] || fail "missing executable: $CT_CLI (run cargo build --release --workspace)"
"$CT_SVC" --version
"$CT_CLI" --version

echo "==> starting ctsvc (foreground, socket: $SOCKET)"
mkdir -p "$WORK_DIR/home"
HOME=$WORK_DIR/home "$CT_SVC" --foreground --socket "$SOCKET" >"$SERVICE_LOG" 2>&1 &
SERVICE_PID=$!

wait_for_socket "$SOCKET" || fail "ctsvc did not create its socket within 15s (log: $SERVICE_LOG)"
kill -0 "$SERVICE_PID" 2>/dev/null || fail "ctsvc exited during startup (log: $SERVICE_LOG)"

socket_mode=$(stat -c '%a' "$SOCKET")
[[ $socket_mode == 600 ]] || fail "IPC socket mode is $socket_mode, expected 600"
echo "==> IPC socket mode: $socket_mode"

settings_file=$BIN_DIR/settings.yaml
if [[ -f $settings_file ]]; then
  settings_mode=$(stat -c '%a' "$settings_file")
  [[ $settings_mode == 600 ]] || fail "$settings_file mode is $settings_mode, expected 600"
  echo "==> settings.yaml mode: $settings_mode"
fi

echo "==> a second instance must refuse to start"
if "$CT_SVC" --foreground --socket "$SOCKET" >"$DUP_LOG" 2>&1; then
  fail "a second ctsvc started even though $SOCKET was already in use"
fi
grep -q "already running" "$DUP_LOG" \
  || fail "the second instance failed for an unexpected reason (log: $DUP_LOG)"

echo "==> IPC round-trip"
python3 - "$SOCKET" <<'PY'
import json
import socket
import sys

sock_path = sys.argv[1]


def fail(message):
    print(f"FAIL: {message}", file=sys.stderr)
    sys.exit(1)


def call(raw):
    """Send one newline-terminated command and return the parsed reply (or None
    when the server closed the connection without answering)."""
    sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    sock.settimeout(10)
    try:
        sock.connect(sock_path)
        sock.sendall(raw if raw.endswith(b"\n") else raw + b"\n")
        data = b""
        while b"\n" not in data:
            chunk = sock.recv(4096)
            if not chunk:
                break
            data += chunk
    finally:
        sock.close()
    if not data.strip():
        return None
    return json.loads(data)


status = call(b'{"cmd":"Status"}')
if status is None or status["code"] != 0:
    fail(f"Status failed: {status}")
if status["data"]["running"] is not False:
    fail(f"service should not report a running Mihomo: {status['data']}")
if status["data"]["state"] != "NotRunning":
    fail(f"unexpected lifecycle state: {status['data']}")
print(f"ok: Status -> {status['data']['state']}")

logs = call(b'{"cmd":"Logs","data":{"lines":10}}')
if logs is None or logs["code"] != 0:
    fail(f"Logs failed: {logs}")
if not isinstance(logs["data"], list):
    fail(f"Logs data should be a list: {logs}")
print(f"ok: Logs -> {len(logs['data'])} entries")

cron = call(b'{"cmd":"ReloadCron"}')
if cron is None or cron["code"] != 0:
    fail(f"ReloadCron failed: {cron}")
print("ok: ReloadCron")

bad = call(b'this is not json')
if bad is not None and bad["code"] == 0:
    fail(f"a malformed request was accepted: {bad}")
print("ok: malformed request rejected")

status = call(b'{"cmd":"Status"}')
if status is None or status["code"] != 0:
    fail(f"service is unhealthy after a malformed request: {status}")
print("ok: still healthy after a malformed request")

shutdown = call(b'{"cmd":"Shutdown"}')
if shutdown is None or shutdown["code"] != 0:
    fail(f"Shutdown failed: {shutdown}")
print("ok: Shutdown accepted")
PY

echo "==> waiting for ctsvc to exit"
wait_for_exit "$SERVICE_PID" || fail "ctsvc did not exit after the Shutdown command"

set +e
wait "$SERVICE_PID"
exit_code=$?
set -e
SERVICE_PID=

[[ $exit_code -eq 0 ]] || fail "ctsvc exited with code $exit_code after Shutdown"
[[ ! -e $SOCKET ]] || fail "$SOCKET was not removed on shutdown"
echo "==> clean exit, socket removed"

echo "==> ctctl against a running ctsvc (default socket)"
[[ ! -e $DEFAULT_SOCKET ]] || fail "$DEFAULT_SOCKET already exists, refusing to touch it"

HOME=$WORK_DIR/home "$CT_SVC" --foreground >"$CTCTL_SERVICE_LOG" 2>&1 &
SERVICE_PID=$!
wait_for_socket "$DEFAULT_SOCKET" \
  || fail "ctsvc did not create $DEFAULT_SOCKET (log: $CTCTL_SERVICE_LOG)"

status_out=$("$CT_CLI" service status)
echo "$status_out"
grep -q "Status: Stopped" <<<"$status_out" \
  || fail "ctctl service status did not report 'Status: Stopped'"

stop_out=$("$CT_CLI" service stop)
echo "$stop_out"
grep -q "Control Tower service stopped" <<<"$stop_out" \
  || fail "ctctl service stop did not succeed"

echo "==> SIGTERM must shut the service down gracefully"
kill -TERM "$SERVICE_PID"
wait_for_exit "$SERVICE_PID" || fail "ctsvc ignored SIGTERM"

set +e
wait "$SERVICE_PID"
exit_code=$?
set -e
SERVICE_PID=

[[ $exit_code -eq 0 ]] || fail "ctsvc exited with code $exit_code after SIGTERM"
[[ ! -e $DEFAULT_SOCKET ]] || fail "$DEFAULT_SOCKET was not removed on shutdown"

echo "==> smoke test passed"
