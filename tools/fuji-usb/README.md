# Fujifilm USB capture

Tools for documenting how Fujifilm cameras talk over USB in RAW conversion mode, so
OmaStudio's `fuji-camera` commands can support more cameras and settings. The method
is black-box observation of the USB cable: a vendor application such as X RAW Studio
is used normally while the kernel records the traffic. The application's files are
never disassembled, decompiled or copied, and nothing from it ships with OmaStudio.

## What you need

- The camera, its USB cable, and the connection mode set to **USB RAW CONV./BACKUP
  RESTORE** in the camera menu.
- X RAW Studio in a Windows virtual machine on the Omarchy machine (QEMU/KVM, for
  example through virt-manager), with the camera added to the VM as a USB host device
  (vendor `04cb`). The traffic between the VM and the camera passes through the
  host's USB stack, where usbmon sees it.
- `wireshark-cli` (`sudo pacman -S wireshark-cli`) and sudo rights for usbmon.

## Recording

```bash
tools/fuji-usb/capture.sh                 # writes ~/omastudio-captures/<date>/
```

1. Start the script with the camera connected and the VM running.
2. Before each action in X RAW Studio, type a short note and press Enter, for example
   `open DSCF0001.RAF`, `film simulation Classic Chrome`, `grain strong small`,
   `convert`, `save C3`. Change one setting at a time; that is what makes the
   transcript easy to read.
3. An empty line stops the recording and writes `transcript.txt`.

The folder holds `capture.pcapng`, `markers.tsv`, `transcript.txt` and `data/` (data
phases longer than 64 bytes, such as the conversion profile, one file each).

## Reading a capture again

```bash
python3 -I tools/fuji-usb/decode.py capture.pcapng --device <address> \
    --markers markers.tsv --dump data > transcript.txt
```

The transcript lists every PTP container (command, data, response, event) with the
operation and property names OmaStudio knows and the decoded values, interleaved with
your notes. Comparing the `data/` files of two captures that differ by one setting
shows which bytes of the conversion profile hold that setting.

Captures contain the RAW files and settings that were sent to the camera. Keep them
private; share only the transcript lines that matter.
