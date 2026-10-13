# A machine's desktop, and computer use on it

**What this checks:** `desktop` on a machine from a distribution's image.
The machine gets a virtual display with a window manager and a terminal,
noVNC to watch it in a browser, and screenshots, pointer and keys through
the API. Each coding agent installed is given the same as an MCP server.
boat has a desktop and computer use; see the [comparison](../../../BOAT_PARITY.md).

**No key was used, so no agent used the desktop.** What is shown is that
each agent connects to the computer-use server, and that the server works.

## Method

`tools/check-machine-desktop-kvm.py` runs an owned `hv2-sandboxd` and
creates one machine from Ubuntu 24.04's image as published. It has two
processors, 2 GiB and a 10 GiB disk, the Internet allowed, `guestAgent`,
Claude Code and Codex, and `desktop: true`.

1. **Refusals:** five things that cannot be made are refused, each with
   why.
2. **Install:** the five desktop units are active, the size is 1280x800,
   and the terminal is one window on it that has not restarted.
3. **Screenshot:** a PNG of 1280x800.
4. **Clicks:**
   - a second terminal is opened at the lower right;
   - the first terminal is clicked, a command is typed and Return is
     pressed, then the same is done in the second terminal, then in the
     first again;
   - each command ran in the terminal that was clicked.
5. **Other actions:** move, drag, scroll and `cursor` leave the pointer
   where they say. Five bad actions are refused, each with why.
6. **noVNC:** guest port 6080 is published.
   - `/vnc.html` is served through it.
   - A WebSocket handshake to `/websockify` gets the VNC server's protocol
     greeting.
7. **The agents' tools:**
   - `claude mcp list`, for root and for `ubuntu`, finds `computer` and
     connects to it;
   - `codex mcp list` lists it for both;
   - the server, run as `ubuntu`, gives its eight tools and a screenshot.
8. **Restart:** the machine is stopped and started, and the desktop comes
   back.

## Result

All eight cases passed on 2026-10-12, on Linux 6.18.33 under WSL2 with
nested KVM, on a 24-thread host. From [`report.json`](report.json):

| | |
|---|---|
| Desktop installed | 165 s after it was asked for, on top of both agents |
| Screenshot | 1280x800 PNG, 8,089 bytes |
| Typed where clicked | `first`, `second`, `first-again`, each in its own terminal |
| noVNC | `/vnc.html` 15,212 bytes; greeting `RFB 003.008` |
| Claude Code, root and `ubuntu` | `computer: /usr/local/bin/hv2-computer mcp - ✔ Connected` |
| Codex, root and `ubuntu` | `computer … enabled` |
| The server as `ubuntu` | 8 tools; a screenshot of 31,352 bytes of base64 |
| After a restart | a screenshot again in 30 s |

[`after-typing.png`](after-typing.png) is the screenshot taken in case 4:
the two terminals, each with the commands typed into it.

The first run failed at case 3 with a black screen. The terminal could not
start: it needs X's core fonts. These come with the X server only as a
recommendation, and the install leaves recommendations out. WSL, where the
helper was tried first, had them already. The install now asks for them by
name, and the check now asserts that the terminal is a window and has not
restarted, not only that the picture is big enough.

## What this does not show

- **An agent using the desktop.** That needs a key.
- **Fedora and its kin.** The `dnf` package names have not been run.
- **noVNC in a browser.** The page and the VNC greeting were fetched by the
  check; no browser drew the screen.
- **A browser on the desktop.** None is installed.
- **A user made after the first boot.** That user is not given the tools.
- **Speed.** One run; the install time is mostly the network's.
