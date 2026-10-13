#!/usr/bin/env python3
"""Computer use on a machine's desktop, written into the guest by its node.

The desktop is display :1, a virtual X server. This acts on it with xdotool
and takes pictures of it with scrot, for two callers:

  hv2-computer size                 its width and height, as JSON
  hv2-computer screenshot PATH      a PNG of the whole screen
  hv2-computer act ACTION           one action, given as a JSON object
  hv2-computer mcp                  the same, as an MCP server on stdio

`act` is what the node runs, through its guest agent, with the action as one
argument: nothing in it is read by a shell. `mcp` is what each coding agent
installed in the machine is given, so that it can see the screen and use it.

An action is {"action": NAME, ...}:

  move         x, y
  click        x, y, and optionally button (left, middle, right) and count
  drag         x, y to toX, toY, with button
  type         text, typed as keystrokes
  key          keys: an xdotool key name or chord, such as Return or ctrl+c;
               several separated by spaces
  scroll       x, y, direction (up, down, left, right), amount
  cursor       nothing; where the pointer is

Each answers with where the pointer is afterwards, {"cursor": [x, y]}.
"""
import base64
import json
import os
import re
import subprocess
import sys
import tempfile

DISPLAY = os.environ.get("HV2_DISPLAY", ":1")
ENV = dict(os.environ, DISPLAY=DISPLAY)
BUTTONS = {"left": "1", "middle": "2", "right": "3"}
WHEEL = {"up": "4", "down": "5", "left": "6", "right": "7"}
# X key names (Return, Page_Down, F5, slash) joined into chords by +, such as
# ctrl+shift+t; several separated by single spaces.
KEY = r"[A-Za-z0-9_]+(\+[A-Za-z0-9_]+)*"
KEYS = re.compile(rf"^{KEY}( {KEY})*$")
MOST_TEXT = 4096
MOST_REPEAT = 100


class Refused(Exception):
    """What was asked cannot be done, and why."""


def run(*args):
    try:
        done = subprocess.run(list(args), env=ENV, capture_output=True, text=True, timeout=60)
    except FileNotFoundError:
        raise Refused(f"{args[0]} is not installed")
    except subprocess.TimeoutExpired:
        raise Refused(f"{args[0]} did not finish in 60 seconds")
    if done.returncode != 0:
        raise Refused(f"{args[0]} failed: {(done.stderr or done.stdout).strip()}")
    return done.stdout


def size():
    width, height = run("xdotool", "getdisplaygeometry").split()
    return int(width), int(height)


def cursor():
    found = dict(line.split("=", 1) for line in run("xdotool", "getmouselocation", "--shell").split())
    return int(found["X"]), int(found["Y"])


def screenshot(path):
    run("scrot", "--overwrite", "--silent", path)


def number(action, name, low, high):
    value = action.get(name)
    if not isinstance(value, int) or isinstance(value, bool) or not low <= value <= high:
        raise Refused(f"{name} must be a whole number from {low} to {high}")
    return value


def point(action, x="x", y="y"):
    width, height = size()
    return number(action, x, 0, width - 1), number(action, y, 0, height - 1)


def choice(action, name, choices, default):
    value = action.get(name, default)
    if value not in choices:
        raise Refused(f"{name} must be one of {', '.join(sorted(choices))}")
    return choices[value]


def act(action):
    if not isinstance(action, dict):
        raise Refused("an action is a JSON object")
    name = action.get("action")
    if name == "move":
        x, y = point(action)
        run("xdotool", "mousemove", "--sync", str(x), str(y))
    elif name == "click":
        x, y = point(action)
        button = choice(action, "button", BUTTONS, "left")
        count = number(action, "count", 1, 3) if "count" in action else 1
        run("xdotool", "mousemove", "--sync", str(x), str(y), "click", "--repeat", str(count), button)
    elif name == "drag":
        x, y = point(action)
        to_x, to_y = point(action, "toX", "toY")
        button = choice(action, "button", BUTTONS, "left")
        run("xdotool", "mousemove", "--sync", str(x), str(y), "mousedown", button,
            "mousemove", "--sync", str(to_x), str(to_y), "mouseup", button)
    elif name == "type":
        text = action.get("text")
        if not isinstance(text, str) or not text or len(text) > MOST_TEXT:
            raise Refused(f"text must be 1 to {MOST_TEXT} characters")
        run("xdotool", "type", "--delay", "12", "--", text)
    elif name == "key":
        keys = action.get("keys")
        if not isinstance(keys, str) or not KEYS.match(keys):
            raise Refused("keys must be key names or chords such as Return or ctrl+c, separated by spaces")
        run("xdotool", "key", "--", *keys.split(" "))
    elif name == "scroll":
        x, y = point(action)
        wheel = choice(action, "direction", WHEEL, "down")
        amount = number(action, "amount", 1, MOST_REPEAT) if "amount" in action else 3
        run("xdotool", "mousemove", "--sync", str(x), str(y), "click", "--repeat", str(amount), wheel)
    elif name != "cursor":
        raise Refused("action must be one of move, click, drag, type, key, scroll, cursor")
    return {"cursor": list(cursor())}


def screenshot_bytes():
    with tempfile.TemporaryDirectory() as tmp:
        path = os.path.join(tmp, "screen.png")
        screenshot(path)
        with open(path, "rb") as f:
            return f.read()


POINT = {"x": {"type": "integer", "minimum": 0}, "y": {"type": "integer", "minimum": 0}}
BUTTON = {"type": "string", "enum": sorted(BUTTONS)}
TOOLS = [
    ("screenshot", "A picture of the whole screen, as a PNG.", {}, [],
     lambda args: None),
    ("move", "Move the pointer to x, y.", POINT, ["x", "y"],
     lambda args: {"action": "move", **args}),
    ("click", "Move the pointer to x, y and click.", {**POINT, "button": BUTTON,
     "count": {"type": "integer", "minimum": 1, "maximum": 3, "description": "2 for a double click"}},
     ["x", "y"], lambda args: {"action": "click", **args}),
    ("drag", "Press at x, y, move to toX, toY and let go.",
     {**POINT, "toX": {"type": "integer", "minimum": 0}, "toY": {"type": "integer", "minimum": 0}, "button": BUTTON},
     ["x", "y", "toX", "toY"], lambda args: {"action": "drag", **args}),
    ("type", "Type text as keystrokes into whatever has the focus.",
     {"text": {"type": "string"}}, ["text"], lambda args: {"action": "type", **args}),
    ("key", "Press keys: an X key name or a chord such as Return, Escape or ctrl+c; several separated by spaces.",
     {"keys": {"type": "string"}}, ["keys"], lambda args: {"action": "key", **args}),
    ("scroll", "Move the pointer to x, y and turn the wheel.",
     {**POINT, "direction": {"type": "string", "enum": sorted(WHEEL)},
      "amount": {"type": "integer", "minimum": 1, "maximum": MOST_REPEAT}},
     ["x", "y"], lambda args: {"action": "scroll", **args}),
    ("cursor_position", "Where the pointer is.", {}, [], lambda args: {"action": "cursor"}),
]


def tool_list():
    width, height = size()
    return [{"name": name,
             "description": f"{description} The screen is {width}x{height}; x is from 0 at the left, "
                            f"y from 0 at the top.",
             "inputSchema": {"type": "object", "properties": properties, "required": required}}
            for name, description, properties, required, _ in TOOLS]


def call(name, args):
    if name == "screenshot":
        data = base64.b64encode(screenshot_bytes()).decode()
        return [{"type": "image", "data": data, "mimeType": "image/png"}]
    for tool, _, _, _, to_action in TOOLS:
        if tool == name:
            done = act(to_action(args or {}))
            return [{"type": "text", "text": "done; the pointer is at %d, %d" % tuple(done["cursor"])}]
    raise Refused(f"no tool {name}")


def mcp():
    """A Model Context Protocol server on stdin and stdout, a JSON-RPC
    message a line."""
    for line in sys.stdin:
        if not line.strip():
            continue
        try:
            message = json.loads(line)
        except ValueError:
            reply = {"jsonrpc": "2.0", "id": None, "error": {"code": -32700, "message": "not JSON"}}
            print(json.dumps(reply), flush=True)
            continue
        if "id" not in message:
            continue  # a notification: nothing to answer
        method, params = message.get("method"), message.get("params") or {}
        reply = {"jsonrpc": "2.0", "id": message["id"]}
        if method == "initialize":
            reply["result"] = {
                "protocolVersion": params.get("protocolVersion", "2025-06-18"),
                "capabilities": {"tools": {}},
                "serverInfo": {"name": "hv2-computer", "version": "1"},
            }
        elif method == "ping":
            reply["result"] = {}
        elif method == "tools/list":
            try:
                reply["result"] = {"tools": tool_list()}
            except Refused as why:
                reply["error"] = {"code": -32603, "message": f"the desktop is not there: {why}"}
        elif method == "tools/call":
            try:
                reply["result"] = {"content": call(params.get("name"), params.get("arguments"))}
            except Refused as why:
                reply["result"] = {"content": [{"type": "text", "text": str(why)}], "isError": True}
        else:
            reply["error"] = {"code": -32601, "message": f"no method {method}"}
        print(json.dumps(reply), flush=True)


def main(argv):
    try:
        if argv[1:] == ["size"]:
            width, height = size()
            print(json.dumps({"width": width, "height": height}))
        elif len(argv) == 3 and argv[1] == "screenshot":
            screenshot(argv[2])
        elif len(argv) == 3 and argv[1] == "act":
            try:
                action = json.loads(argv[2])
            except ValueError:
                raise Refused("the action is not JSON")
            print(json.dumps(act(action)))
        elif argv[1:] == ["mcp"]:
            mcp()
        else:
            print(__doc__, file=sys.stderr)
            return 64
    except Refused as why:
        print(why, file=sys.stderr)
        return 2
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
