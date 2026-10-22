#!/usr/bin/env python3
"""Turns a usbmon capture of a PTP camera into a readable transcript.

    decode.py capture.pcapng [--device N] [--markers markers.tsv] [--dump DIR]

Reassembles the PTP-over-USB containers (command, data, response, event) of each
direction, names the standard operations and the Fujifilm properties OmaStudio
knows, decodes property values, and interleaves the notes written during the
capture. Data phases longer than 64 bytes are saved to --dump for comparison
between captures. Needs tshark (wireshark-cli).
"""

import argparse
import os
import struct
import subprocess
import sys

OPS = {
    0x1001: "GetDeviceInfo", 0x1002: "OpenSession", 0x1003: "CloseSession",
    0x1004: "GetStorageIDs", 0x1005: "GetStorageInfo", 0x1006: "GetNumObjects",
    0x1007: "GetObjectHandles", 0x1008: "GetObjectInfo", 0x1009: "GetObject",
    0x100A: "GetThumb", 0x100B: "DeleteObject", 0x100C: "SendObjectInfo",
    0x100D: "SendObject", 0x100E: "InitiateCapture", 0x1014: "GetDevicePropDesc",
    0x1015: "GetDevicePropValue", 0x1016: "SetDevicePropValue", 0x101B: "GetPartialObject",
    0x900C: "Fuji.SendObjectInfo", 0x900D: "Fuji.SendObject",
}

PROPS = {
    0xD183: "StartRawConversion", 0xD185: "RawConversionProfile",
    0xD18C: "PresetSlot", 0xD18D: "PresetName", 0xD190: "Preset.DynamicRange",
    0xD192: "Preset.FilmSimulation", 0xD193: "Preset.MonoWC", 0xD194: "Preset.MonoMG",
    0xD195: "Preset.Grain", 0xD196: "Preset.ColorChrome", 0xD197: "Preset.ColorChromeBlue",
    0xD199: "Preset.WhiteBalance", 0xD19A: "Preset.WBShiftR", 0xD19B: "Preset.WBShiftB",
    0xD19C: "Preset.ColorTemp", 0xD19D: "Preset.Highlight", 0xD19E: "Preset.Shadow",
    0xD19F: "Preset.Color", 0xD1A0: "Preset.Sharpness", 0xD1A1: "Preset.NoiseReduction",
    0xD1A2: "Preset.Clarity",
}

RESPONSES = {0x2001: "OK", 0x2002: "GeneralError", 0x2005: "OperationNotSupported",
             0x2019: "DeviceBusy", 0x201D: "InvalidParameter"}

KINDS = {1: "CMD", 2: "DATA", 3: "RESP", 4: "EVENT"}


def name(table, code):
    return f"{table.get(code, '?')}(0x{code:04X})"


def frames(path, device):
    """(time, out?, payload) per bulk transfer that carries data."""
    flt = "usb.transfer_type == 0x03 && usb.capdata"
    if device is not None:
        flt += f" && usb.device_address == {device}"
    cmd = ["tshark", "-r", path, "-Y", flt, "-T", "fields", "-E", "separator=\t",
           "-e", "frame.time_epoch", "-e", "usb.endpoint_address", "-e", "usb.capdata"]
    proc = subprocess.run(cmd, capture_output=True, text=True, check=True)
    for row in proc.stdout.splitlines():
        t, ep, data = (row.split("\t") + ["", "", ""])[:3]
        if not data:
            continue
        ep = int(ep, 16) if ep.startswith("0x") else int(ep or 0)
        yield float(t), not (ep & 0x80), bytes.fromhex(data.replace(":", ""))


def containers(stream):
    """Reassembles containers that span several transfers, per direction."""
    pending = {True: None, False: None}
    for t, out, data in stream:
        cur = pending[out]
        if cur is None:
            if len(data) < 12:
                continue
            length = struct.unpack_from("<I", data)[0]
            if length < 12 or length > 1 << 31:
                length = len(data)
            cur = [t, bytearray(), length]
        cur[1] += data
        if len(cur[1]) >= cur[2]:
            yield cur[0], out, bytes(cur[1][:cur[2]])
            cur = None
        pending[out] = cur


def value(data, prop):
    if prop == 0xD18D or len(data) > 8:
        # PTP string: count of UTF-16 units, then the units
        if data and 1 + data[0] * 2 <= len(data):
            return repr(data[1:1 + data[0] * 2].decode("utf-16-le", "replace").rstrip("\0"))
        return data[:32].hex(" ")
    if len(data) == 2:
        u = struct.unpack("<H", data)[0]
        return f"{u} (0x{u:04X}, signed {struct.unpack('<h', data)[0]})"
    if len(data) == 4:
        u = struct.unpack("<I", data)[0]
        return f"{u} (0x{u:08X}, signed {struct.unpack('<i', data)[0]})"
    return data.hex(" ")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("capture")
    ap.add_argument("--device", type=int)
    ap.add_argument("--markers")
    ap.add_argument("--dump")
    a = ap.parse_args()

    notes = []
    if a.markers and os.path.exists(a.markers):
        with open(a.markers, encoding="utf-8") as f:
            for row in f:
                t, _, text = row.rstrip("\n").partition("\t")
                notes.append((float(t), text))
    if a.dump:
        os.makedirs(a.dump, exist_ok=True)

    start = None
    last_op, last_param = 0, 0
    n = 0
    for t, out, c in containers(frames(a.capture, a.device)):
        start = start if start is not None else min([t] + [x for x, _ in notes[:1]])
        while notes and notes[0][0] <= t:
            nt, text = notes.pop(0)
            print(f"\n=== {nt - start:9.3f}  {text}")
        length, kind, code, tx = struct.unpack_from("<IHHI", c)
        body = c[12:]
        arrow = "->" if out else "<-"
        head = f"{t - start:9.3f} {arrow} {KINDS.get(kind, kind):5} tx={tx:<5}"
        if kind == 1:
            params = list(struct.unpack_from(f"<{len(body) // 4}I", body))
            last_op, last_param = code, (params[0] if params else 0)
            shown = [name(PROPS, p) if p in PROPS else f"0x{p:X}" for p in params]
            print(f"{head} {name(OPS, code)} {' '.join(shown)}")
        elif kind == 2:
            desc = f"{len(body)} bytes"
            if last_op in (0x1015, 0x1016) and last_param in PROPS or len(body) <= 8:
                desc += f" {PROPS.get(last_param, '')} = {value(body, last_param)}"
            if a.dump and len(body) > 64:
                n += 1
                f = os.path.join(a.dump, f"{n:04d}-{OPS.get(last_op, f'{last_op:04X}')}-{'out' if out else 'in'}.bin")
                with open(f, "wb") as fh:
                    fh.write(body)
                desc += f" -> {os.path.basename(f)}"
            elif len(body) > 8:
                desc += " " + body[:48].hex(" ")
            print(f"{head} {desc}")
        else:
            params = " ".join(f"0x{p:X}" for p in struct.unpack_from(f"<{len(body) // 4}I", body))
            label = name(RESPONSES, code) if kind == 3 else f"0x{code:04X}"
            print(f"{head} {label} {params}")
    for nt, text in notes:
        print(f"\n=== {nt - (start or nt):9.3f}  {text}")


if __name__ == "__main__":
    try:
        main()
    except subprocess.CalledProcessError as e:
        sys.exit(f"tshark failed: {e.stderr.strip()}")
