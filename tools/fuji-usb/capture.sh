#!/usr/bin/env bash
# Records the USB traffic between a Fujifilm camera and a vendor application
# (X RAW Studio running in a virtual machine with the camera passed through) with
# the kernel's usbmon, plus timestamped notes of what was done in the application.
# Black-box observation only: the application itself is never inspected.
#
#   tools/fuji-usb/capture.sh [output-dir]
#
# Needs: wireshark-cli (tshark), sudo for usbmon. The capture holds whatever the
# application sent, including your RAW files: keep it private.
set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
command -v tshark >/dev/null || { echo "tshark is missing: sudo pacman -S wireshark-cli" >&2; exit 1; }

line=$(lsusb -d 04cb: | head -n1 || true)
if [[ -z $line ]]; then
    echo "No Fujifilm camera on USB. Set the camera's connection mode to" >&2
    echo "USB RAW CONV./BACKUP RESTORE and connect it." >&2
    exit 1
fi
bus=$((10#$(awk '{print $2}' <<<"$line")))
dev=$((10#$(awk '{print $4}' <<<"$line" | tr -d :)))
echo "Camera: $line"

out=${1:-"$HOME/omastudio-captures/$(date +%Y%m%d-%H%M%S)"}
mkdir -p "$out"
markers="$out/markers.tsv"
: >"$markers"

sudo modprobe usbmon
sudo -v
sudo tshark -q -i "usbmon$bus" -w "$out/capture.pcapng" 2>"$out/tshark.log" &
cap=$!
trap 'sudo kill -INT $cap 2>/dev/null || true' EXIT
sleep 2

cat <<MSG

Recording bus $bus (camera address $dev) to $out
Now use the application one step at a time. Before each step, type a short note
of what you are about to do and press Enter, for example:
  connect | open DSCF0001.RAF | film simulation Classic Chrome | grain strong small
  convert | save profile | read C1
An empty line stops the recording.

MSG
while IFS= read -r -p "> " note && [[ -n $note ]]; do
    printf '%s\t%s\n' "$(date +%s.%N)" "$note" >>"$markers"
done

sudo kill -INT $cap
wait $cap 2>/dev/null || true
trap - EXIT
sudo chown "$(id -u):$(id -g)" "$out/capture.pcapng"
echo "device=$dev" >"$out/device"

python3 -I "$here/decode.py" "$out/capture.pcapng" --device "$dev" --markers "$markers" --dump "$out/data" >"$out/transcript.txt"
echo "Transcript: $out/transcript.txt"
