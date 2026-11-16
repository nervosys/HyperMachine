# A real browser draws a machine's desktop through noVNC

**What this checks:** the open item in the
[2026-10-12 desktop check](../../2026-10-12/machine-desktop-kvm/README.md):
a machine's [desktop](../../../MACHINES.md#desktop) was served by noVNC and
the VNC greeting was received, but no browser drew the screen. Here headless
Chromium opens noVNC's page through the published port, connects, and the
canvas it draws is read back and compared with the machine API's own
screenshot. Keys and the pointer are then sent through the browser alone.

## Method

`tools/check-desktop-browser-kvm.py` runs an owned `hv2-sandboxd` and creates
the same machine as the 2026-10-12 check: Ubuntu 24.04's image, two
processors, 2 GiB, 10 GiB disk, Internet allowed, `guestAgent`, Claude Code
and Codex, `desktop: true` (1280x800). Guest port 6080 is published. Then:

1. **Ran on KVM:** the daemon's log has `platform: Kvm` and no
   `platform: Tcg`.
2. **Desktop up:** the five desktop units are active and the terminal is one
   window.
3. **Browser connects:** Chromium opens `/vnc.html?autoconnect=true&resize=off`
   through the published port; noVNC says `Connected (unencrypted) to
   desk-01:1`; its `<canvas>` is 1280x800.
4. **Not blank:** read back with `toDataURL`, the canvas has at least 4
   distinct colours and its commonest colour covers under 99.5 % of it.
5. **Same as the API:** the canvas and `GET /desktop/screenshot` are the same
   size and their mean absolute difference per channel is under 2 of 255.
6. **Interaction, through the browser only:** the pointer is moved and
   clicked in the terminal's window on the canvas, then
   `echo typed-in-the-browser > /tmp/browser-typed; echo done-by-browser` is
   typed and Return pressed. The canvas must change, every changed pixel
   (over a difference of 32) must lie inside the terminal's window plus a
   30 px allowance for the window manager's frame, the guest's X server must
   put the pointer exactly where the browser clicked
   (`xdotool getmouselocation`), the guest must hold the file, and the canvas
   must still match the API's screenshot.

## Result

All six cases passed in each of three runs, on Linux 6.18.33 under WSL2 with
nested KVM (`platform: Kvm` once, `Tcg` never, in each run's log). From
[`run1`](run1/report.json), [`run2`](run2/report.json),
[`run3`](run3/report.json):

| | run 1 | run 2 | run 3 |
|---|---|---|---|
| Desktop ready, after the request | 266 s | 526 s | 376 s |
| Browser connected and canvas drawn | 2.1 s | 2.0 s | 2.1 s |
| Browser clicks and types, plus 3 s settle | 5.0 s | 5.0 s | 5.1 s |

A fourth run on the same inputs, by the reviewing session, independent of the three above, also passed all six cases on KVM: desktop ready at 406 s, canvas drawn in 2.5 s, interaction in 5.0 s. Its report is not kept here.

The desktop's install time is the archive's and the network's, not a claim.
The rest did not vary between runs:

| | all three runs |
|---|---|
| Canvas size | 1280x800 |
| Blank check | 2,739 distinct colours; the commonest, black, covers 75.23 % |
| Canvas against the API's screenshot | mean difference 0.0426 of 255 per channel; 0 pixels differ by over 32 |
| API screenshot, for reference | 792 distinct colours; black 75.23 % |
| Pixels the typing changed | 1,402, in the box (43,62)-(546,101) |
| Terminal's window | (42,80)-(646,474), client area |
| Pointer after the click | guest X server at (242,180); the click was aimed at (242,180) |
| File in the guest | `typed-in-the-browser` |
| Canvas against the API after typing | 0.0426 of 255 |

The changed box begins 18 px above the client area: it is the title bar,
repainted as the click gave the terminal focus, which is why the check allows
the frame. That the three runs agree to the pixel is expected: the screen is
a still picture of a terminal.

Pictures, at half size (640x400) to keep them small:

- [`canvas-first.png`](run1/canvas-first.png): noVNC's canvas as drawn,
  the terminal on a black desktop.
- [`canvas-after-typing.png`](run1/canvas-after-typing.png): the same canvas
  after the typing, the command and `done-by-browser` in the terminal.
- [`api-screenshot.png`](run1/api-screenshot.png) and
  [`api-screenshot-after-typing.png`](run1/api-screenshot-after-typing.png):
  the machine API's own screenshots, for comparison.
- [`browser-page.png`](run1/browser-page.png): the whole browser page,
  noVNC's own chrome around the canvas.

Each run directory holds the same set, and `report.json`.

## Inputs

sha256, as in `report.json` (`input_sha256`), which also holds the daemon,
the kernel, the initrd and the driver:

| | |
|---|---|
| `hypervisor-fw`, Rust Hypervisor Firmware 0.5.0, GitHub release asset | `4a0a1e97…ad958` (no checksum is published with the release) |
| `ubuntu-24.04-minimal-cloudimg-amd64.img`, as downloaded from `cloud-images.ubuntu.com/minimal/releases/noble/release/` on 2026-11-15 | `406630c0…a3fe0`, equal to its published `SHA256SUMS` |
| That image as raw (`qemu-img convert -f qcow2 -O raw`), nothing else changed | `ff40d989…304d2` |
| `hv2-guest-agentd`, built from this tree (HEAD `f0ae2e9e`) with `-C target-feature=+crt-static`, rustc 1.100.0 | `52f84b96…e1ddd32` |
| Chromium: Playwright 1.65.2's "Chrome for Testing" 159.0.8154.6 | executable `045558b2…201ae` |
| Pillow (to read the PNGs) | 13.0.0 |

- **The image was not patched.** The 2026-10-10 check set a root password
  and disabled cloud-init for a serial-console login. A machine with
  `guestAgent` gets its agent from cloud-init's vendor data, so disabling
  cloud-init would remove the agent; the 2026-10-12 desktop check also used
  the image as published. Nothing here logs in on the console.
- **Kernel and initrd** (`/var/tmp/hm-kernel-build/bzImage`,
  `initrd.cpio.gz`) are what the driver asks for as arguments. The
  2026-10-12 evidence does not record which it used. A machine made from an
  image boots from the firmware, not from them.
- The noble image is the current build in the `release` directory, not the
  one the 2026-10-12 run downloaded.
- Apt packages installed in WSL for this: `qemu-utils` (and its
  dependencies), and `libnss3` (with `libnspr4`), which Chromium needs.
  Playwright, Pillow and Chromium live under `/var/tmp/hm-desktop`.
- Chromium was started with `--no-sandbox` because the run is root in WSL.

## What this does not show

- **WebRTC, sound, clipboard, recording, or a stream of the browser alone.**
  noVNC is a picture and input; none of the others is exercised.
- **A frame rate or resolution beyond 1280x800.** Two seconds from page load
  to a drawn canvas is not a frame rate.
- **Chromium only.** Not Firefox or Safari; a headless browser, not a person
  watching it.
- **The proxy's login.** The port was published without `proxy: true`.
- **A browser running in the guest.** None is installed there.
- **Pointer shape or the cursor's drawing.** The pointer's position was
  checked in the guest; the cursor image was not looked for.
- **Why the canvas holds more distinct colours than the API's screenshot**
  (2,739 against 792) while differing by 0.04 of 255. It was not
  investigated; no pixel differs by over 32.
- The checks are of one machine, one image and one browser, three times, on
  nested KVM under WSL2.
