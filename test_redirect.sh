#!/bin/bash
# Quick test for shell redirection
pkill -9 -f qemu-system 2>/dev/null; sleep 1
rm -f /tmp/neodos_serial.log /tmp/neodos_qemu_stderr.log

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
OVMF_CODE="/usr/share/OVMF/OVMF_CODE.fd"
OVMF_VARS_TMPL="/usr/share/OVMF/OVMF_VARS.fd"
OVMF_VARS="/tmp/OVMF_VARS_test_$$.fd"
cp "$OVMF_VARS_TMPL" "$OVMF_VARS"

qemu-system-x86_64 \
  -machine q35,accel=tcg \
  -monitor telnet:127.0.0.1:4446,server,nowait \
  -display none \
  -drive if=pflash,format=raw,readonly=on,file="$OVMF_CODE" \
  -drive if=pflash,format=raw,file="$OVMF_VARS" \
  -device ahci,id=ahci \
  -drive if=none,format=raw,file="$SCRIPT_DIR/disk_image.img",id=mydisk \
  -device ide-hd,drive=mydisk,bus=ahci.0 \
  -netdev user,id=net0,net=10.0.1.0/24,dhcpstart=10.0.1.80,host=10.0.1.1 \
  -device e1000,netdev=net0 \
  -m 512M \
  -serial file:/tmp/neodos_serial.log \
  2>/tmp/neodos_qemu_stderr.log &

QEMU_PID=$!
echo "QEMU PID: $QEMU_PID"

# Wait for shell prompt
echo "Waiting for shell..."
TIMEOUT=120
while [ $TIMEOUT -gt 0 ]; do
  if grep -q "Type HELP" /tmp/neodos_serial.log 2>/dev/null; then
    echo "Shell detected!"
    break
  fi
  sleep 1
  TIMEOUT=$((TIMEOUT - 1))
done

if [ $TIMEOUT -eq 0 ]; then
  echo "Timeout waiting for shell"
  kill $QEMU_PID 2>/dev/null
  rm -f "$OVMF_VARS"
  exit 1
fi

sleep 2

# Connect to monitor and send keys
send_keys() {
  local keys="$1"
  sleep 0.5
  for key in $keys; do
    echo "sendkey $key" | nc -q 0 127.0.0.1 4446 2>/dev/null
    sleep 0.15
  done
  sleep 1
}

send_type() {
  local text="$1"
  for ((i=0; i<${#text}; i++)); do
    local ch="${text:$i:1}"
    case "$ch" in
      ' ') key="spc" ;;
      '>') key="shift-." ;;
      '<') key="shift-," ;;
      '|') key="shift-\\" ;;
      '/') key="slash" ;;
      '.') key="dot" ;;
      '\') key="backslash" ;;
      ':') key="shift-;" ;;
      [a-z]) key="$ch" ;;
      [A-Z]) key="shift-$(echo $ch | tr 'A-Z' 'a-z')" ;;
      *) key="$ch" ;;
    esac
    echo "sendkey $key" | nc -q 0 127.0.0.1 4446 2>/dev/null
    sleep 0.08
  done
  echo "sendkey ret" | nc -q 0 127.0.0.1 4446 2>/dev/null
  sleep 2
}

echo "=== Test 1: dir > hello.txt ==="
send_type "dir > hello.txt"
sleep 3

echo "=== Test 2: type hello.txt ==="
send_type "type hello.txt"
sleep 3

echo "=== Test 3: dir > hello.txt | type hello.txt ==="
send_type "dir > hello.txt | type hello.txt"
sleep 3

# Show output
echo ""
echo "=== SERIAL LOG ==="
cat /tmp/neodos_serial.log

# Cleanup
kill $QEMU_PID 2>/dev/null
wait $QEMU_PID 2>/dev/null
rm -f "$OVMF_VARS"
echo ""
echo "=== Done ==="
